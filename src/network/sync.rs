use crate::protocol::{Block, Hash};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_WAITING: usize = 1024;
#[derive(Default)]
pub struct Sync {
    pub waiting: BTreeMap<Hash, Block>,
    pub wanted: BTreeSet<Hash>,
}
impl Sync {
    pub fn want(&mut self, hash: Hash) {
        if self.wanted.len() < MAX_WAITING {
            self.wanted.insert(hash);
        }
    }
    pub fn defer(&mut self, block: Block) {
        if self.waiting.len() >= MAX_WAITING {
            // Prefer older ancestry so a history longer than the orphan limit
            // can still reach a known parent. Descendants are fetched again.
            let highest = self
                .waiting
                .iter()
                .max_by_key(|(_, b)| b.height)
                .map(|(hash, b)| (*hash, b.height));
            if let Some((hash, height)) = highest {
                if block.height > height {
                    return;
                }
                self.waiting.remove(&hash);
            }
        }
        self.waiting.insert(block.hash(), block);
    }
}
