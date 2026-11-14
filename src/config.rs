use clap::Args;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Debug, Clone, Args)]
pub struct Config {
    #[arg(long, default_value = "127.0.0.1:7001")]
    pub listen: SocketAddr,
    #[arg(long, value_delimiter = ',')]
    pub peers: Vec<SocketAddr>,
    #[arg(long, default_value = "data/node")]
    pub data_dir: PathBuf,
    #[arg(long)]
    pub key: Option<PathBuf>,
    #[arg(long)]
    pub no_lag: bool,
    /// Deterministic simulation seed (not used for cryptographic keys).
    #[arg(long, default_value_t = 1)]
    pub lag_seed: u64,
    #[arg(long)]
    pub mine: bool,
    /// Start with no listener, connections, gossip, or peer synchronization.
    #[arg(long)]
    pub offline: bool,
    /// CPU mining workers: a count (1..1024) or 'all'. Overrides saved settings.
    #[arg(long)]
    pub threads: Option<crate::settings::MiningThreads>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:7001".parse().expect("default address"),
            peers: Vec::new(),
            data_dir: "data/node".into(),
            key: None,
            no_lag: false,
            lag_seed: 1,
            mine: false,
            offline: false,
            threads: None,
        }
    }
}
