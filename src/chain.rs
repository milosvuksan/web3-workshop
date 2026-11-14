use crate::{
    Error, Result,
    protocol::{Block, Hash, params::genesis, validation::transition},
    state::State,
};
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Entry {
    pub block: Block,
    pub state: State,
}
pub struct Chain {
    pub entries: BTreeMap<Hash, Entry>,
    pub tip: Hash,
}
impl Default for Chain {
    fn default() -> Self {
        let block = genesis();
        let tip = block.hash();
        let mut entries = BTreeMap::new();
        entries.insert(tip, Entry { block, state: State::default() });
        Self { entries, tip }
    }
}
impl Chain {
    pub fn head(&self) -> &Entry {
        &self.entries[&self.tip]
    }
    pub fn insert(&mut self, block: Block, now: u64) -> Result<bool> {
        if block.transactions.len() > crate::protocol::params::MAX_TRANSACTIONS {
            return Err(Error::Invalid("too many transactions"));
        }
        let hash = block.hash();
        if self.entries.contains_key(&hash) {
            return Ok(false);
        }
        let parent = self.entries.get(&block.previous).ok_or(Error::UnknownParent(block.previous))?;
        let state = transition(&block, &parent.block, &parent.state, now)?;
        // At fixed difficulty accumulated work is height * 2^23 (u128).
        let wins = block.height > self.head().block.height || (block.height == self.head().block.height && hash < self.tip);
        self.entries.insert(hash, Entry { block, state });
        if wins {
            self.tip = hash;
        }
        Ok(wins)
    }
    pub fn path(&self, mut tip: Hash) -> Vec<Hash> {
        let mut path = Vec::new();
        while let Some(entry) = self.entries.get(&tip) {
            path.push(tip);
            if entry.block.height == 0 {
                break;
            }
            tip = entry.block.previous;
        }
        path.reverse();
        path
    }
    pub fn detached(&self, mut old_tip: Hash) -> Vec<crate::protocol::Transaction> {
        if !self.entries.contains_key(&old_tip) {
            return Vec::new();
        }
        let mut new_tip = self.tip;
        let mut detached = Vec::new();
        // Walk only the divergent suffixes. Scanning both full paths and testing
        // membership in a Vec made even a one-block extension quadratic in height.
        while old_tip != new_tip {
            let old = &self.entries[&old_tip].block;
            let new = &self.entries[&new_tip].block;
            if old.height >= new.height {
                detached.push(old_tip);
                old_tip = old.previous;
            } else {
                new_tip = new.previous;
            }
        }
        detached.reverse();
        detached.into_iter().flat_map(|hash| self.entries[&hash].block.transactions.clone()).collect()
    }
}
