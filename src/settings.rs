//! Local runtime preferences, separate from protocol state and exported chain data.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, fs, num::NonZeroUsize, path::Path, str::FromStr};

pub const MAX_MINING_THREADS: usize = 1024;
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
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub mining_threads: MiningThreads,
}
impl Settings {
    pub fn load(data_dir: &Path) -> Result<Self> {
        match fs::read(data_dir.join("settings.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let temporary = data_dir.join("settings.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, data_dir.join("settings.json"))?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        }
        .save(&directory)
        .unwrap();
        assert_eq!(
            Settings::load(&directory).unwrap().mining_threads,
            MiningThreads::All
        );
        Settings {
            mining_threads: "3".parse().unwrap(),
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
