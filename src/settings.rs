//! Local runtime preferences, separate from protocol state and exported chain data.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    net::{IpAddr, SocketAddr},
    num::{NonZeroU16, NonZeroUsize},
    path::Path,
    str::FromStr,
};

pub const MAX_MINING_THREADS: usize = 1024;
pub const MIN_MINING_DIFFICULTY: u16 = crate::protocol::params::DIFFICULTY as u16;
pub const MAX_MINING_DIFFICULTY: u16 = 256;
fn deserialize_difficulty<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u16, D::Error> {
    let value = u16::deserialize(deserializer)?;
    if !(MIN_MINING_DIFFICULTY..=MAX_MINING_DIFFICULTY).contains(&value) {
        return Err(serde::de::Error::custom(
            "mining difficulty must be 23–256 leading zero bits",
        ));
    }
    Ok(value)
}
pub fn available_threads() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum MiningThreads {
    All,
    Count(NonZeroUsize),
}
impl Default for MiningThreads {
    fn default() -> Self {
        Self::Count(NonZeroUsize::new(1).expect("one is nonzero"))
    }
}
impl MiningThreads {
    pub fn resolved(self) -> usize {
        match self {
            Self::All => available_threads(),
            Self::Count(count) => count.get(),
        }
    }
}
impl fmt::Display for MiningThreads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Count(count) => count.fmt(f),
        }
    }
}
impl FromStr for MiningThreads {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        value
            .parse::<NonZeroUsize>()
            .ok()
            .filter(|n| n.get() <= MAX_MINING_THREADS)
            .map(Self::Count)
            .ok_or(Error::Invalid(
                "threads must be 'all' or a count from 1 to 1024",
            ))
    }
}
impl TryFrom<String> for MiningThreads {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<MiningThreads> for String {
    fn from(value: MiningThreads) -> Self {
        value.to_string()
    }
}
pub const DEFAULT_PORT: NonZeroU16 = NonZeroU16::new(7001).unwrap();
pub const MAX_SAVED_PEERS: usize = 256;

/// Registry of optional wire/consensus adapters. Add future teams here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityProfile {
    #[default]
    Native,
    Grupa1Luka,
}
impl CompatibilityProfile {
    pub const ALL: [Self; 2] = [Self::Native, Self::Grupa1Luka];
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "None (native)",
            Self::Grupa1Luka => "Grupa 1 - Luka",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Grupa1Luka => "grupa1-luka",
        }
    }
    pub fn default_port(self) -> u16 {
        match self {
            Self::Native => DEFAULT_PORT.get(),
            Self::Grupa1Luka => 8080,
        }
    }
}
impl FromStr for CompatibilityProfile {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.id() == value)
            .ok_or(Error::Invalid(
                "unknown compatibility profile (use native or grupa1-luka)",
            ))
    }
}
pub fn peer_address_for(value: &str, profile: CompatibilityProfile) -> Result<SocketAddr> {
    let value = value
        .strip_prefix("http://")
        .unwrap_or(value)
        .trim_end_matches('/');
    if let Ok(ip) = value.parse::<IpAddr>() {
        return peer_address(&SocketAddr::new(ip, profile.default_port()).to_string());
    }
    peer_address(value)
}
/// CLI aliases remain free text; the profile option precedes them.
pub fn configured_peer(address: &str, fields: &[&str]) -> Result<SavedPeer> {
    configured_peer_with_default(address, fields, CompatibilityProfile::Native)
}
pub fn configured_peer_with_default(
    address: &str,
    fields: &[&str],
    default: CompatibilityProfile,
) -> Result<SavedPeer> {
    let (profile, alias) = match fields {
        ["--compat", profile, rest @ ..] => (profile.parse()?, rest),
        ["--compat"] => return Err(Error::Invalid("--compat requires a profile")),
        rest => (default, rest),
    };
    let mut peer = SavedPeer::new(peer_address_for(address, profile)?, &alias.join(" "))?;
    peer.compatibility = profile;
    Ok(peer)
}

