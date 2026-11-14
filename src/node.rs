use crate::{
    Error, Result,
    chain::Chain,
    config::Config,
    crypto,
    mempool::Mempool,
    mining::{self, Job},
    network::{
        peer::{self, Event},
        sync::Sync,
        wire::{Message, VERSION},
    },
    protocol::{Block, params::genesis},
    render,
};
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
struct Connection {
    sender: mpsc::Sender<Message>,
    ready: bool,
    _close: tokio::sync::oneshot::Sender<()>,
}
pub struct Node {
    config: Config,
    key: SigningKey,
    chain: Chain,
    pool: Mempool,
    sync: Sync,
    peers: BTreeMap<u64, Connection>,
    events: mpsc::Sender<Event>,
    mining: bool,
    generation: u64,
    job: Option<Job>,
    mining_result_pending: Arc<AtomicBool>,
    announcement_cursor: usize,
    request_cursor: usize,
}
impl Node {
    fn hello(&self) -> Message {
        Message::Hello {
            version: VERSION,
            genesis: genesis().hash(),
            identity: crypto::public_key(&self.key),
        }
    }
    fn tip(&self) -> Message {
        Message::Tip {
            hash: self.chain.tip,
            height: self.chain.head().block.height,
        }
    }
    fn send(&mut self, id: u64, message: Message) {
        if let Some(peer) = self.peers.get(&id)
            && peer.sender.try_send(message).is_err()
        {
            self.peers.remove(&id);
        }
    }
    fn broadcast(&mut self, message: Message) {
        let ids: Vec<_> = self
            .peers
            .iter()
            .filter(|(_, p)| p.ready)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.send(id, message.clone());
        }
    }
    fn restart_miner(&mut self) {
        if let Some(job) = self.job.take() {
            job.stop();
        }
        self.generation = self.generation.wrapping_add(1);
        if !self.mining {
            return;
        }
        let head = self.chain.head();
        let Some(height) = head.block.height.checked_add(1) else {
            return;
        };
        let block = Block {
            previous: self.chain.tip,
            height,
            miner: crypto::public_key(&self.key),
            timestamp: now().max(head.block.timestamp),
            nonce: 0,
            transactions: self.pool.select(&head.state),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let events = self.events.clone();
        let generation = self.generation;
        let pending = Arc::new(AtomicBool::new(false));
        self.mining_result_pending = pending.clone();
        let handle = std::thread::spawn(move || {
            if let Some(block) = mining::mine(block, &worker_cancel) {
                // Never block shutdown on a full coordinator queue.
                if events.try_send(Event::Mined { generation, block }).is_ok() {
                    pending.store(true, Ordering::Release);
                }
            }
        });
        self.job = Some(Job { cancel, handle });
    }
    fn accept(&mut self, block: Block) {
        let hash = block.hash();
        let old_tip = self.chain.tip;
        let known = self.chain.entries.contains_key(&hash);
        match self.chain.insert(block.clone(), now()) {
            Ok(changed) => {
                self.sync.wanted.remove(&hash);
                if !known {
                    self.broadcast(Message::Block { block });
                }
                if changed {
                    let detached = self.chain.detached(old_tip);
                    self.pool.reconcile(&self.chain.head().state, detached);
                    self.restart_miner();
                    self.broadcast(self.tip());
                    eprintln!(
                        "tip {} height {}",
                        self.chain.tip,
                        self.chain.head().block.height
                    );
                }
            }
            Err(Error::UnknownParent(parent)) => {
                if block.transactions.len() <= crate::protocol::params::MAX_TRANSACTIONS
                    && crate::protocol::validation::valid_pow(hash)
                {
                    self.sync.wanted.remove(&hash);
                    self.sync.defer(block);
                    self.sync.want_parent(parent);
                }
            }
            Err(Error::Future) => {
                if crate::protocol::validation::valid_pow(hash) {
                    self.sync.wanted.remove(&hash);
                    self.sync.defer(block);
                }
            }
            Err(error) => {
                self.sync.wanted.remove(&hash);
                eprintln!("rejected block {hash}: {error}");
            }
        }
    }
    fn mined(&mut self, generation: u64, block: Block) {
        if generation == self.generation && self.mining && block.previous == self.chain.tip {
            self.mining_result_pending.store(false, Ordering::Release);
            self.accept(block);
            self.retry_waiting();
        }
    }
    fn retry_waiting(&mut self) {
        loop {
            let ready: Vec<_> = self
                .sync
                .waiting
                .iter()
                .filter(|(_, b)| {
                    self.chain.entries.contains_key(&b.previous)
                        && b.timestamp
                            <= now().saturating_add(crate::protocol::params::MAX_FUTURE_SECONDS)
                })
                .map(|(h, _)| *h)
                .collect();
            if ready.is_empty() {
                break;
            }
            for hash in ready {
                if let Some(block) = self.sync.waiting.remove(&hash) {
                    self.accept(block);
                }
            }
        }
    }
    fn message(&mut self, id: u64, message: Message) {
        let Some(peer) = self.peers.get(&id) else {
            return;
        };
        if let Message::Hello {
            version,
            genesis: chain,
            identity,
        } = message
        {
            if version != VERSION
                || chain != genesis().hash()
                || crypto::valid_key(identity).is_err()
                || identity == crypto::public_key(&self.key)
            {
                self.peers.remove(&id);
                return;
            }
            if !peer.ready {
                self.peers.get_mut(&id).unwrap().ready = true;
                self.send(id, self.hello());
                self.send(id, self.tip());
                self.send(id, Message::GetTip {});
            }
            return;
        }
        if !peer.ready {
            return;
        }
        match message {
            Message::GetTip {} => self.send(id, self.tip()),
            Message::Tip { hash, .. } => {
                if !self.chain.entries.contains_key(&hash) && !self.sync.waiting.contains_key(&hash)
                {
                    self.sync.want(hash);
                    self.send(id, Message::GetBlock { hash });
                }
            }
            Message::GetBlock { hash } => {
                if let Some(entry) = self.chain.entries.get(&hash) {
                    self.send(
                        id,
                        Message::Block {
                            block: entry.block.clone(),
                        },
                    );
                }
            }
            Message::Block { block } => {
                let parent = block.previous;
                self.accept(block);
                self.retry_waiting();
                if self.sync.wanted.contains(&parent) {
                    self.send(id, Message::GetBlock { hash: parent });
                }
            }
            Message::Transaction { transaction } => {
                if matches!(
                    self.pool
                        .admit(transaction.clone(), &self.chain.head().state),
                    Ok(true)
                ) {
                    self.broadcast(Message::Transaction { transaction });
                    self.restart_miner();
                }
            }
            Message::Hello { .. } => {}
        }
    }
    fn periodic(&mut self) -> Result<()> {
        self.retry_waiting();
        let hello = self.hello();
        let ids: Vec<_> = self.peers.keys().copied().collect();
        for id in ids {
            self.send(id, hello.clone());
        }
        self.broadcast(Message::GetTip {});
        self.broadcast(self.tip());
        self.sync
            .wanted
            .retain(|hash| !self.chain.entries.contains_key(hash));
        let missing: Vec<_> = self
            .sync
            .waiting
            .values()
            .filter(|b| {
                !self.chain.entries.contains_key(&b.previous)
                    && !self.sync.waiting.contains_key(&b.previous)
            })
            .map(|b| b.previous)
            .collect();
        for hash in missing {
            self.sync.want_parent(hash);
        }
        let requests: Vec<_> = self.sync.wanted.iter().copied().collect();
        let len = requests.len();
        let wanted: Vec<_> = (0..len.min(4))
            .map(|offset| requests[(self.request_cursor + offset) % len])
            .collect();
        self.request_cursor = if len == 0 {
            0
        } else {
            (self.request_cursor + 4) % len
        };
        for hash in wanted {
            self.broadcast(Message::GetBlock { hash });
        }
        // Anti-entropy recovers dropped transaction announcements too.
        let len = self.pool.transactions.len();
        let transactions: Vec<_> = (0..len.min(4))
            .map(|offset| self.pool.transactions[(self.announcement_cursor + offset) % len].clone())
            .collect();
        self.announcement_cursor = if len == 0 {
            0
        } else {
            (self.announcement_cursor + 4) % len
        };
        for transaction in transactions {
            self.broadcast(Message::Transaction { transaction });
        }
        self.maintain_miner();
        self.export()
    }
    fn maintain_miner(&mut self) {
        if self.mining
            && self.job.as_ref().is_none_or(|job| job.handle.is_finished())
            && !self.mining_result_pending.load(Ordering::Acquire)
        {
            self.restart_miner();
        }
    }
    fn status(&self) -> serde_json::Value {
        let identity = crypto::public_key(&self.key);
        let state = &self.chain.head().state;
        json!({"identity": identity, "genesis": genesis().hash(), "tip": self.chain.tip,
            "height": self.chain.head().block.height,
            "work": (u128::from(self.chain.head().block.height) << crate::protocol::params::DIFFICULTY).to_string(),
            "state_hash": state.diagnostic_hash(), "mining": self.mining,
            "lag": !self.config.no_lag, "connections": self.peers.values().filter(|p| p.ready).count(),
            "mempool": self.pool.transactions.len(), "waiting": self.sync.waiting.len(),
            "owned": state.parcels.iter().enumerate().filter(|(_, p)| p.owner == Some(identity)).map(|(id, _)| id).collect::<Vec<_>>()})
    }
    fn atomic_json(path: &Path, value: &serde_json::Value) -> Result<()> {
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
    fn export(&self) -> Result<()> {
        Self::atomic_json(&self.config.data_dir.join("status.json"), &self.status())?;
        Self::atomic_json(
            &self.config.data_dir.join("state.json"),
            &json!({"genesis":genesis().hash(), "tip":self.chain.tip, "state":self.chain.head().state}),
        )?;
        let tmp = self.config.data_dir.join("canvas.tmp");
        render::ppm(&self.chain.head().state, &tmp)?;
        std::fs::rename(tmp, self.config.data_dir.join("canvas.ppm"))?;
        Ok(())
    }
    fn command(&mut self, line: &str) -> Result<bool> {
        let args: Vec<_> = line.split_whitespace().collect();
        match args.as_slice() {
            [] => {}
            ["quit"] => return Ok(false),
            ["help"] => println!("{}", crate::cli::HELP),
            ["status"] => println!("{}", self.status()),
            ["identity"] => println!("{}", crypto::public_key(&self.key)),
            ["mining", mode @ ("on" | "off")] => {
                self.mining = *mode == "on";
                self.restart_miner();
            }
            ["export"] => self.export()?,
            ["export", path] => render::ppm(&self.chain.head().state, Path::new(path))?,
            ["paint" | "transfer", parcel, _] => {
                let parcel: usize = parcel
                    .parse()
                    .map_err(|_| Error::Invalid("invalid parcel"))?;
                let projected = self.pool.projected(&self.chain.head().state);
                let counter = projected
                    .parcels
                    .get(parcel)
                    .ok_or(Error::Invalid("parcel out of range"))?
                    .counter;
                let mut tx = crate::cli::transaction(line, counter)?;
                crypto::sign(&mut tx, &self.key);
                self.pool.admit(tx.clone(), &self.chain.head().state)?;
                println!("{}", json!({"submitted":tx.id()}));
                self.broadcast(Message::Transaction { transaction: tx });
                self.restart_miner();
            }
            _ => return Err(Error::Invalid(crate::cli::HELP)),
        }
        self.export()?;
        Ok(true)
    }
}
pub async fn run(config: Config) -> Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;
    let key = match &config.key {
        Some(path) => crypto::load_key(path)?,
        None => crypto::load_or_create_key(&config.data_dir.join("identity.seed"))?,
    };
    let (events, mut receiver) = mpsc::channel(1024);
    let _network = peer::start(
        config.listen,
        config.peers.clone(),
        events.clone(),
        !config.no_lag,
        config.lag_seed,
    )
    .await?;
    let mut node = Node {
        mining: config.mine,
        config,
        key,
        chain: Chain::default(),
        pool: Mempool::default(),
        sync: Sync::default(),
        peers: BTreeMap::new(),
        events,
        generation: 0,
        job: None,
        mining_result_pending: Arc::new(AtomicBool::new(false)),
        announcement_cursor: 0,
        request_cursor: 0,
    };
    eprintln!(
        "identity {} listening {} lag={}\n{}",
        crypto::public_key(&node.key),
        node.config.listen,
        !node.config.no_lag,
        crate::cli::HELP
    );
    node.export()?;
    node.restart_miner();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdin_open = true;
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    let outcome: Result<()> = async {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = ticker.tick() => node.periodic()?,
                line = lines.next_line(), if stdin_open => match line? {
                    Some(line) => match node.command(&line) {
                        Ok(false) => break,
                        Ok(true) => {},
                        Err(error) => eprintln!("command rejected: {error}"),
                    },
                    None => stdin_open = false,
                },
                Some(event) = receiver.recv() => match event {
                    Event::Connected { id, sender, close } => {
                        node.peers.insert(id, Connection { sender, ready: false, _close: close });
                        node.send(id, node.hello());
                    }
                    Event::Disconnected { id } => { node.peers.remove(&id); },
                    Event::Message { id, message } => node.message(id, message),
                    Event::Mined { generation, block } => node.mined(generation, block),
                },
            }
        }
        Ok(())
    }
    .await;
    if let Some(job) = node.job.take() {
        job.stop();
    }
    outcome?;
    node.export()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> Node {
        let (events, _) = mpsc::channel(16);
        Node {
            config: Config {
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: Vec::new(),
                data_dir: std::path::PathBuf::new(),
                key: None,
                no_lag: true,
                lag_seed: 1,
                mine: false,
            },
            key: SigningKey::from_bytes(&[1; 32]),
            chain: Chain::default(),
            pool: Mempool::default(),
            sync: Sync::default(),
            peers: BTreeMap::new(),
            events,
            mining: false,
            generation: 0,
            job: None,
            mining_result_pending: Arc::new(AtomicBool::new(false)),
            announcement_cursor: 0,
            request_cursor: 0,
        }
    }

    #[test]
    fn queued_mining_result_survives_periodic_worker_maintenance() {
        let mut node = node();
        node.mining = true;
        node.generation = 42;
        node.mining_result_pending.store(true, Ordering::Release);
        node.job = Some(Job {
            cancel: Arc::new(AtomicBool::new(false)),
            handle: std::thread::spawn(|| {}),
        });
        while !node.job.as_ref().unwrap().handle.is_finished() {
            std::thread::yield_now();
        }
        node.maintain_miner();
        assert_eq!(
            node.generation, 42,
            "queued result must keep its generation"
        );
        // If result delivery failed, restarting is necessary for progress.
        node.mining_result_pending.store(false, Ordering::Release);
        node.maintain_miner();
        assert_eq!(node.generation, 43);
        node.job.take().unwrap().stop();
    }

    #[test]
    fn stale_mining_result_cannot_extend_the_chain() {
        let mut node = node();
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/prototype-v1.json")).unwrap();
        let block: Block = serde_json::from_value(vectors["one"]["block"].clone()).unwrap();
        node.mining = true;
        node.generation = 2;
        node.mined(1, block.clone());
        assert_eq!(node.chain.tip, genesis().hash());
        node.mining = false;
        node.mined(2, block);
        assert_eq!(node.chain.tip, genesis().hash());
    }

    #[test]
    fn rejected_handshake_cancels_session_and_prehello_messages_are_ignored() {
        let mut node = node();
        let (sender, mut messages) = mpsc::channel(16);
        let (close, mut closed) = tokio::sync::oneshot::channel();
        node.peers.insert(
            1,
            Connection {
                sender,
                ready: false,
                _close: close,
            },
        );
        node.message(
            1,
            Message::Tip {
                hash: crate::protocol::Bytes([9; 32]),
                height: 10,
            },
        );
        assert!(node.sync.wanted.is_empty());
        assert!(messages.try_recv().is_err());
        // An otherwise valid hello using our own identity must close promptly.
        node.message(1, node.hello());
        assert!(node.peers.is_empty());
        assert!(matches!(
            closed.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
    }
}
