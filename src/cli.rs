use crate::{
    Result,
    config::Config,
    crypto,
    protocol::{Action, Bytes, Transaction},
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(version, about = "Pixel Chain workshop prototype")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    /// Open the live terminal dashboard (also the default without a subcommand).
    Tui(Config),
    /// Start a node. Interactive commands are read from stdin.
    Node(Config),
    /// Create or reload a persistent Ed25519 identity; print only its public key.
    Key {
        #[arg(long, default_value = "data/node/identity.seed")]
        file: PathBuf,
    },
    /// Measure actual header hashing at the fixed 23-bit target.
    Benchmark {
        #[arg(long, default_value_t = 2_000_000, value_parser = clap::value_parser!(u64).range(1..))]
        hashes: u64,
    },
    /// Mine and fully validate sequential new blocks at the fixed 23-bit target.
    BenchmarkMining {
        /// Number of new blocks; 30 or more recommended for timing percentiles.
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..))]
        samples: u32,
        /// Emit the complete measurements as JSON.
        #[arg(long)]
        json: bool,
    },
}
pub fn transaction(line: &str, counter: u64) -> Result<Transaction> {
    let args: Vec<_> = line.split_whitespace().collect();
    let parcel = args
        .get(1)
        .ok_or(crate::Error::Invalid("missing parcel"))?
        .parse()
        .map_err(|_| crate::Error::Invalid("invalid parcel"))?;
    let payload = args
        .get(2)
        .ok_or(crate::Error::Invalid("missing color/recipient"))?;
    let action = match args.first().copied() {
        Some("paint") => Action::Paint {
            color: crate::colors::parse(payload)?,
        },
        Some("transfer") => Action::Transfer {
            recipient: payload.parse()?,
        },
        _ => return Err(crate::Error::Invalid("unknown transaction command")),
    };
    if args.len() != 3 {
        return Err(crate::Error::Invalid("expected command parcel payload"));
    }
    Ok(Transaction {
        parcel,
        counter,
        sender: Bytes::default(),
        action,
        signature: Bytes::default(),
    })
}
pub fn key(file: PathBuf) -> Result<()> {
    println!(
        "{}",
        crypto::public_key(&crypto::load_or_create_key(&file)?)
    );
    Ok(())
}
pub const HELP: &str = "status | identity | owned | paint PARCEL COLOR_NAME|RRGGBB | transfer PARCEL PUBLIC_KEY | mining on|off | threads COUNT|all | port PORT | peer add IP[:PORT] [ALIAS] | peer edit OLD_IP[:PORT] IP[:PORT] [ALIAS] | peer remove IP[:PORT] | network on|off | export [PATH.ppm] | quit";
