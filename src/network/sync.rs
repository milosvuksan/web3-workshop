use crate::protocol::{Block, Hash};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Bound::{Excluded, Unbounded},
};
pub const MAX_WAITING: usize = 1024;
#[derive(Default)]
pub struct Sync {
    pub waiting: BTreeMap<Hash, Block>,
    pub wanted: BTreeSet<Hash>,
    wanted_order: VecDeque<Hash>,
    retry_after: Option<Hash>,
}
impl Sync {
    pub fn want(&mut self, hash: Hash) {
        if self.waiting.contains_key(&hash) || self.wanted.contains(&hash) {
            return;
        }
        if self.wanted.len() >= MAX_WAITING
            && let Some(oldest) = self.wanted_order.pop_front()
        {
            self.wanted.remove(&oldest);
        }
        self.wanted.insert(hash);
        self.wanted_order.push_back(hash);
    }
    pub fn want_parent(&mut self, hash: Hash) {
        self.want(hash);
    }
    pub fn forget(&mut self, hash: &Hash) {
        self.wanted.remove(hash);
        self.wanted_order.retain(|h| h != hash);
    }
    /// Rotate by hash rather than index so removals cannot skip later requests.
    pub fn retry_requests(&mut self) -> Vec<Hash> {
        let requests: Vec<_> = self
            .wanted
            .range((self.retry_after.map_or(Unbounded, Excluded), Unbounded))
            .chain(self.wanted.iter())
            .take(self.wanted.len().min(4))
            .copied()
            .collect();
        self.retry_after = requests.last().copied();
        requests
    }
    pub fn defer(&mut self, block: Block) {
        let hash = block.hash();
        if self.waiting.contains_key(&hash) {
            self.forget(&hash);
            return;
        }
        if self.waiting.len() >= MAX_WAITING {
            // Prefer older ancestry so a history longer than the orphan limit
            // can still reach a known parent. Descendants are fetched again.
            let highest = self.waiting.iter().max_by_key(|(_, b)| b.height).map(|(hash, b)| (*hash, b.height));
            if let Some((hash, height)) = highest {
                if block.height > height {
                    return;
                }
                self.waiting.remove(&hash);
            }
        }
        self.forget(&hash);
        self.waiting.insert(hash, block);
    }
}
