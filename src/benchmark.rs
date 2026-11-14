//! Reproducible empty-block workload using the production miner and validator.
use baneworkshop::{
    Error, Result,
    chain::Chain,
    mining,
    protocol::{Block, Hash, PublicKey, params::DIFFICULTY},
};
use serde::Serialize;
use std::{sync::atomic::AtomicBool, time::Instant};

// RFC 8032 public test identity. No local identity file or private key is used.
const MINER: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const FIRST_TIMESTAMP: u64 = 1_700_000_000;

#[derive(Debug, Serialize)]
struct Statistics {
    mean: f64,
    median: f64,
    /// Nearest-rank percentile (ceil(0.95 * sample_count)).
    p95: f64,
    min: f64,
    max: f64,
}

fn statistics(values: impl Iterator<Item = f64>) -> Statistics {
    let mut values: Vec<_> = values.collect();
    assert!(!values.is_empty());
    values.sort_by(f64::total_cmp);
    let count = values.len();
    let median = if count.is_multiple_of(2) {
        (values[count / 2 - 1] + values[count / 2]) / 2.0
    } else {
        values[count / 2]
    };
    Statistics {
        mean: values.iter().sum::<f64>() / count as f64,
        median,
        p95: values[(95 * count).div_ceil(100) - 1],
        min: values[0],
        max: values[count - 1],
    }
}

#[derive(Serialize)]
struct Sample {
    height: u64,
    hash: Hash,
    nonce: u64,
    hash_attempts: u64,
    mining_seconds: f64,
    /// Candidate preparation, mining, and full chain insertion/validation.
    block_seconds: f64,
}

#[derive(Serialize)]
struct Report {
    workload: &'static str,
    package_version: &'static str,
    optimized_build: bool,
    difficulty_bits: u32,
    worker_threads: u32,
    miner_public_key: PublicKey,
    first_timestamp: u64,
    initial_nonce: u64,
    transactions_per_block: u32,
    sample_count: u32,
    percentile_method: &'static str,
    total_hash_attempts: u128,
    total_mining_seconds: f64,
    total_block_seconds: f64,
    hashes_per_second: f64,
    expected_hash_attempts_per_block: u64,
    expected_solo_block_seconds: f64,
    mining_seconds: Statistics,
    block_seconds: Statistics,
    samples: Vec<Sample>,
}

fn candidate(chain: &Chain, miner: PublicKey) -> Result<Block> {
    let height = chain
        .head()
        .block
        .height
        .checked_add(1)
        .ok_or(Error::Invalid("benchmark height overflow"))?;
    Ok(Block {
        previous: chain.tip,
        height,
        miner,
        timestamp: FIRST_TIMESTAMP
            .checked_add(height - 1)
            .ok_or(Error::Invalid("benchmark timestamp overflow"))?,
        nonce: 0,
        transactions: vec![],
    })
}

fn measure(sample_count: u32) -> Result<Report> {
    if sample_count == 0 {
        return Err(Error::Invalid("benchmark samples must be positive"));
    }
    let miner = MINER.parse()?;
    let mut chain = Chain::default();
    let cancel = AtomicBool::new(false);
    let mut samples = Vec::new();
    for _ in 0..sample_count {
        let block_start = Instant::now();
        let block = candidate(&chain, miner)?;
        let mining_start = Instant::now();
        let block = mining::mine(block, &cancel)
            .ok_or(Error::Invalid("benchmark exhausted nonce space"))?;
        let mining_seconds = mining_start.elapsed().as_secs_f64();
        // Nonce starts at zero and the successful attempt is included.
        let hash_attempts = block
            .nonce
            .checked_add(1)
            .ok_or(Error::Invalid("benchmark hash attempt overflow"))?;
        let mut sample = Sample {
            height: block.height,
            hash: block.hash(),
            nonce: block.nonce,
            hash_attempts,
            mining_seconds,
            block_seconds: 0.0,
        };
        let now = block.timestamp;
        if !chain.insert(block, now)? || chain.tip != sample.hash {
            return Err(Error::Invalid("benchmark block did not advance chain"));
        }
        sample.block_seconds = block_start.elapsed().as_secs_f64();
        samples.push(sample);
    }
    let total_hash_attempts = samples.iter().map(|s| u128::from(s.hash_attempts)).sum();
    let total_mining_seconds = samples.iter().map(|s| s.mining_seconds).sum();
    let total_block_seconds = samples.iter().map(|s| s.block_seconds).sum();
    let hashes_per_second = total_hash_attempts as f64 / total_mining_seconds;
    let expected_hash_attempts_per_block = 1_u64 << DIFFICULTY;
    Ok(Report {
        workload: "sequential-empty-blocks-v1",
        package_version: env!("CARGO_PKG_VERSION"),
        optimized_build: !cfg!(debug_assertions),
        difficulty_bits: DIFFICULTY,
        worker_threads: 1,
        miner_public_key: miner,
        first_timestamp: FIRST_TIMESTAMP,
        initial_nonce: 0,
        transactions_per_block: 0,
        sample_count,
        percentile_method: "nearest-rank",
        total_hash_attempts,
        total_mining_seconds,
        total_block_seconds,
        hashes_per_second,
        expected_hash_attempts_per_block,
        expected_solo_block_seconds: expected_hash_attempts_per_block as f64 / hashes_per_second,
        mining_seconds: statistics(samples.iter().map(|s| s.mining_seconds)),
        block_seconds: statistics(samples.iter().map(|s| s.block_seconds)),
        samples,
    })
}

