use super::{Block, Hash, params::*};
use crate::{Error, Result, crypto, state::State};
pub fn valid_pow(hash: Hash) -> bool {
    hash.0[0] == 0 && hash.0[1] == 0 && hash.0[2] < 2
}
pub fn transition(block: &Block, parent: &Block, state: &State, now: u64) -> Result<State> {
    if block.transactions.len() > MAX_TRANSACTIONS {
        return Err(Error::Invalid("too many transactions"));
    }
    if block.previous != parent.hash() {
        return Err(Error::Invalid("wrong parent hash"));
    }
    if Some(block.height) != parent.height.checked_add(1) {
        return Err(Error::Invalid("wrong block height"));
    }
    crypto::valid_key(block.miner)?;
    if block.timestamp < parent.timestamp {
        return Err(Error::Invalid("timestamp precedes parent"));
    }
    if block.timestamp > now.saturating_add(MAX_FUTURE_SECONDS) {
        return Err(Error::Future);
    }
    let hash = block.hash();
    if !valid_pow(hash) {
        return Err(Error::Invalid("insufficient proof of work"));
    }
    let mut next = state.clone();
    for tx in &block.transactions {
        next.apply(tx)?;
    }
    next.reward(hash, block.timestamp, block.miner);
    Ok(next)
}
