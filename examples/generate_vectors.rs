//! Regenerate prototype vectors at the real target: cargo run --release --example generate_vectors.
use baneworkshop::{
    crypto, mining,
    protocol::{Action, Block, Bytes, Transaction, params::genesis},
    state::State,
};
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::sync::atomic::AtomicBool;
fn child(parent: &Block, miner: &SigningKey, transactions: Vec<Transaction>, timestamp: u64) -> Block {
    let block =
        Block { previous: parent.hash(), height: parent.height + 1, miner: crypto::public_key(miner), timestamp, nonce: 0, transactions };
    let found = mining::mine(block, &AtomicBool::new(false)).unwrap();
    eprintln!("mined fixture height {}: {} nonce {}", found.height, found.hash(), found.nonce);
    found
}
fn tx(key: &SigningKey, parcel: u16, counter: u64, action: Action) -> Transaction {
    let mut tx = Transaction { parcel, counter, sender: Bytes::default(), action, signature: Bytes::default() };
    crypto::sign(&mut tx, key);
    tx
}
fn main() {
    let a_seed: Bytes<32> = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60".parse().unwrap();
    let a = SigningKey::from_bytes(&a_seed.0);
    let b = SigningKey::from_bytes(&[2; 32]);
    let genesis = genesis();
    let one = child(&genesis, &a, vec![], 1_700_000_000);
    let mut state = State::default();
    let parcel = state.reward(one.hash(), one.timestamp, one.miner).unwrap();
    let paint = tx(&a, parcel, 0, Action::Paint { color: Bytes([255, 128, 0]) });
    let transfer = tx(&a, parcel, 1, Action::Transfer { recipient: crypto::public_key(&b) });
    let wrong = tx(&b, parcel, 1, Action::Paint { color: Bytes([1, 2, 3]) });
    let two = child(&one, &a, vec![paint.clone(), transfer.clone()], 1_700_000_002);
    let bpaint = tx(&b, parcel, 2, Action::Paint { color: Bytes([0, 0, 255]) });
    let three = child(&two, &b, vec![bpaint.clone()], 1_700_000_004);
    let fork2 = child(&one, &b, vec![], 1_700_000_003);
    let fork3 = child(&fork2, &b, vec![], 1_700_000_005);
    let fork4 = child(&fork3, &b, vec![], 1_700_000_006);
    let invalid = child(&one, &a, vec![paint.clone(), wrong.clone()], 1_700_000_007);
    let duplicate = child(&one, &a, vec![paint.clone(), paint.clone()], 1_700_000_008);
    let tx_vector = |tx: &Transaction| json!({"transaction":tx, "signing_hex":hex::encode(tx.signing_bytes()), "canonical_hex":hex::encode(tx.canonical_bytes()), "id":tx.id()});
    let block_vector = |b: &Block| json!({"block":b, "canonical_hex":hex::encode(b.canonical_bytes()), "hash":b.hash()});
    let mut collision = one.hash();
    collision.0[30..].copy_from_slice(&parcel.to_be_bytes());
    let occupied = state.reward(collision, 0, one.miner);
    let result = json!({"provenance":"Rust prototype v1; deterministic public test seeds; not yet cross-language approved", "a_seed":a_seed,"a_public":crypto::public_key(&a), "b_seed":Bytes([2;32]),"b_public":crypto::public_key(&b),
        "paint":tx_vector(&paint),"transfer":tx_vector(&transfer),"b_paint":tx_vector(&bpaint),"wrong_signer":tx_vector(&wrong),
        "genesis":block_vector(&genesis),"one":block_vector(&one),"two":block_vector(&two),"three":block_vector(&three),
        "fork2":block_vector(&fork2),"fork3":block_vector(&fork3),"fork4":block_vector(&fork4),
        "invalid":block_vector(&invalid),"duplicate":block_vector(&duplicate),
        "reward_one":parcel,"occupied_retry":{"hash":collision,"timestamp":0,"occupied":[parcel],"reward":occupied}});
    std::fs::write("tests/fixtures/prototype-v1.json", serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
