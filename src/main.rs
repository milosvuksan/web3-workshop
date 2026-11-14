use baneworkshop::{
    cli::{Cli, Command},
    protocol::{Bytes, params::genesis},
};
use clap::Parser;
use sha2::{Digest, Sha256};
mod benchmark;
fn run_node(config: baneworkshop::config::Config, tui: bool) -> baneworkshop::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(baneworkshop::node::run_with_ui(config, tui));
    // Node shutdown already stops and joins its miner and writes diagnostics.
    // Tokio stdin uses a blocking OS read that cannot be cancelled while stdin
    // remains open. Bound runtime teardown so Ctrl-C can finish in that case.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result
}

fn main() {
    let result = match Cli::parse()
        .command
        .unwrap_or_else(|| Command::Tui(Default::default()))
    {
        Command::Tui(config) => run_node(config, true),
        Command::Node(config) => run_node(config, false),
        Command::Key { file } => baneworkshop::cli::key(file),
        Command::BenchmarkMining { samples, json } => benchmark::run(samples, json),
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
