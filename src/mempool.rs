use crate::{
    Error, Result,
    protocol::{Transaction, params::MAX_TRANSACTIONS},
    state::State,
};
use std::collections::BTreeSet;
pub const MAX_PENDING: usize = 4096;
#[derive(Default)]
pub struct Mempool {
    pub transactions: Vec<Transaction>,
}
impl Mempool {
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
        self.transactions
            .iter()
            .filter(|tx| state.apply(tx).is_ok())
            .take(MAX_TRANSACTIONS)
            .cloned()
            .collect()
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
