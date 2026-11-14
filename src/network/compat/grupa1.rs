use crate::{
    Error, Result, crypto,
    protocol::{Bytes, Color, Hash, PublicKey, Signature},
};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_INTEGER: u64 = (1 << 53) - 1;
pub const GENESIS: Hash = Bytes([0; 32]);
pub const MAX_PENDING: usize = 1024;
pub const MAX_WAITING: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Paint,
    Transfer,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Self::Paint => "paint",
            Self::Transfer => "transfer",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    #[serde(rename = "type")]
    pub kind: Kind,
    pub sender: PublicKey,
    pub nonce: u64,
    pub parcel: u8,
    pub value: String,
    pub signature: Signature,
}
impl Transaction {
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "{}|{}|{}|{}|{}",
            self.kind.text(),
            self.sender,
            self.nonce,
            self.parcel,
            self.value
        )
        .into_bytes()
    }
    pub fn sign(&mut self, key: &SigningKey) {
        self.sender = crypto::public_key(key);
        self.signature = Bytes(key.sign(&self.signing_bytes()).to_bytes());
    }
    pub fn validate(&self) -> Result<()> {
        if self.nonce > MAX_INTEGER {
            return Err(Error::Invalid("Grupa 1 nonce exceeds 2^53 - 1"));
        }
        match self.kind {
            Kind::Paint => {
                self.value.parse::<Color>()?;
            }
            Kind::Transfer => {
                self.value.parse::<PublicKey>()?;
            }
        }
        ed25519_dalek::VerifyingKey::from_bytes(&self.sender.0)
            .map_err(|_| Error::Invalid("invalid Grupa 1 public key"))?
            .verify_strict(
                &self.signing_bytes(),
                &ed25519_dalek::Signature::from_bytes(&self.signature.0),
            )
            .map_err(|_| Error::Invalid("invalid Grupa 1 signature"))
    }
    pub fn id(&self) -> Hash {
        crypto::hash(&self.canonical_bytes())
    }
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = self.signing_bytes();
        bytes.extend_from_slice(format!("|{}", self.signature).as_bytes());
        bytes
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub height: u64,
    pub prev_hash: Hash,
    pub miner: PublicKey,
    pub nonce: u64,
    pub transactions: Vec<Transaction>,
}
impl Block {
    pub fn genesis() -> Self {
        Self {
            height: 0,
            prev_hash: GENESIS,
            miner: GENESIS,
            nonce: 0,
            transactions: vec![],
        }
    }
    pub fn hash(&self) -> Hash {
        if self.height == 0
            && self.prev_hash == GENESIS
            && self.miner == GENESIS
            && self.nonce == 0
            && self.transactions.is_empty()
        {
            return GENESIS;
        }
        let mut bytes = format!(
            "{}|{}|{}|{}",
            self.height, self.prev_hash, self.miner, self.nonce
        )
        .into_bytes();
        for tx in &self.transactions {
            bytes.push(b'\n');
            bytes.extend(tx.canonical_bytes());
        }
        crypto::hash(&bytes)
    }
    pub fn validate(&self) -> Result<()> {
        if self.height == 0 || self.height > MAX_INTEGER || self.nonce > MAX_INTEGER {
            return Err(Error::Invalid("invalid Grupa 1 height or nonce"));
        }
        if !valid_pow(self.hash()) {
            return Err(Error::Invalid("Grupa 1 requires 20 leading zero bits"));
        }
        for tx in &self.transactions {
            tx.validate()?;
        }
        Ok(())
    }
}
pub fn valid_pow(hash: Hash) -> bool {
    hash.0[0] == 0 && hash.0[1] == 0 && hash.0[2] < 16
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Parcel {
    pub owner: Option<PublicKey>,
    pub color: Color,
}
#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub parcels: Vec<Parcel>,
    pub nonces: BTreeMap<PublicKey, u64>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            parcels: vec![Parcel::default(); 256],
            nonces: BTreeMap::new(),
        }
    }
}
impl State {
    fn apply(&mut self, tx: &Transaction) -> Result<()> {
        let parcel = &mut self.parcels[usize::from(tx.parcel)];
        if parcel.owner != Some(tx.sender) {
            return Err(Error::Invalid("Grupa 1 sender does not own parcel"));
        }
        if tx.nonce != *self.nonces.get(&tx.sender).unwrap_or(&0) {
            return Err(Error::Invalid("incorrect Grupa 1 sender nonce"));
        }
        match tx.kind {
            Kind::Paint => parcel.color = tx.value.parse()?,
            Kind::Transfer => parcel.owner = Some(tx.value.parse()?),
        }
        self.nonces.insert(tx.sender, tx.nonce + 1);
        Ok(())
    }
}
pub struct Entry {
    pub block: Block,
    pub state: State,
}
pub struct Ledger {
    pub entries: BTreeMap<Hash, Entry>,
    pub tip: Hash,
    pub pending: Vec<Transaction>,
    pub waiting: BTreeMap<Hash, Block>,
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            entries: BTreeMap::from([(
                GENESIS,
                Entry {
                    block: Block::genesis(),
                    state: State::default(),
                },
            )]),
            tip: GENESIS,
            pending: vec![],
            waiting: BTreeMap::new(),
        }
    }
}
impl Ledger {
    pub fn head(&self) -> &Entry {
        &self.entries[&self.tip]
    }
    pub fn projected(&self) -> State {
        let mut state = self.head().state.clone();
        for tx in &self.pending {
            let _ = state.apply(tx);
        }
        state
    }
    pub fn admit(&mut self, tx: Transaction) -> Result<()> {
        tx.validate()?;
        if self.pending.contains(&tx) {
            return Ok(());
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(Error::Invalid("Grupa 1 transaction queue full"));
        }
        self.projected().apply(&tx)?;
        self.pending.push(tx);
        Ok(())
    }
    /// 200 = connected/known; 202 = valid envelope waiting for its parent.
    pub fn accept(&mut self, block: Block) -> Result<u16> {
        if block == Block::genesis() {
            return Ok(200);
        }
        block.validate()?;
        let hash = block.hash();
        if self.entries.contains_key(&hash) {
            return Ok(200);
        }
        if !self.entries.contains_key(&block.prev_hash) {
            if self.waiting.len() >= MAX_WAITING && !self.waiting.contains_key(&hash) {
                return Err(Error::Invalid("Grupa 1 orphan queue full"));
            }
            self.waiting.insert(hash, block);
            return Ok(202);
        }
        self.insert(block)?;
        loop {
            let ready = self
                .waiting
                .iter()
                .find(|(_, b)| self.entries.contains_key(&b.prev_hash))
                .map(|(h, _)| *h);
            let Some(hash) = ready else {
                break;
            };
            let block = self.waiting.remove(&hash).unwrap();
            let _ = self.insert(block);
        }
        Ok(200)
    }
    fn insert(&mut self, block: Block) -> Result<()> {
        let parent = &self.entries[&block.prev_hash];
        if block.height != parent.block.height + 1 {
            return Err(Error::Invalid("incorrect Grupa 1 block height"));
        }
        let mut state = parent.state.clone();
        for tx in &block.transactions {
            state.apply(tx)?;
        }
        let hash = block.hash();
        let parcel = &mut state.parcels[usize::from(hash.0[31])];
        if parcel.owner.is_none() {
            parcel.owner = Some(block.miner);
        }
        let wins = block.height > self.head().block.height
            || (block.height == self.head().block.height && hash > self.tip);
        let old_tip = self.tip;
        self.entries.insert(hash, Entry { block, state });
        if wins {
            self.tip = hash;
            // Return transactions from detached blocks to the projected queue.
            let mut detached = vec![];
            let mut cursor = old_tip;
            let mut canonical = self.tip;
            while cursor != canonical {
                let old = &self.entries[&cursor].block;
                let new = &self.entries[&canonical].block;
                if old.height >= new.height {
                    detached.extend(old.transactions.iter().rev().cloned());
                    cursor = old.prev_hash;
                } else {
                    canonical = new.prev_hash;
                }
            }
            detached.reverse();
            detached.append(&mut self.pending);
            let mut projected = self.head().state.clone();
            for tx in detached {
                if self.pending.len() < MAX_PENDING && projected.apply(&tx).is_ok() {
                    self.pending.push(tx);
                }
            }
        }
        Ok(())
    }
    pub fn path(&self) -> Vec<Hash> {
        let mut cursor = self.tip;
        let mut path = vec![];
        while cursor != GENESIS {
            path.push(cursor);
            cursor = self.entries[&cursor].block.prev_hash;
        }
        path.reverse();
        path
    }
    pub fn candidate(&self, miner: PublicKey) -> Block {
        Block {
            height: self.head().block.height + 1,
            prev_hash: self.tip,
            miner,
            nonce: 0,
            transactions: self.pending.clone(),
        }
    }
    pub fn summary(&self, identity: PublicKey) -> serde_json::Value {
        serde_json::json!({"profile":"Grupa 1 - Luka", "genesis":GENESIS, "height":self.head().block.height, "hash":self.tip,
            "mempool":self.pending.len(), "waiting":self.waiting.len(), "owned":self.head().state.parcels.iter().enumerate().filter(|(_, p)| p.owner == Some(identity)).map(|(i, _)| i).collect::<Vec<_>>()})
    }
}