/// A local label and a reachable TCP endpoint, never consensus data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PeerFields")]
pub struct SavedPeer {
    pub address: SocketAddr,
    pub alias: String,
    #[serde(default)]
    pub compatibility: CompatibilityProfile,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerFields {
    address: SocketAddr,
    alias: String,
    #[serde(default)]
    compatibility: CompatibilityProfile,
}
impl TryFrom<PeerFields> for SavedPeer {
    type Error = Error;
    fn try_from(value: PeerFields) -> Result<Self> {
        let mut peer = Self::new(value.address, &value.alias)?;
        peer.compatibility = value.compatibility;
        Ok(peer)
    }
}
pub fn peer_address(value: &str) -> Result<SocketAddr> {
    let address = value
        .parse::<SocketAddr>()
        .or_else(|_| {
            value
                .parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_PORT.get()))
        })
        .map_err(|_| {
            Error::Invalid("enter an IP address, optionally with :PORT (IPv6 ports use [IP]:PORT)")
        })?;
    if address.port() == 0
        || address.ip().is_unspecified()
        || address.ip().is_multicast()
        || address.ip() == IpAddr::V4(std::net::Ipv4Addr::BROADCAST)
    {
        return Err(Error::Invalid(
            "peer must be a host IP with a port from 1 to 65535",
        ));
    }
    Ok(address)
}
impl SavedPeer {
    pub fn new(address: SocketAddr, alias: &str) -> Result<Self> {
        peer_address(&address.to_string())?;
        let alias = alias.trim();
        if alias.chars().any(char::is_control) || alias.chars().count() > 64 {
            return Err(Error::Invalid(
                "alias must be at most 64 characters without control characters",
            ));
        }
        Ok(Self {
            address,
            compatibility: CompatibilityProfile::Native,
            alias: if alias.is_empty() {
                address.to_string()
            } else {
                alias.to_owned()
            },
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub mining_threads: MiningThreads,
    #[serde(deserialize_with = "deserialize_difficulty")]
    pub mining_difficulty: u16,
    pub port: NonZeroU16,
    pub peers: Vec<SavedPeer>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mining_threads: MiningThreads::default(),
            mining_difficulty: MIN_MINING_DIFFICULTY,
            port: DEFAULT_PORT,
            peers: Vec::new(),
        }
    }
}
impl Settings {
    pub fn load(data_dir: &Path) -> Result<Self> {
        match fs::read(data_dir.join("settings.json")) {
            Ok(bytes) => {
                let settings: Self = serde_json::from_slice(&bytes)?;
                settings.validate_peers()?;
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        self.validate_peers()?;
        if !(MIN_MINING_DIFFICULTY..=MAX_MINING_DIFFICULTY).contains(&self.mining_difficulty) {
            return Err(Error::Invalid(
                "mining difficulty must be 23–256 leading zero bits",
            ));
        }
        let temporary = data_dir.join("settings.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, data_dir.join("settings.json"))?;
        Ok(())
    }
    fn validate_peers(&self) -> Result<()> {
        let mut addresses = std::collections::BTreeSet::new();
        if self.peers.len() > MAX_SAVED_PEERS {
            return Err(Error::Invalid("at most 256 saved peers are supported"));
        }
        for peer in &self.peers {
            SavedPeer::new(peer.address, &peer.alias)?;
            if !addresses.insert(peer.address) {
                return Err(Error::Invalid("duplicate saved peer address"));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_profiles_migrate_and_roundtrip_for_multiple_peers() {
        let old: SavedPeer =
            serde_json::from_str(r#"{"address":"192.0.2.1:7001","alias":"Native"}"#).unwrap();
        assert_eq!(old.compatibility, CompatibilityProfile::Native);
        let foreign = configured_peer(
            "http://192.0.2.2",
            &["--compat", "grupa1-luka", "Luka", "laptop"],
        )
        .unwrap();
        assert_eq!(foreign.address.port(), 8080);
        assert_eq!(foreign.alias, "Luka laptop");
        assert_eq!(
            configured_peer("[::1]:9000", &["--compat", "grupa1-luka"])
                .unwrap()
                .address
                .port(),
            9000
        );
        assert!(configured_peer("192.0.2.1", &["--compat", "unknown"]).is_err());
        let settings = Settings {
            peers: vec![old, foreign],
            ..Settings::default()
        };
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.peers, settings.peers);
    }
    #[test]
    fn difficulty_defaults_migrate_and_validate() {
        let old: Settings = serde_json::from_str(r#"{"mining_threads":"all"}"#).unwrap();
        assert_eq!(old.mining_difficulty, 23);
        for value in [23, 24, 256] {
            let settings: Settings =
                serde_json::from_str(&format!(r#"{{"mining_difficulty":{value}}}"#)).unwrap();
            assert_eq!(settings.mining_difficulty, value);
            assert_eq!(
                serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap())
                    .unwrap()
                    .mining_difficulty,
                value
            );
        }
        for value in [0, 22, 257] {
            assert!(
                serde_json::from_str::<Settings>(&format!(r#"{{"mining_difficulty":{value}}}"#))
                    .is_err()
            );
        }
    }
    #[test]
    fn port_defaults_migrate_old_settings_and_validate_boundaries() {
        assert_eq!(Settings::default().port.get(), 7001);
        let old: Settings = serde_json::from_str(r#"{"mining_threads":"all"}"#).unwrap();
        assert_eq!(old.port.get(), 7001);
        assert_eq!(old.mining_threads, MiningThreads::All);
        assert!(old.peers.is_empty());
        for port in [1, 7001, 65535] {
            let settings: Settings =
                serde_json::from_str(&format!(r#"{{"port":{port}}}"#)).unwrap();
            assert_eq!(settings.port.get(), port);
        }
        for port in ["0", "-1", "65536", "1.5", "\"7001\""] {
            assert!(serde_json::from_str::<Settings>(&format!(r#"{{"port":{port}}}"#)).is_err());
        }
    }

    #[test]
    fn saved_peer_addresses_aliases_and_configuration_validation() {
        for (input, expected) in [
            ("192.0.2.1", "192.0.2.1:7001"),
            ("192.0.2.1:7002", "192.0.2.1:7002"),
            ("::1", "[::1]:7001"),
            ("[2001:db8::1]:7002", "[2001:db8::1]:7002"),
        ] {
            assert_eq!(peer_address(input).unwrap().to_string(), expected);
        }
        for input in [
            "localhost",
            "bad",
            "0.0.0.0",
            "::",
            "255.255.255.255",
            "224.0.0.1",
            "ff02::1",
            "127.0.0.1:0",
            "127.0.0.1:65536",
        ] {
            assert!(peer_address(input).is_err(), "accepted {input}");
        }
        let peer =
            SavedPeer::new(peer_address("192.0.2.1").unwrap(), "  Belgrade workshop  ").unwrap();
        assert_eq!(peer.alias, "Belgrade workshop");
        assert!(SavedPeer::new(peer.address, "bad\x1b[31m").is_err());
        assert!(SavedPeer::new(peer.address, &"a".repeat(65)).is_err());
        assert_eq!(
            serde_json::from_str::<SavedPeer>(&serde_json::to_string(&peer).unwrap()).unwrap(),
            peer
        );
        assert!(
            serde_json::from_str::<SavedPeer>(r#"{"address":"0.0.0.0:7001","alias":"invalid"}"#)
                .is_err()
        );
        let mut settings = Settings {
            peers: vec![peer.clone(), peer],
            ..Settings::default()
        };
        assert!(settings.validate_peers().is_err());
        settings.peers.pop();
        settings.validate_peers().unwrap();
    }

    #[test]
    fn cli_port_and_discovery_options() {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        for command in ["node", "tui"] {
            let Some(Command::Node(config) | Command::Tui(config)) = Cli::try_parse_from([
                "pixel",
                command,
                "--port",
                "7002",
                "--discover",
                "--discover-port",
                "7100",
            ])
            .unwrap()
            .command
            else {
                panic!("configuration");
            };
            assert_eq!(config.port.unwrap().get(), 7002);
            assert!(config.discover);
            assert_eq!(config.discover_port.get(), 7100);
            for port in ["0", "65536", "-1"] {
                assert!(Cli::try_parse_from(["pixel", command, "--port", port]).is_err());
            }
            assert!(
                Cli::try_parse_from([
                    "pixel",
                    command,
                    "--port",
                    "7002",
                    "--listen",
                    "127.0.0.1:7003"
                ])
                .is_err()
            );
        }
    }
    #[test]
    fn counts_all_and_invalid_preferences() {
        assert_eq!(
            "ALL".parse::<MiningThreads>().unwrap().resolved(),
            available_threads()
        );
        assert_eq!("3".parse::<MiningThreads>().unwrap().resolved(), 3);
        for bad in ["0", "-1", "1.5", "alll", "1025", "18446744073709551616"] {
            assert!(bad.parse::<MiningThreads>().is_err());
        }
        assert!(serde_json::from_str::<Settings>(r#"{"mining_threads":"0"}"#).is_err());
    }
    #[test]
    fn saved_mode_roundtrips_without_changing_all_into_a_fixed_count() {
        let directory = std::env::temp_dir().join(format!(
            "pixel-settings-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&directory).unwrap();
        assert_eq!(
            Settings::load(&directory).unwrap().mining_threads,
            MiningThreads::default()
        );
        Settings {
            mining_threads: MiningThreads::All,
            ..Settings::default()
        }
        .save(&directory)
        .unwrap();
        assert_eq!(
            Settings::load(&directory).unwrap().mining_threads,
            MiningThreads::All
        );
        Settings {
            mining_threads: "3".parse().unwrap(),
            ..Settings::default()
        }
        .save(&directory)
        .unwrap();
        assert_eq!(
            Settings::load(&directory)
                .unwrap()
                .mining_threads
                .resolved(),
            3
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cli_accepts_all_and_exact_counts_and_rejects_zero_before_startup() {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        for command in ["node", "tui"] {
            let Some(Command::Node(config) | Command::Tui(config)) =
                Cli::try_parse_from(["pixel", command, "--threads", "all"])
                    .unwrap()
                    .command
            else {
                panic!("expected node configuration");
            };
            assert_eq!(config.threads, Some(MiningThreads::All));
            assert!(Cli::try_parse_from(["pixel", command, "--threads", "0"]).is_err());
        }
    }
}
