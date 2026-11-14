use clap::Args;
use std::{net::SocketAddr, num::NonZeroU16, path::PathBuf};
#[derive(Debug, Clone, Args)]
pub struct Config {
    /// Bind address. Overrides the saved port; defaults to 0.0.0.0:7001.
    #[arg(long, conflicts_with = "port")]
    pub listen: Option<SocketAddr>,
    /// TCP port on all IPv4 interfaces. Overrides the saved port for this run.
    #[arg(long)]
    pub port: Option<NonZeroU16>,
    /// Enable the optional LAN UDP peer discovery extension.
    #[arg(long)]
    pub discover: bool,
    #[arg(long, default_value = "7000")]
    pub discover_port: NonZeroU16,
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
            listen: None,
            port: None,
            discover: false,
            discover_port: NonZeroU16::new(7000).unwrap(),
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