pub fn run(samples: u32, json: bool) -> Result<()> {
    let report = measure(samples)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} sequential new blocks mined and fully validated; {}-bit target; one worker",
            report.sample_count, report.difficulty_bits
        );
        let s = &report.block_seconds;
        println!(
            "Block seconds: mean {:.6}; median {:.6}; p95 {:.6}; min {:.6}; max {:.6}",
            s.mean, s.median, s.p95, s.min, s.max
        );
        let s = &report.mining_seconds;
        println!(
            "Mining seconds: mean {:.6}; median {:.6}; p95 {:.6}; min {:.6}; max {:.6}",
            s.mean, s.median, s.p95, s.min, s.max
        );
        println!(
            "{} hash attempts in {:.6}s mining; {:.0} hashes/s; expected solo block {:.6}s",
            report.total_hash_attempts,
            report.total_mining_seconds,
            report.hashes_per_second,
            report.expected_solo_block_seconds
        );
        println!(
            "Expected group block seconds = {} / combined_hashes_per_second",
            report.expected_hash_attempts_per_block
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_use_arithmetic_median_and_nearest_rank_p95() {
        let s = statistics((1..=20).rev().map(f64::from));
        assert_eq!(s.mean, 10.5);
        assert_eq!(s.median, 10.5);
        assert_eq!(s.p95, 19.0);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 20.0);
        let s = statistics(std::iter::once(3.0));
        assert_eq!(s.median, 3.0);
        assert_eq!(s.p95, 3.0);
    }

    #[test]
    fn deterministic_workload_uses_public_identity_and_real_target() {
        let chain = Chain::default();
        let block = candidate(&chain, MINER.parse().unwrap()).unwrap();
        assert_eq!(block, candidate(&chain, MINER.parse().unwrap()).unwrap());
        assert_eq!(block.height, 1);
        assert_eq!(block.previous, chain.tip);
        assert_eq!(block.timestamp, FIRST_TIMESTAMP);
        assert_eq!(block.nonce, 0);
        assert!(block.transactions.is_empty());
        baneworkshop::crypto::valid_key(block.miner).unwrap();
        assert_eq!(DIFFICULTY, 23);
    }

    #[test]
    fn zero_samples_rejected_before_mining() {
        assert!(measure(0).is_err());
    }

    #[test]
    fn cli_rejects_zero_work_and_preserves_existing_benchmark() {
        use baneworkshop::cli::{Cli, Command};
        use clap::Parser;
        assert!(Cli::try_parse_from(["pixel", "benchmark", "--hashes", "0"]).is_err());
        assert!(Cli::try_parse_from(["pixel", "benchmark-mining", "--samples", "0"]).is_err());
        assert!(matches!(
            Cli::try_parse_from(["pixel", "benchmark"]).unwrap().command,
            Some(Command::Benchmark { hashes: 2_000_000 })
        ));
        assert!(matches!(
            Cli::try_parse_from(["pixel", "benchmark-mining", "--json"])
                .unwrap()
                .command,
            Some(Command::BenchmarkMining {
                samples: 30,
                json: true
            })
        ));
    }

    #[test]
    fn node_defaults_match_documented_simulation_configuration() {
        use baneworkshop::cli::{Cli, Command};
        use clap::Parser;
        let Some(Command::Node(config)) = Cli::try_parse_from(["pixel", "node"]).unwrap().command
        else {
            panic!("expected node command");
        };
        assert_eq!(config.listen.to_string(), "127.0.0.1:7001");
        assert!(config.peers.is_empty());
        assert_eq!(config.data_dir.to_str(), Some("data/node"));
        assert!(config.key.is_none());
        assert!(!config.no_lag);
        assert_eq!(config.lag_seed, 1);
        assert!(!config.mine);
        let Some(Command::Node(config)) = Cli::try_parse_from([
            "pixel",
            "node",
            "--lag-seed",
            "42",
            "--peers",
            "127.0.0.1:7002,127.0.0.1:7003",
        ])
        .unwrap()
        .command
        else {
            panic!("expected node command");
        };
        assert_eq!(config.lag_seed, 42);
        assert_eq!(config.peers.len(), 2);
    }
}
