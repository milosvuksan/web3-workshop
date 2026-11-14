use super::{Action, Block, Hash, Transaction, params::*};
use crate::crypto::hash;
impl Transaction {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = TX_DOMAIN.to_vec();
        out.push(match self.action {
            Action::Paint { .. } => 1,
            Action::Transfer { .. } => 2,
        });
        out.extend(self.parcel.to_be_bytes());
        out.extend(self.counter.to_be_bytes());
        out.extend(self.sender.0);
        match self.action {
            Action::Paint { color } => out.extend(color.0),
            Action::Transfer { recipient } => out.extend(recipient.0),
        }
        out
    }
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = self.signing_bytes();
        out.extend(self.signature.0);
        out
    }
    pub fn id(&self) -> Hash {
        let mut bytes = ID_DOMAIN.to_vec();
        bytes.extend(self.canonical_bytes());
        hash(&bytes)
    }
}
impl Block {
    pub fn transaction_commitment(&self) -> Hash {
        let mut out = Vec::new();
        for tx in &self.transactions {
            let bytes = tx.canonical_bytes();
            out.extend((bytes.len() as u16).to_be_bytes());
            out.extend(bytes);
        }
        hash(&out)
    }
    /// Nonce is last so a miner can reuse the SHA-256 prefix state.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = BLOCK_DOMAIN.to_vec();
        out.extend(self.previous.0);
        out.extend(self.height.to_be_bytes());
        out.extend(self.miner.0);
        out.extend(self.timestamp.to_be_bytes());
        out.extend((self.transactions.len() as u16).to_be_bytes());
        out.extend(self.transaction_commitment().0);
        out.extend(self.nonce.to_be_bytes());
        out
    }
    pub fn hash(&self) -> Hash {
        hash(&self.canonical_bytes())
    }
}
