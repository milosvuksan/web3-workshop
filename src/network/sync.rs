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
    /// Ancestry needed by retained blocks takes priority over speculative tips.
    pub fn want_parent(&mut self, hash: Hash) {
        if self.wanted.contains(&hash) {
            return;
        }
        if self.wanted.len() >= MAX_WAITING {
            let parents: BTreeSet<_> = self.waiting.values().map(|b| b.previous).collect();
            let expendable = self.wanted.iter().find(|h| !parents.contains(h)).copied();
            if let Some(candidate) = expendable {
                self.wanted.remove(&candidate);
            }
        }
        self.want(hash);
    }
    pub fn defer(&mut self, block: Block) {
        let hash = block.hash();
        if self.waiting.contains_key(&hash) {
            return;
        }
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
