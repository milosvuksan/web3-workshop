use sha2::{Digest, Sha256};
use std::{env, hint::black_box, time::Instant};

fn main() {
    let args: Vec<String> = env::args().collect();
    let n: u64 = args[1].parse().unwrap();
    let size: usize = args[2].parse().unwrap();
    assert!(size >= 8);
    let mut buf: Vec<u8> = (0..size).map(|j| j as u8).collect();
    let mut digest = [0u8; 32];
    let start = Instant::now();
    for i in 0..n {
        buf[size - 8..].copy_from_slice(&i.to_le_bytes());
        digest.copy_from_slice(&Sha256::digest(black_box(buf.as_slice())));
        black_box(&digest);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    println!("{{\"seconds\":{elapsed},\"hashes_per_second\":{},\"digest\":\"{hex}\"}}", n as f64 / elapsed);
}
