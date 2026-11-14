use crate::protocol::{Block, Bytes, validation::valid_pow};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
/// Dedicated worker; callers retain the cancellation token and join the worker.
pub fn mine(mut block: Block, cancel: &AtomicBool) -> Option<Block> {
    let mut bytes = block.canonical_bytes();
    bytes.truncate(bytes.len() - 8);
    let prefix = Sha256::new().chain_update(bytes);
    loop {
        if block.nonce & 0x3fff == 0 && cancel.load(Ordering::Relaxed) {
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
        block.nonce = block.nonce.checked_add(1)?;
    }
}
pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub handle: std::thread::JoinHandle<()>,
}
impl Job {
    pub fn stop(self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}
