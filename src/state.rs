use crate::{
    Error, Result, crypto,
    protocol::{Action, Color, Hash, PublicKey, Transaction, params::*},
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parcel {
    pub owner: Option<PublicKey>,
    pub color: Color,
    pub counter: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub parcels: Vec<Parcel>,
}
impl Default for State {
    fn default() -> Self {
        Self { parcels: vec![Parcel::default(); PARCELS] }
    }
}
impl State {
    pub fn apply(&mut self, tx: &Transaction) -> Result<()> {
        let parcel = self.parcels.get_mut(usize::from(tx.parcel)).ok_or(Error::Invalid("parcel out of range"))?;
        crypto::verify(tx)?;
        if let Action::Transfer { recipient } = tx.action {
            crypto::valid_key(recipient)?;
        }
        if parcel.owner != Some(tx.sender) {
            return Err(Error::Invalid("sender is not parcel owner"));
        }
        if tx.counter != parcel.counter {
            return Err(Error::Invalid("incorrect parcel counter"));
        }
        let next = parcel.counter.checked_add(1).ok_or(Error::Invalid("parcel counter exhausted"))?;
        match tx.action {
            Action::Paint { color } => parcel.color = color,
            Action::Transfer { recipient } => parcel.owner = Some(recipient),
        }
        parcel.counter = next;
        Ok(())
    }
    pub fn reward(&mut self, hash: Hash, timestamp: u64, miner: PublicKey) -> Option<u16> {
        let mut candidate = usize::from(u16::from_be_bytes([hash.0[30], hash.0[31]])) % PARCELS;
        for _ in 0..PARCELS {
            if self.parcels[candidate].owner.is_none() {
                self.parcels[candidate].owner = Some(miner);
                return Some(candidate as u16);
            }
            // Reduce timestamp first: exactly the mathematical product, without overflow.
            candidate = candidate * (timestamp % PARCELS as u64) as usize % PARCELS;
        }
        None
    }
    pub fn diagnostic_hash(&self) -> Hash {
        let mut out = Vec::with_capacity(PARCELS * 44);
        for p in &self.parcels {
            out.push(u8::from(p.owner.is_some()));
            out.extend(p.owner.unwrap_or_default().0);
            out.extend(p.color.0);
            out.extend(p.counter.to_be_bytes());
        }
        crypto::hash(&out)
    }
}
