use crate::{
    Error, Result,
    protocol::{Action, Color, Hash, PublicKey, Transaction, params::MAX_TRANSACTIONS},
    state::State,
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingPaint {
    pub color: Color,
    pub transaction: Hash,
}
pub const MAX_PENDING: usize = 4096;
#[derive(Default)]
pub struct Mempool {
    pub transactions: Vec<Transaction>,
}
impl Mempool {
    /// The latest admitted local edit per parcel; does not alter confirmed state.
    pub fn pending_paints(&self, identity: PublicKey) -> BTreeMap<usize, PendingPaint> {
        self.transactions
            .iter()
            .filter_map(|tx| match tx.action {
                Action::Paint { color } if tx.sender == identity => {
                    Some((usize::from(tx.parcel), PendingPaint { color, transaction: tx.id() }))
                }
                _ => None,
            })
            .collect()
    }
    pub fn projected(&self, state: &State) -> State {
        let mut projected = state.clone();
        for tx in &self.transactions {
            let _ = projected.apply(tx);
        }
        projected
    }
    pub fn admit(&mut self, tx: Transaction, state: &State) -> Result<bool> {
        if self.transactions.iter().any(|t| t.id() == tx.id()) {
            return Ok(false);
        }
        if self.transactions.len() >= MAX_PENDING {
            return Err(Error::Invalid("mempool is full"));
        }
        self.projected(state).apply(&tx)?;
        self.transactions.push(tx);
        Ok(true)
    }
    pub fn select(&self, state: &State) -> Vec<Transaction> {
        let mut state = state.clone();
        self.transactions.iter().filter(|tx| state.apply(tx).is_ok()).take(MAX_TRANSACTIONS).cloned().collect()
    }
    pub fn reconcile(&mut self, state: &State, detached: Vec<Transaction>) {
        let mut candidates = std::mem::take(&mut self.transactions);
        candidates.extend(detached);
        let mut seen = BTreeSet::new();
        candidates.retain(|tx| seen.insert(tx.id()));
        let mut projected = state.clone();
        // Multiple passes restore dependency order after a reorganization.
        loop {
            let before = self.transactions.len();
            candidates.retain(|tx| {
                if self.transactions.len() < MAX_PENDING && projected.apply(tx).is_ok() {
                    self.transactions.push(tx.clone());
                    false
                } else {
                    true
                }
            });
            if self.transactions.len() == before {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chain::Chain, cli, crypto, protocol::Block};
    #[test]
    fn repeated_paint_previews_follow_latest_edit_and_clear_on_confirmation() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/prototype-v1.json")).unwrap();
        let mut chain = Chain::default();
        for name in ["one", "two", "three"] {
            let block: Block = serde_json::from_value(vectors[name]["block"].clone()).unwrap();
            chain.insert(block, 1_700_000_100).unwrap();
        }
        let key = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
        let identity = crypto::public_key(&key);
        let state = &chain.head().state;
        let mut pool = Mempool::default();
        for (counter, color) in [(3, "red"), (4, "#123ABC")] {
            let mut tx = cli::transaction(&format!("paint 181 {color}"), counter).unwrap();
            crypto::sign(&mut tx, &key);
            pool.admit(tx, state).unwrap();
        }
        assert_eq!(pool.pending_paints(identity)[&181].color, crate::colors::parse("123abc").unwrap());
        assert!(pool.pending_paints(crate::protocol::Bytes([9; 32])).is_empty());
        assert_eq!(state.parcels[181].color, crate::colors::parse("blue").unwrap());
        let confirmed = pool.projected(state);
        pool.reconcile(&confirmed, Vec::new());
        assert!(pool.pending_paints(identity).is_empty());
        assert_eq!(confirmed.parcels[181].color, crate::colors::parse("123abc").unwrap());
    }
}
