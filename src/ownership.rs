//! Local ownership history derived from confirmed canonical blocks, never consensus state.
use crate::{
    chain::{Chain, Entry},
    protocol::{
        Action, Hash, PublicKey,
        params::{WIDTH, genesis},
    },
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Source {
    Mining,
    Transfer { sender: PublicKey, transaction: Hash },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Acquisition {
    pub height: u64,
    pub timestamp: u64,
    pub block: Hash,
    #[serde(flatten)]
    pub source: Source,
}
impl Acquisition {
    pub fn method(&self) -> &'static str {
        match self.source {
            Source::Mining => "mining",
            Source::Transfer { .. } => "transfer",
        }
    }
}
pub fn timestamp_utc(timestamp: u64) -> String {
    i64::try_from(timestamp).ok().and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok()).map_or_else(
        || format!("{timestamp} (Unix seconds)"),
        |date| format!("{} {:02}:{:02}:{:02} UTC", date.date(), date.hour(), date.minute(), date.second()),
    )
}
pub struct Ownership {
    tip: Hash,
    pub parcels: BTreeMap<usize, Acquisition>,
}
impl Default for Ownership {
    fn default() -> Self {
        Self { tip: genesis().hash(), parcels: BTreeMap::new() }
    }
}
impl Ownership {
    pub fn update(&mut self, chain: &Chain, identity: PublicKey) {
        if self.tip == chain.tip {
            return;
        }
        if chain.head().block.previous == self.tip {
            self.apply(chain.tip, chain.head(), &chain.entries[&self.tip], identity);
        } else {
            // A reorg or initial catch-up rebuilds receipts from the winning path.
            self.parcels.clear();
            for hash in chain.path(chain.tip).into_iter().skip(1) {
                let entry = &chain.entries[&hash];
                self.apply(hash, entry, &chain.entries[&entry.block.previous], identity);
            }
        }
        self.tip = chain.tip;
    }
    fn apply(&mut self, hash: Hash, entry: &Entry, parent: &Entry, identity: PublicKey) {
        let block = &entry.block;
        let receipt = |source| Acquisition { height: block.height, timestamp: block.timestamp, block: hash, source };
        // Preserve receipt dates for painting and self-transfers. A transfer back
        // is a new acquisition, including multiple transfers in the same block.
        for tx in &block.transactions {
            if let Action::Transfer { recipient } = tx.action {
                if tx.sender == identity && recipient != identity {
                    self.parcels.remove(&usize::from(tx.parcel));
                }
                if recipient == identity && tx.sender != identity {
                    self.parcels.insert(usize::from(tx.parcel), receipt(Source::Transfer { sender: tx.sender, transaction: tx.id() }));
                }
            }
        }
        if block.miner == identity {
            // Transfers do not unclaim parcels. The sole None -> Some transition
            // is the reward, which is applied after all block transactions.
            if let Some(id) = parent
                .state
                .parcels
                .iter()
                .zip(&entry.state.parcels)
                .position(|(old, new)| old.owner.is_none() && new.owner == Some(identity))
            {
                self.parcels.insert(id, receipt(Source::Mining));
            }
        }
    }
    pub fn json(&self, state: &crate::state::State) -> serde_json::Value {
        serde_json::Value::Array(
            self.parcels
                .iter()
                .map(|(id, acquisition)| {
                    serde_json::json!({
                        "parcel":id, "x":id % WIDTH, "y":id / WIDTH, "color":state.parcels[*id].color,
                        "received_at_utc":timestamp_utc(acquisition.timestamp), "acquisition":acquisition,
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Block;
    fn fixture(name: &str) -> Block {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/prototype-v1.json")).unwrap();
        serde_json::from_value(vectors[name]["block"].clone()).unwrap()
    }
    #[test]
    fn mining_transfer_paint_and_reorg_receipts_follow_canonical_chain() {
        let mut chain = Chain::default();
        let a = fixture("one").miner;
        let b = fixture("fork2").miner;
        let mut owned_a = Ownership::default();
        let mut owned_b = Ownership::default();
        for name in ["one", "two", "three", "fork2", "fork3", "fork4"] {
            chain.insert(fixture(name), 1_700_000_100).unwrap();
            owned_a.update(&chain, a);
            owned_b.update(&chain, b);
            for (identity, owned) in [(a, &owned_a), (b, &owned_b)] {
                let expected: Vec<_> =
                    chain.head().state.parcels.iter().enumerate().filter_map(|(id, p)| (p.owner == Some(identity)).then_some(id)).collect();
                assert_eq!(owned.parcels.keys().copied().collect::<Vec<_>>(), expected);
            }
            if name == "one" {
                assert_eq!(owned_a.parcels[&181].source, Source::Mining);
            }
            if name == "two" {
                assert!(!owned_a.parcels.contains_key(&181));
                assert!(matches!(owned_b.parcels[&181].source, Source::Transfer { sender, .. } if sender == a));
                assert_eq!(owned_b.parcels[&181].timestamp, fixture("two").timestamp);
            }
            if name == "three" {
                assert_eq!(owned_b.parcels[&181].height, 2, "painting must preserve receipt");
            }
        }
        assert_eq!(owned_a.parcels[&181].source, Source::Mining, "reorg must remove orphaned transfer receipt");
        assert_eq!(owned_a.parcels[&181].height, 1);
        assert!(!owned_b.parcels.contains_key(&181));
        let mut recovered = Ownership::default();
        recovered.update(&chain, a);
        assert_eq!(recovered.parcels, owned_a.parcels, "late join reconstructs dates");
    }
    #[test]
    fn timestamp_format_handles_protocol_extremes() {
        assert_eq!(timestamp_utc(1_700_000_000), "2023-11-14 22:13:20 UTC");
        assert!(timestamp_utc(u64::MAX).contains("Unix seconds"));
    }

    #[test]
    fn self_transfer_preserves_date_and_transfer_back_records_new_acquisition() {
        let mut chain = Chain::default();
        chain.insert(fixture("one"), 1_700_000_100).unwrap();
        let a = fixture("one").miner;
        let b = fixture("fork2").miner;
        let mut owned = Ownership::default();
        owned.update(&chain, a);
        let original = owned.parcels[&181].clone();
        let parent = chain.head().clone();
        let tx = crate::protocol::Transaction {
            parcel: 181,
            counter: 0,
            sender: a,
            action: Action::Transfer { recipient: a },
            signature: Default::default(),
        };
        // Receipt derivation runs after validation; these synthetic entries
        // isolate event ordering without spending proof-of-work on unit tests.
        let mut entry = Entry {
            block: Block { previous: chain.tip, height: 2, miner: b, timestamp: 1_700_000_002, nonce: 0, transactions: vec![tx.clone()] },
            state: parent.state.clone(),
        };
        owned.apply(entry.block.hash(), &entry, &parent, a);
        assert_eq!(owned.parcels[&181], original);
        let mut outbound = tx.clone();
        outbound.action = Action::Transfer { recipient: b };
        let mut inbound = tx;
        inbound.sender = b;
        entry.block.transactions = vec![outbound, inbound];
        owned.apply(entry.block.hash(), &entry, &parent, a);
        assert_eq!(owned.parcels[&181].timestamp, 1_700_000_002);
        assert!(matches!(owned.parcels[&181].source, Source::Transfer { sender, .. } if sender == b));
    }
}
