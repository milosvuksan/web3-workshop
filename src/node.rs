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
    settings::{MiningThreads, SavedPeer, Settings, available_threads, peer_address},
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
    sender: peer::Sender,
    address: std::net::SocketAddr,
    ready: bool,
    _close: tokio::sync::oneshot::Sender<()>,
    identity: Option<crate::protocol::PublicKey>,
}
pub struct Node {
    config: Config,
    settings: Settings,
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
    peer_identities: BTreeMap<std::net::SocketAddr, crate::protocol::PublicKey>,
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
        if self.chain.entries.contains_key(&hash) || self.sync.waiting.contains_key(&hash) {
            return;
        }
        match self.chain.insert(block.clone(), now()) {
            Ok(changed) => {
                self.sync.wanted.remove(&hash);
                self.broadcast(Message::Block { block });
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
                self.sync.wanted.remove(&hash);
                self.sync.defer(block);
                self.sync.want_parent(parent);
            }
            Err(Error::Future) => {
                self.sync.wanted.remove(&hash);
                self.sync.defer(block);
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
            if self.peer_addresses().contains(&peer.address) {
                self.peer_identities.insert(peer.address, identity);
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
            "port": self.config.listen.expect("resolved address").port(),
            "discovery": self.config.discover, "discover_port": self.config.discover_port,
            "network_peers": self.peer_status(),
            "network_sessions": self.peers.values().filter(|p| !p.sender.is_closed()).map(|p| json!({"address":p.address,"ready":p.ready,"identity":p.identity,"alias":self.session_alias(p)})).collect::<Vec<_>>(),
            "startup_peers": self.config.peers,
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
                    self.config.listen.expect("resolved address"),
                    self.peer_addresses(),
                    self.events.clone(),
                    !self.config.no_lag,
                    self.config.lag_seed,
                    self.discovery(),
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
                    .map(|(id, p)| json!({"session":id,"ready":p.ready,"identity":p.identity,"address":p.address}))
                    .collect();
                serde_json::to_string_pretty(&json!({"online": self.network.is_some(), "listen":self.config.listen,"configured_peers": self.config.peers,"saved_peers":self.peer_status(),"sessions":sessions})).unwrap_or_default()
            }
            2 => {
                let mut hash = self.chain.tip;
                let mut blocks = Vec::new();
                for _ in 0..100 {
                    let block = &self.chain.entries[&hash].block;
                    blocks.push(json!({"hash":hash,"miner_label":self.miner_label(block.miner),"block":block}));
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
    fn discovery(&self) -> Option<crate::network::discovery::Discovery> {
        self.config
            .discover
            .then(|| crate::network::discovery::Discovery {
                port: self.config.discover_port.get(),
                identity: crypto::public_key(&self.key),
            })
    }
    fn miner_label(&self, miner: crate::protocol::PublicKey) -> String {
        if miner == crypto::public_key(&self.key) {
            return "ME".into();
        }
        self.peers
            .values()
            .find(|p| p.ready && p.identity == Some(miner))
            .map_or_else(
                || miner.to_string()[..8].to_owned(),
                |p| {
                    self.session_alias(p)
                        .unwrap_or_else(|| p.address.to_string())
                },
            )
    }
    fn peer_addresses(&self) -> Vec<std::net::SocketAddr> {
        self.config
            .peers
            .iter()
            .copied()
            .chain(self.settings.peers.iter().map(|p| p.address))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn session_alias(&self, session: &Connection) -> Option<String> {
        self.settings
            .peers
            .iter()
            .find(|peer| {
                peer.address == session.address
                    || session.identity.is_some_and(|identity| {
                        self.peer_identities.get(&peer.address) == Some(&identity)
                    })
            })
            .map(|peer| peer.alias.clone())
    }
    fn peer_status(&self) -> Vec<serde_json::Value> {
        self.settings.peers.iter().map(|saved| {
            let identity = self.peer_identities.get(&saved.address);
            let sessions: Vec<_> = self.peers.values().filter(|p| !p.sender.is_closed()
                && (p.address == saved.address || identity.is_some_and(|key| p.identity == Some(*key)))).collect();
            let ready = sessions.iter().filter(|p| p.ready).count();
            let status = if self.network.is_none() { "Offline" } else if ready > 0 { "Connected" }
                else if !sessions.is_empty() { "Handshaking" } else { "Retrying" };
            json!({"address":saved.address,"alias":saved.alias,"status":status,"sessions":ready,"identity":identity})
        }).collect()
    }
    fn save_peers(&mut self, peers: Vec<SavedPeer>) -> Result<()> {
        let mut settings = self.settings.clone();
        settings.peers = peers;
        settings.save(&self.config.data_dir)?;
        self.settings = settings;
        let addresses = self.peer_addresses();
        if let Some(network) = &self.network {
            network.set_peers(addresses.clone());
        }
        // Cancel removed outgoing sessions immediately; inbound sessions remain welcome.
        self.peers.retain(|_, session| {
            !self.peer_identities.contains_key(&session.address)
                || addresses.contains(&session.address)
        });
        self.note(format!(
            "Saved {} peers to settings.json",
            self.settings.peers.len()
        ));
        Ok(())
    }
    fn add_peer(&mut self, old: Option<std::net::SocketAddr>, peer: SavedPeer) -> Result<()> {
        let mut peers = self.settings.peers.clone();
        if let Some(old) = old {
            let index = peers
                .iter()
                .position(|p| p.address == old)
                .ok_or(Error::Invalid("saved peer not found"))?;
            if peers
                .iter()
                .any(|p| p.address == peer.address && p.address != old)
            {
                return Err(Error::Invalid("a peer with this address is already saved"));
            }
            peers[index] = peer;
        } else if let Some(existing) = peers.iter_mut().find(|p| p.address == peer.address) {
            *existing = peer;
        } else {
            peers.push(peer);
        }
        self.save_peers(peers)
    }
    async fn set_port(&mut self, port: std::num::NonZeroU16) -> Result<()> {
        let mut address = self.config.listen.expect("resolved address");
        address.set_port(port.get());
        // Bind first so a busy port leaves the existing network intact.
        let replacement = if self.network.is_some() && self.config.listen != Some(address) {
            Some(
                peer::start(
                    address,
                    self.peer_addresses(),
                    self.events.clone(),
                    !self.config.no_lag,
                    self.config.lag_seed,
                    self.discovery(),
                )
                .await?,
            )
        } else {
            None
        };
        let mut settings = self.settings.clone();
        settings.port = port;
        if let Err(error) = settings.save(&self.config.data_dir) {
            if let Some(network) = replacement {
                network.shutdown().await;
            }
            return Err(error);
        }
        if let Some(network) = replacement {
            self.peers.clear();
            if let Some(old) = self.network.replace(network) {
                old.shutdown().await;
            }
        }
        self.settings = settings;
        self.config.listen = Some(address);
        self.note(format!("TCP port: {port}; saved to settings.json"));
        Ok(())
    }
    fn set_threads(&mut self, threads: MiningThreads) -> Result<()> {
        let mut settings = self.settings.clone();
        settings.mining_threads = threads;
        settings.save(&self.config.data_dir)?;
        self.settings = settings;
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
            ["peer", "add", address, alias @ ..] => self.add_peer(
                None,
                SavedPeer::new(peer_address(address)?, &alias.join(" "))?,
            )?,
            ["peer", "edit", old, address, alias @ ..] => self.add_peer(
                Some(peer_address(old)?),
                SavedPeer::new(peer_address(address)?, &alias.join(" "))?,
            )?,
            ["peer", "remove", address] => {
                let address = peer_address(address)?;
                let peers: Vec<_> = self
                    .settings
                    .peers
                    .iter()
                    .filter(|p| p.address != address)
                    .cloned()
                    .collect();
                if peers.len() == self.settings.peers.len() {
                    return Err(Error::Invalid("saved peer not found"));
                }
                self.save_peers(peers)?;
            }
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
            ["port", port] => {
                self.set_port(
                    port.parse()
                        .map_err(|_| Error::Invalid("port must be 1–65535"))?,
                )
                .await?
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
    let settings = Settings::load(&config.data_dir)?;
    if config.threads.is_none() {
        config.threads = Some(settings.mining_threads);
    }
    if config.listen.is_none() {
        config.listen = Some(std::net::SocketAddr::from((
            [0, 0, 0, 0],
            config.port.unwrap_or(settings.port).get(),
        )));
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
                config.listen.expect("resolved address"),
                config
                    .peers
                    .iter()
                    .copied()
                    .chain(settings.peers.iter().map(|p| p.address))
                    .collect(),
                events.clone(),
                !config.no_lag,
                config.lag_seed,
                config
                    .discover
                    .then(|| crate::network::discovery::Discovery {
                        port: config.discover_port.get(),
                        identity: crypto::public_key(&key),
                    }),
            )
            .await?,
        )
    };
    let mut node = Node {
        mining: config.mine,
        config,
        settings,
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
        peer_identities: BTreeMap::new(),
    };
    node.note(format!(
        "identity {} {} {} lag={}\n{}",
        crypto::public_key(&node.key),
        if node.network.is_some() {
            "listening"
        } else {
            "offline; configured address"
        },
        node.config.listen.expect("resolved address"),
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
                    if let Some(command) = app.input(crate::tui::Input {
                        mining: node.mining, online: node.network.is_some(), data_dir: &node.config.data_dir,
                        owned: &node.owned.parcels, threads: node.config.threads.unwrap_or_default(),
                        port: node.config.listen.expect("resolved address").port(), saved_peers: &node.settings.peers,
                    })? {
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
                    Event::Connected { id, sender, close, address } => {
                        // Old Connected events can remain queued across an
                        // offline/online toggle; their transports are closed.
                        if node.network.is_some() && !sender.is_closed() {
                            node.peers.insert(id, Connection { sender, address, ready: false, _close: close, identity: None });
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
                listen: Some("127.0.0.1:0".parse().unwrap()),
                no_lag: true,
                offline: true,
                ..Config::default()
            },
            settings: Settings::default(),
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
            peer_identities: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn saved_peer_commands_are_atomic_and_preserve_other_preferences() {
        let directory =
            std::env::temp_dir().join(format!("pixel-saved-peers-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut node = node();
        node.config.data_dir = directory.clone();
        node.command("peer add 192.0.2.1 Main workshop")
            .await
            .unwrap();
        node.command("peer add [::1]:7002 Local IPv6")
            .await
            .unwrap();
        node.command("peer add 192.0.2.1 Renamed peer")
            .await
            .unwrap();
        assert_eq!(node.settings.peers.len(), 2);
        assert_eq!(node.settings.peers[0].alias, "Renamed peer");
        assert_eq!(node.peer_status()[0]["status"], "Offline");
        node.command("threads 3").await.unwrap();
        node.command("port 7003").await.unwrap();
        let original = Settings::load(&directory).unwrap();
        for invalid in [
            "peer add bad Nope",
            "peer add 0.0.0.0 Nope",
            "peer edit 192.0.2.1 [::1]:7002 Duplicate",
            "peer remove 192.0.2.99",
        ] {
            assert!(node.command(invalid).await.is_err());
            assert_eq!(Settings::load(&directory).unwrap().peers, original.peers);
        }
        std::fs::create_dir(directory.join("settings.tmp")).unwrap();
        assert!(
            node.command("peer add 192.0.2.7 Failed save")
                .await
                .is_err()
        );
        assert_eq!(node.settings.peers, original.peers);
        std::fs::remove_dir(directory.join("settings.tmp")).unwrap();
        node.command("peer edit 192.0.2.1 192.0.2.2:7010 Moved peer")
            .await
            .unwrap();
        node.command("peer remove [::1]:7002").await.unwrap();
        let saved = Settings::load(&directory).unwrap();
        assert_eq!(saved.peers.len(), 1);
        assert_eq!(saved.peers[0].address.to_string(), "192.0.2.2:7010");
        assert_eq!(saved.peers[0].alias, "Moved peer");
        assert_eq!(saved.port.get(), 7003);
        assert_eq!(saved.mining_threads.to_string(), "3");
        assert!(!node.mining && node.network.is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn port_changes_preserve_settings_and_existing_listener_on_bind_failure() {
        let directory =
            std::env::temp_dir().join(format!("pixel-port-change-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut node = node();
        node.config.data_dir = directory.clone();
        let initial = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let old = initial.local_addr().unwrap();
        drop(initial);
        node.config.listen = Some(old);
        node.set_online(true).await.unwrap();
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = std::num::NonZeroU16::new(occupied.local_addr().unwrap().port()).unwrap();
        assert!(node.set_port(busy).await.is_err());
        assert_eq!(node.config.listen, Some(old));
        assert!(tokio::net::TcpStream::connect(old).await.is_ok());
        assert!(!directory.join("settings.json").exists());
        node.set_online(false).await.unwrap();
        // Offline settings can be changed without trying to bind any socket.
        node.set_port(busy).await.unwrap();
        node.set_threads(MiningThreads::All).unwrap();
        let saved = Settings::load(&directory).unwrap();
        assert_eq!(saved.port, busy);
        assert_eq!(saved.mining_threads, MiningThreads::All);
        assert!(node.network.is_none() && !node.mining);
        drop(occupied);
        node.set_online(true).await.unwrap();
        let next = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = next.local_addr().unwrap();
        drop(next);
        node.set_port(std::num::NonZeroU16::new(address.port()).unwrap())
            .await
            .unwrap();
        assert!(tokio::net::TcpStream::connect(address).await.is_ok());
        assert!(tokio::net::TcpStream::connect(old).await.is_err());
        assert_eq!(
            Settings::load(&directory).unwrap().mining_threads,
            MiningThreads::All
        );
        node.set_online(false).await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unknown_parent_is_requested_before_pow_and_duplicate_waiting_blocks_are_ignored() {
        let mut node = node();
        let block = Block {
            previous: crate::protocol::Bytes([42; 32]),
            height: 2,
            ..genesis()
        };
        let hash = block.hash();
        node.accept(block.clone());
        assert!(node.sync.waiting.contains_key(&hash));
        assert!(node.sync.wanted.contains(&block.previous));
        node.sync.wanted.clear();
        node.accept(block);
        assert!(node.sync.wanted.is_empty());
        assert_eq!(node.sync.waiting.len(), 1);
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
        let (sender, mut messages) = peer::outbox(false, 1);
        let (close, mut closed) = tokio::sync::oneshot::channel();
        node.peers.insert(
            1,
            Connection {
                sender,
                address: "127.0.0.1:1".parse().unwrap(),
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
