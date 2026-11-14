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
    ownership::Ownership,
    protocol::{Block, params::genesis},
    render,
    settings::{MiningThreads, Settings, available_threads},
};
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
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
    identity: Option<crate::protocol::PublicKey>,
}
pub struct Node {
    config: Config,
    network: Option<peer::Network>,
    interactive: bool,
    logs: VecDeque<String>,
    log: Option<std::fs::File>,
    key: SigningKey,
    chain: Chain,
    owned: Ownership,
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
        let events = self.events.clone();
        let generation = self.generation;
        let pending = Arc::new(AtomicBool::new(false));
        self.mining_result_pending = pending.clone();
        let threads = self.config.threads.unwrap_or_default().resolved();
        match mining::start(block, threads, move |block| {
            // Never block shutdown on a full coordinator queue.
            if events.try_send(Event::Mined { generation, block }).is_ok() {
                pending.store(true, Ordering::Release);
            }
        }) {
            Ok(job) => self.job = Some(job),
            Err(error) => {
                self.mining = false;
                self.note(format!(
                    "Mining stopped: could not start {threads} workers: {error}"
                ));
            }
        }
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
                    self.owned
                        .update(&self.chain, crypto::public_key(&self.key));
                    let detached = self.chain.detached(old_tip);
                    self.pool.reconcile(&self.chain.head().state, detached);
                    self.restart_miner();
                    self.broadcast(self.tip());
                    self.note(format!(
                        "tip {} height {}",
                        self.chain.tip,
                        self.chain.head().block.height
                    ));
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
                self.note(format!("rejected block {hash}: {error}"));
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
                self.note(format!("rejected handshake on session {id}"));
                return;
            }
            if !peer.ready {
                let peer = self.peers.get_mut(&id).unwrap();
                peer.ready = true;
                peer.identity = Some(identity);
                self.note(format!("peer session {id} ready: {identity}"));
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
            && self.job.as_ref().is_none_or(|job| job.is_finished())
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
            "mining_thread_setting": self.config.threads.unwrap_or_default().to_string(),
            "mining_threads": self.config.threads.unwrap_or_default().resolved(),
            "available_threads": available_threads(),
            "active_mining_threads": if self.mining { self.job.as_ref().map_or(0, Job::worker_count) } else { 0 },
            "online": self.network.is_some(), "listen": self.config.listen,
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
    fn note(&mut self, message: String) {
        if !self.interactive {
            eprintln!("{message}");
        }
        if let Some(log) = &mut self.log {
            let _ = writeln!(log, "{} {message}", now());
        }
        self.logs.push_back(message);
        while self.logs.len() > 500 {
            self.logs.pop_front();
        }
    }
    fn response(&mut self, message: String) {
        if !self.interactive {
            println!("{message}");
        }
        // Log responses without printing them a second time to stderr.
        if let Some(log) = &mut self.log {
            let _ = writeln!(log, "{} {message}", now());
        }
        self.logs.push_back(message);
        while self.logs.len() > 500 {
            self.logs.pop_front();
        }
    }
    async fn set_online(&mut self, online: bool) -> Result<()> {
        if online == self.network.is_some() {
            return Ok(());
        }
        if online {
            self.network = Some(
                peer::start(
                    self.config.listen,
                    self.config.peers.clone(),
                    self.events.clone(),
                    !self.config.no_lag,
                    self.config.lag_seed,
                )
                .await?,
            );
            self.note("Network ONLINE: listener and peer reconnection enabled".into());
        } else {
            self.peers.clear();
            if let Some(network) = self.network.take() {
                network.shutdown().await;
            }
            self.note("Network OFFLINE: all sockets closed; local mining is independent".into());
        }
        Ok(())
    }
    fn debug(&self, page: usize) -> String {
        match page {
            0 => serde_json::to_string_pretty(&self.status()).unwrap_or_default(),
            1 => {
                let sessions: Vec<_> = self
                    .peers
                    .iter()
                    .map(|(id, p)| json!({"session":id,"ready":p.ready,"identity":p.identity}))
                    .collect();
                serde_json::to_string_pretty(&json!({"online": self.network.is_some(), "listen":self.config.listen,"configured_peers": self.config.peers,"sessions":sessions})).unwrap_or_default()
            }
            2 => {
                let mut hash = self.chain.tip;
                let mut blocks = Vec::new();
                for _ in 0..100 {
                    let block = &self.chain.entries[&hash].block;
                    blocks.push(json!({"hash":hash,"block":block}));
                    if block.height == 0 {
                        break;
                    }
                    hash = block.previous;
                }
                serde_json::to_string_pretty(&json!({"total_entries_including_forks":self.chain.entries.len(),"recent_canonical_blocks":blocks})).unwrap_or_default()
            }
            3 => serde_json::to_string_pretty(&self.pool.transactions).unwrap_or_default(),
            4 => serde_json::to_string_pretty(
                &json!({"wanted":self.sync.wanted,"waiting":self.sync.waiting}),
            )
            .unwrap_or_default(),
            _ => format!(
                "{:#?}\n\nData: {}\nNode log: {}\nDiagnostics: {}\n\nChain lives in memory. JSON exports are diagnostics, not persistence.\nIdentity seed is private and is never displayed here.",
                self.config,
                self.config.data_dir.display(),
                self.config.data_dir.join("node.log").display(),
                self.config.data_dir.join("diagnostics").display()
            ),
        }
    }
    fn set_threads(&mut self, threads: MiningThreads) -> Result<()> {
        Settings {
            mining_threads: threads,
        }
        .save(&self.config.data_dir)?;
        let changed = self.config.threads != Some(threads)
            || self
                .job
                .as_ref()
                .is_some_and(|job| job.worker_count() != threads.resolved());
        self.config.threads = Some(threads);
        if changed && self.mining {
            self.restart_miner();
        }
        self.note(format!(
            "Mining threads: {threads} ({} workers); saved to settings.json",
            threads.resolved()
        ));
        Ok(())
    }
    async fn command(&mut self, line: &str) -> Result<bool> {
        let args: Vec<_> = line.split_whitespace().collect();
        match args.as_slice() {
            [] => {}
            ["quit"] => return Ok(false),
            ["help"] => self.response(crate::cli::HELP.into()),
            ["status"] => self.response(self.status().to_string()),
            ["identity"] => self.response(crypto::public_key(&self.key).to_string()),
            ["owned"] => self.response(self.owned.json(&self.chain.head().state).to_string()),
            ["mining", mode @ ("on" | "off")] => {
                self.mining = *mode == "on";
                self.restart_miner();
                self.note(format!(
                    "Mining {}",
                    if self.mining {
                        "ON"
                    } else {
                        "OFF (worker stopped)"
                    }
                ));
            }
            ["threads", count] => self.set_threads(count.parse()?)?,
            ["network", mode @ ("on" | "off")] => self.set_online(*mode == "on").await?,
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
                self.response(json!({"submitted":tx.id()}).to_string());
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
    run_with_ui(config, false).await
}

pub async fn run_with_ui(mut config: Config, interactive: bool) -> Result<()> {
    // Validate the terminal before creating keys or opening network sockets.
    let mut ui = if interactive {
        Some(crate::tui::App::new()?)
    } else {
        None
    };
    std::fs::create_dir_all(&config.data_dir)?;
    if config.threads.is_none() {
        config.threads = Some(Settings::load(&config.data_dir)?.mining_threads);
    }
    let key = match &config.key {
        Some(path) => crypto::load_key(path)?,
        None => crypto::load_or_create_key(&config.data_dir.join("identity.seed"))?,
    };
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config.data_dir.join("node.log"))?;
    let (events, mut receiver) = mpsc::channel(1024);
    let network = if config.offline {
        None
    } else {
        Some(
            peer::start(
                config.listen,
                config.peers.clone(),
                events.clone(),
                !config.no_lag,
                config.lag_seed,
            )
            .await?,
        )
    };
    let mut node = Node {
        mining: config.mine,
        config,
        network,
        interactive,
        logs: VecDeque::new(),
        log: Some(log),
        key,
        chain: Chain::default(),
        owned: Ownership::default(),
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
    node.note(format!(
        "identity {} {} {} lag={}\n{}",
        crypto::public_key(&node.key),
        if node.network.is_some() {
            "listening"
        } else {
            "offline; configured address"
        },
        node.config.listen,
        !node.config.no_lag,
        crate::cli::HELP
    ));
    node.export()?;
    node.restart_miner();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdin_open = !interactive;
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    let mut frames = tokio::time::interval(Duration::from_millis(100));
    frames.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let outcome: Result<()> = async {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = ticker.tick() => node.periodic()?,
                _ = frames.tick(), if interactive => {
                    let app = ui.as_mut().expect("interactive UI");
                    if let Err(error) = app.runner.poll() { node.note(format!("diagnostic: {error}")); }
                    if let Some(command) = app.input(node.mining, node.network.is_some(), &node.config.data_dir, &node.owned.parcels, node.config.threads.unwrap_or_default())? {
                        let result = node.command(&command).await;
                        app.command_result(&command, &result, node.mining);
                        match result {
                            Ok(false) => break,
                            Ok(true) => {},
                            Err(error) => node.note(format!("command rejected: {error}")),
                        }
                    }
                    let pending = node.pool.pending_paints(crypto::public_key(&node.key));
                    app.draw(crate::tui::View {
                        status: node.status(), state: &node.chain.head().state,
                        owned: &node.owned.parcels,
                        pending: &pending,
                        logs: &node.logs, debug: if app.needs_debug() { node.debug(app.debug_page) } else { String::new() },
                    })?;
                },
                line = lines.next_line(), if stdin_open => match line? {
                    Some(line) => match node.command(&line).await {
                        Ok(false) => break,
                        Ok(true) => {},
                        Err(error) => node.note(format!("command rejected: {error}")),
                    },
                    None => stdin_open = false,
                },
                Some(event) = receiver.recv() => match event {
                    Event::Connected { id, sender, close } => {
                        // Old Connected events can remain queued across an
                        // offline/online toggle; their transports are closed.
                        if node.network.is_some() && !sender.is_closed() {
                            node.peers.insert(id, Connection { sender, ready: false, _close: close, identity: None });
                            node.send(id, node.hello());
                        }
                    }
                    Event::Disconnected { id } => {
                        if node.peers.remove(&id).is_some() {
                            node.note(format!("peer session {id} disconnected"));
                        }
                    },
                    Event::Message { id, message } => node.message(id, message),
                    Event::Mined { generation, block } => node.mined(generation, block),
                },
            }
        }
        Ok(())
    }.await;
    // Always clean up, even when rendering, input, or exporting failed.
    if let Some(app) = &mut ui {
        app.runner.cancel();
    }
    node.mining = false;
    if let Some(job) = node.job.take() {
        job.stop();
    }
    node.peers.clear();
    if let Some(network) = node.network.take() {
        network.shutdown().await;
    }
    let exported = node.export();
    outcome.and(exported)
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
                offline: true,
                threads: None,
            },
            network: None,
            interactive: false,
            logs: VecDeque::new(),
            log: None,
            key: SigningKey::from_bytes(&[1; 32]),
            chain: Chain::default(),
            owned: Ownership::default(),
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
            handles: vec![std::thread::spawn(|| {})],
        });
        while !node.job.as_ref().unwrap().is_finished() {
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
                identity: None,
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

    #[test]
    fn live_thread_changes_replace_the_pool_and_preserve_offline_and_stopped_modes() {
        let directory =
            std::env::temp_dir().join(format!("pixel-thread-change-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut node = node();
        node.config.data_dir = directory.clone();
        node.set_threads("2".parse().unwrap()).unwrap();
        assert!(!node.mining && node.job.is_none() && node.network.is_none());
        node.mining = true;
        node.restart_miner();
        assert_eq!(node.job.as_ref().unwrap().worker_count(), 2);
        let generation = node.generation;
        node.set_threads("3".parse().unwrap()).unwrap();
        assert_eq!(node.job.as_ref().unwrap().worker_count(), 3);
        assert!(
            node.generation > generation,
            "queued results from the old pool must become stale"
        );
        assert!(node.network.is_none());
        node.mining = false;
        node.restart_miner();
        node.set_threads(MiningThreads::All).unwrap();
        assert!(node.job.is_none() && !node.mining);
        assert_eq!(
            Settings::load(&directory).unwrap().mining_threads,
            MiningThreads::All
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
