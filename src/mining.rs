use crate::protocol::{Block, Bytes, validation::valid_pow};
use sha2::{Digest, Sha256};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

/// Single-worker entry point retained for reproducible benchmarks.
pub fn mine(block: Block, cancel: &AtomicBool) -> Option<Block> {
    let start = block.nonce;
    mine_lane(block, cancel, start, 1)
}
fn mine_lane(mut block: Block, cancel: &AtomicBool, start: u64, stride: u64) -> Option<Block> {
    let mut bytes = block.canonical_bytes();
    bytes.truncate(bytes.len() - 8);
    let prefix = Sha256::new().chain_update(bytes);
    block.nonce = start;
    let mut iterations = 0_u32;
    loop {
        // Count attempts rather than nonce bits: every stride must observe stop.
        if iterations & 0x3fff == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        let hash = Bytes(
            prefix
                .clone()
                .chain_update(block.nonce.to_be_bytes())
                .finalize()
                .into(),
        );
        if valid_pow(hash) {
            return Some(block);
        }
        block.nonce = block.nonce.checked_add(stride)?;
        iterations = iterations.wrapping_add(1);
    }
}

/// Workers search disjoint nonce lanes. The first result stops the whole group.
/// Dropping a job also stops and joins its workers, including partial startup.
pub fn start(
    block: Block,
    threads: usize,
    found: impl Fn(Block) + Send + Sync + 'static,
) -> io::Result<Job> {
    if threads == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mining requires at least one thread",
        ));
    }
    let stride = u64::try_from(threads)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many mining threads"))?;
    let mut job = Job {
        cancel: Arc::new(AtomicBool::new(false)),
        handles: Vec::new(),
    };
    let found = Arc::new(found);
    for lane in 0..threads {
        let cancel = job.cancel.clone();
        let found = found.clone();
        let block = block.clone();
        let handle = std::thread::Builder::new()
            .name(format!("pixel-miner-{lane}"))
            .spawn(move || {
                let Some(start) = block.nonce.checked_add(lane as u64) else {
                    return;
                };
                if let Some(block) = mine_lane(block, &cancel, start, stride)
                    && cancel
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    found(block);
                }
            })?;
        job.handles.push(handle);
    }
    Ok(job)
}
pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub handles: Vec<JoinHandle<()>>,
}
impl Job {
    pub fn is_finished(&self) -> bool {
        self.handles.iter().all(JoinHandle::is_finished)
    }
    pub fn worker_count(&self) -> usize {
        self.handles.len()
    }
    fn join(&mut self) {
        self.cancel.store(true, Ordering::Release);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
    pub fn stop(mut self) {
        self.join();
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.join();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_lane_observes_cancellation_even_at_unaligned_nonces() {
        let cancelled = AtomicBool::new(true);
        for stride in [1, 2, 3, 8, 17] {
            for lane in 0..stride {
                assert!(
                    mine_lane(
                        crate::protocol::params::genesis(),
                        &cancelled,
                        lane + 1,
                        stride
                    )
                    .is_none()
                );
            }
        }
    }
    #[test]
    fn multiple_workers_deliver_one_valid_result_and_join() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/prototype-v1.json")).unwrap();
        let mut block: Block = serde_json::from_value(vectors["one"]["block"].clone()).unwrap();
        // The second lane starts exactly at a known, full-difficulty solution.
        block.nonce -= 1;
        let (sender, receiver) = std::sync::mpsc::channel();
        let job = start(block, 2, move |block| {
            sender.send(block).unwrap();
        })
        .unwrap();
        assert_eq!(job.worker_count(), 2);
        let result = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(valid_pow(result.hash()));
        job.stop();
        assert!(
            receiver.try_recv().is_err(),
            "only the winner may deliver a result"
        );
    }
}