/// Uses this profile's decimal/LF hash format, never native mining bytes.
pub fn start_miner(
    block: Block,
    ledger: std::sync::Arc<super::Compatibility>,
) -> std::io::Result<crate::mining::Job> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let handle = std::thread::Builder::new()
        .name("grupa1-miner".into())
        .spawn(move || {
            use sha2::{Digest, Sha256};
            let mut block = block;
            let prefix = Sha256::new().chain_update(
                format!("{}|{}|{}|", block.height, block.prev_hash, block.miner).as_bytes(),
            );
            let mut suffix = vec![];
            for tx in &block.transactions {
                suffix.push(b'\n');
                suffix.extend(tx.canonical_bytes());
            }
            let mut digits = [0; 20];
            while block.nonce <= MAX_INTEGER {
                // Frequent cancellation also bounds shutdown time for large blocks.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let mut value = block.nonce;
                let mut start = digits.len();
                loop {
                    start -= 1;
                    digits[start] = b'0' + (value % 10) as u8;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                let hash = Bytes(
                    prefix
                        .clone()
                        .chain_update(&digits[start..])
                        .chain_update(&suffix)
                        .finalize()
                        .into(),
                );
                if valid_pow(hash) {
                    let mut ledger = ledger.ledger.lock().unwrap();
                    if ledger.tip == block.prev_hash {
                        let _ = ledger.accept(block);
                    }
                    return;
                }
                block.nonce += 1;
            }
        })?;
    Ok(crate::mining::Job {
        cancel,
        handles: vec![handle],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn vector() -> Block {
        Block {
            height: 1,
            prev_hash: GENESIS,
            miner: Bytes([0x11; 32]),
            nonce: 1426967,
            transactions: vec![],
        }
    }
    #[test]
    fn supplied_vector_and_genesis() {
        let block = vector();
        assert_eq!(
            block.hash().to_string(),
            "000008a6403b5a52eca437a0912dc7a1d788b2d0e093b3ee72667e1636a024df"
        );
        let mut ledger = Ledger::default();
        assert_eq!(ledger.tip, GENESIS);
        assert_eq!(Block::genesis().hash(), GENESIS);
        assert_eq!(ledger.accept(block.clone()).unwrap(), 200);
        assert_eq!(ledger.head().state.parcels[223].owner, Some(block.miner));
        assert_eq!(
            ledger
                .head()
                .state
                .parcels
                .iter()
                .filter(|p| p.owner.is_some())
                .count(),
            1
        );
        assert_eq!(ledger.accept(block).unwrap(), 200);
    }
    #[test]
    fn signatures_and_nonces_are_profile_specific_and_per_sender() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let owner = crypto::public_key(&key);
        let mut ledger = Ledger::default();
        // Isolate state rules without mining additional test blocks.
        for parcel in [2, 3] {
            ledger.entries.get_mut(&GENESIS).unwrap().state.parcels[parcel].owner = Some(owner);
        }
        let mut tx = Transaction {
            kind: Kind::Paint,
            sender: owner,
            nonce: 0,
            parcel: 2,
            value: "aabbcc".into(),
            signature: Signature::default(),
        };
        tx.sign(&key);
        assert_eq!(
            String::from_utf8(tx.signing_bytes()).unwrap(),
            format!("paint|{owner}|0|2|aabbcc")
        );
        ledger.admit(tx.clone()).unwrap();
        tx.parcel = 3;
        tx.sign(&key);
        assert!(ledger.admit(tx.clone()).is_err());
        tx.nonce = 1;
        tx.sign(&key);
        ledger.admit(tx.clone()).unwrap();
        tx.value = "ffffff".into();
        assert!(tx.validate().is_err());
        assert_eq!(ledger.pending.len(), 2);
    }
    fn fixture(name: &str) -> Block {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/grupa1-luka.json")).unwrap();
        let block: Block = serde_json::from_value(vectors[name]["block"].clone()).unwrap();
        assert_eq!(
            block.hash().to_string(),
            vectors[name]["hash"].as_str().unwrap()
        );
        block
    }
    #[test]
    fn independent_vectors_cover_signatures_transfer_orphans_and_sender_nonce_reset() {
        let one = fixture("one");
        let two = fixture("two");
        let three = fixture("three");
        let parcel = usize::from(one.hash().0[31]);
        let mut ledger = Ledger::default();
        assert_eq!(ledger.accept(three.clone()).unwrap(), 202);
        assert_eq!(ledger.accept(two.clone()).unwrap(), 202);
        ledger.accept(one).unwrap();
        assert_eq!(ledger.tip, three.hash());
        assert!(ledger.waiting.is_empty());
        let state = &ledger.head().state;
        assert_eq!(
            state.parcels[parcel].owner,
            Some(three.transactions[0].sender)
        );
        assert_eq!(state.parcels[parcel].color.to_string(), "123456");
        assert_eq!(state.nonces[&two.transactions[0].sender], 2);
        assert_eq!(state.nonces[&three.transactions[0].sender], 1);
    }
    #[test]
    fn equal_height_uses_larger_hash_and_reorg_restores_detached_transactions() {
        let one = fixture("one");
        let two = fixture("two");
        let fork_two = fixture("fork_two");
        let fork_three = fixture("fork_three");
        let mut ledger = Ledger::default();
        ledger.accept(one).unwrap();
        ledger.accept(two.clone()).unwrap();
        ledger.accept(fork_two.clone()).unwrap();
        assert_eq!(ledger.tip, two.hash().max(fork_two.hash()));
        ledger.accept(fork_three.clone()).unwrap();
        assert_eq!(ledger.tip, fork_three.hash());
        assert_eq!(ledger.pending, two.transactions);
        assert_eq!(
            ledger.projected().parcels[usize::from(two.transactions[0].parcel)].owner,
            Some(two.transactions[1].value.parse().unwrap())
        );
    }
    #[test]
    fn profile_miner_hashes_transaction_suffix_and_joins_workers() {
        use std::{
            sync::Arc,
            time::{Duration, Instant},
        };
        let state = Arc::new(super::super::Compatibility::default());
        state.ledger.lock().unwrap().accept(fixture("one")).unwrap();
        let block = fixture("two");
        // Starts at an independently mined solution, exercising production bytes
        // without spending random time brute-forcing a new test fixture.
        let job = start_miner(block.clone(), state.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !job.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        job.stop();
        assert_eq!(state.ledger.lock().unwrap().tip, block.hash());
        let candidate = state.ledger.lock().unwrap().candidate(block.miner);
        start_miner(candidate, state).unwrap().stop();
    }
    #[test]
    fn state_failure_leaves_tip_and_parent_unchanged() {
        let mut ledger = Ledger::default();
        ledger.accept(fixture("one")).unwrap();
        let two = fixture("two");
        let before = ledger.tip;
        // Corrupt only the parent's snapshot to prove the full state transition
        // rejects an otherwise correctly hashed, mined, signed block atomically.
        ledger.entries.get_mut(&before).unwrap().state.parcels
            [usize::from(two.transactions[0].parcel)]
        .owner = None;
        let state = serde_json::to_value(&ledger.head().state).unwrap();
        assert!(ledger.accept(two).is_err());
        assert_eq!(ledger.tip, before);
        assert_eq!(serde_json::to_value(&ledger.head().state).unwrap(), state);
        assert_eq!(ledger.entries.len(), 2);
    }
    #[test]
    fn strict_fields_and_integer_limits() {
        let mut block = serde_json::to_value(vector()).unwrap();
        block["timestamp"] = 0.into();
        assert!(serde_json::from_value::<Block>(block).is_err());
        let mut block = vector();
        block.nonce = MAX_INTEGER + 1;
        assert!(block.validate().is_err());
        let mut block = vector();
        block.height = 2;
        assert!(Ledger::default().accept(block).is_err());
    }
}
