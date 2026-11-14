use baneworkshop::{
    cli::{Cli, Command},
    protocol::{Bytes, params::genesis},
};
use clap::Parser;
use sha2::{Digest, Sha256};
#[tokio::main]
async fn main() {
    let result = match Cli::parse().command {
        Command::Node(config) => baneworkshop::node::run(config).await,
        Command::Key { file } => baneworkshop::cli::key(file),
        Command::Benchmark { hashes } => {
            let mut bytes = genesis().canonical_bytes();
            bytes.truncate(bytes.len() - 8);
            let prefix = Sha256::new().chain_update(bytes);
            let start = std::time::Instant::now();
            for nonce in 0..hashes {
                let hash: Bytes<32> = Bytes(
                    prefix
                        .clone()
                        .chain_update(nonce.to_be_bytes())
                        .finalize()
                        .into(),
                );
                std::hint::black_box(hash);
            }
            let rate = hashes as f64 / start.elapsed().as_secs_f64();
            println!(
                "{rate:.0} hashes/s; expected group block time: 8388608 / combined_hashes_per_second seconds; solo {:.3}s",
                8388608.0 / rate
            );
            Ok(())
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
