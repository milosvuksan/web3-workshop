use baneworkshop::{
    Error,
    chain::Chain,
    crypto,
    mempool::Mempool,
    mining,
    protocol::{
        Action, Block, Bytes, Transaction,
        params::*,
        validation::{transition, valid_pow},
    },
    state::State,
};
use ed25519_dalek::SigningKey;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/prototype-v1.json")).unwrap()
}
fn block(name: &str) -> Block {
    serde_json::from_value(vectors()[name]["block"].clone()).unwrap()
}
fn transaction(name: &str) -> Transaction {
    serde_json::from_value(vectors()[name]["transaction"].clone()).unwrap()
}
const NOW: u64 = 1_700_000_010;
#[test]
fn sequential_transactions_and_replay() {
    let mut chain = Chain::default();
    chain.insert(block("one"), NOW).unwrap();
    let parcel = usize::from(transaction("paint").parcel);
    let before = chain.head().state.clone();
    assert!(chain.insert(block("invalid"), NOW).is_err());
    assert_eq!(chain.head().state, before);
    assert!(chain.insert(block("duplicate"), NOW).is_err());
    assert_eq!(chain.head().state, before);
    chain.insert(block("two"), NOW).unwrap();
    assert_eq!(chain.head().state.parcels[parcel].counter, 2);
    assert_eq!(
        chain.head().state.parcels[parcel].color,
        Bytes([255, 128, 0])
    );
    assert!(
        chain
            .head()
            .state
            .clone()
            .apply(&transaction("paint"))
            .is_err()
    );
    chain.insert(block("three"), NOW).unwrap();
    assert_eq!(chain.head().state.parcels[parcel].color, Bytes([0, 0, 255]));
    assert_eq!(chain.head().state.parcels[parcel].counter, 3);
}
#[test]
fn fork_choice_reorganization_and_mempool() {
    let mut a = Chain::default();
    let mut b = Chain::default();
    for c in [&mut a, &mut b] {
        c.insert(block("one"), NOW).unwrap();
    }
    for name in ["two", "fork2"] {
        a.insert(block(name), NOW).unwrap();
    }
    for name in ["fork2", "two"] {
        b.insert(block(name), NOW).unwrap();
    }
    assert_eq!(a.tip, b.tip);
    assert_eq!(a.tip, block("two").hash().min(block("fork2").hash()));
    a.insert(block("three"), NOW).unwrap();
    let old = a.tip;
    assert!(a.detached(old).is_empty());
    assert!(a.detached(block("one").hash()).is_empty());
    assert!(a.detached(Bytes([0xff; 32])).is_empty());
    let mut pool = Mempool::default();
    for name in ["fork3", "fork4"] {
        a.insert(block(name), NOW).unwrap();
    }
    assert_eq!(a.tip, block("fork4").hash());
    let parcel = usize::from(transaction("paint").parcel);
    let p = &a.head().state.parcels[parcel];
    assert_eq!(p.counter, 0);
    assert_eq!(p.color, Bytes::default());
    assert_eq!(p.owner, Some(transaction("paint").sender));
    let detached = a.detached(old);
    assert_eq!(
        detached,
        vec![
            transaction("paint"),
            transaction("transfer"),
            transaction("b_paint")
        ]
    );
    pool.reconcile(&a.head().state, detached);
    assert_eq!(pool.transactions.len(), 3);
    assert_eq!(pool.projected(&a.head().state).parcels[parcel].counter, 3);
    assert!(a.entries.contains_key(&old));
    // Invalid side branches never become committed entries.
    let count = a.entries.len();
    assert!(a.insert(block("invalid"), NOW).is_err());
    assert_eq!(a.entries.len(), count);
}
#[test]
fn mempool_pending_counters_and_confirmation() {
    let mut c = Chain::default();
    c.insert(block("one"), NOW).unwrap();
    let mut p = Mempool::default();
    assert!(p.admit(transaction("transfer"), &c.head().state).is_err());
    assert!(p.admit(transaction("paint"), &c.head().state).unwrap());
    assert!(!p.admit(transaction("paint"), &c.head().state).unwrap());
    p.admit(transaction("transfer"), &c.head().state).unwrap();
    assert_eq!(p.select(&c.head().state).len(), 2);
    c.insert(block("two"), NOW).unwrap();
    p.reconcile(&c.head().state, vec![]);
    assert!(p.transactions.is_empty());
}
#[test]
fn mempool_reorganization_restores_detached_dependencies_before_pending_children() {
    let mut c = Chain::default();
    c.insert(block("one"), NOW).unwrap();
    let original_state = c.head().state.clone();
    c.insert(block("two"), NOW).unwrap();
    let mut pool = Mempool::default();
    pool.admit(transaction("b_paint"), &c.head().state).unwrap();
    // The pending B action initially fails on A's restored ownership. The
    // detached paint and transfer must execute first, then B's action is retried.
    pool.reconcile(
        &original_state,
        vec![
            transaction("paint"),
            transaction("transfer"),
            transaction("paint"),
        ],
    );
    assert_eq!(
        pool.select(&original_state),
        vec![
            transaction("paint"),
            transaction("transfer"),
            transaction("b_paint")
        ]
    );
    c.insert(block("three"), NOW).unwrap();
    pool.reconcile(&c.head().state, vec![transaction("transfer")]);
    assert!(pool.transactions.is_empty());
}
#[test]
fn bounds_signature_ownership_and_counter_exhaustion() {
    let mut c = Chain::default();
    c.insert(block("one"), NOW).unwrap();
    let mut state = c.head().state.clone();
    let original = state.clone();
    let mut tx = transaction("paint");
    tx.parcel = 2048;
    assert!(state.apply(&tx).is_err());
    tx = transaction("paint");
    tx.signature.0[0] ^= 1;
    assert!(state.apply(&tx).is_err());
    assert!(state.apply(&transaction("wrong_signer")).is_err());
    assert_eq!(state, original);
    let seed: Bytes<32> = serde_json::from_value(vectors()["a_seed"].clone()).unwrap();
    let key = SigningKey::from_bytes(&seed.0);
    let mut tx = transaction("paint");
    state.parcels[usize::from(tx.parcel)].counter = u64::MAX;
    tx.counter = u64::MAX;
    crypto::sign(&mut tx, &key);
    let old = state.clone();
    assert!(state.apply(&tx).is_err());
    assert_eq!(state, old);
    let mut tx = transaction("transfer");
    tx.counter = 0;
    tx.action = Action::Transfer {
        recipient: Bytes::default(),
    };
    crypto::sign(&mut tx, &key);
    assert!(original.clone().apply(&tx).is_err());
}
#[test]
fn allocation_cycles_full_canvas_and_large_timestamp() {
    let miner = block("one").miner;
    let mut h = Bytes::default();
    h.0[31] = 1;
    let mut s = State::default();
    s.parcels[1].owner = Some(miner);
    assert_eq!(s.reward(h, 1, miner), None); // Fixed-point cycle despite free parcels.
    assert_eq!(s.reward(h, 2, miner), Some(2));
    let mut full = State::default();
    for p in &mut full.parcels {
        p.owner = Some(miner);
    }
    assert_eq!(full.reward(h, u64::MAX, miner), None);
    let mut large = State::default();
    large.parcels[1].owner = Some(miner);
    assert_eq!(large.reward(h, u64::MAX, miner), Some(2047));
    let mut even = State::default();
    for i in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 0] {
        even.parcels[i].owner = Some(miner);
    }
    assert_eq!(even.reward(h, 2, miner), None);
}
#[test]
fn block_validation_unknown_parent_future_and_cancellation() {
    let mut chain = Chain::default();
    assert!(matches!(
        chain.insert(block("two"), NOW),
        Err(Error::UnknownParent(_))
    ));
    assert!(matches!(chain.insert(block("one"), 0), Err(Error::Future)));
    chain.insert(block("one"), NOW).unwrap();
    let parent = &chain.head().block;
    let state = &chain.head().state;
    let mut b = block("two");
    b.height += 1;
    assert!(transition(&b, parent, state, NOW).is_err());
    b = block("two");
    b.timestamp = parent.timestamp - 1;
    assert!(transition(&b, parent, state, NOW).is_err());
    b = block("two");
    b.transactions = vec![transaction("paint"); MAX_TRANSACTIONS + 1];
    assert!(transition(&b, parent, state, NOW).is_err());
    let mut bad = Bytes::default();
    bad.0[2] = 2;
    assert!(!valid_pow(bad));
    bad.0[2] = 1;
    assert!(valid_pow(bad));
    let mut b = block("two");
    b.nonce = 0;
    assert!(mining::mine(b, &AtomicBool::new(true)).is_none());
    assert_eq!(PARCELS, WIDTH * HEIGHT);
}
#[test]
fn a_new_reward_cannot_be_spent_in_its_block() {
    let mut s = State::default();
    assert!(s.apply(&transaction("paint")).is_err());
    assert_eq!(s, State::default());
}

#[test]
fn self_transfer_and_transfer_back_preserve_replay_history() {
    // SPEC section 3: self-transfer increments the parcel counter, which must
    // survive a round trip through another owner so old signatures stay spent.
    let mut chain = Chain::default();
    chain.insert(block("one"), NOW).unwrap();
    let mut state = chain.head().state.clone();
    let v = vectors();
    let a_seed: Bytes<32> = serde_json::from_value(v["a_seed"].clone()).unwrap();
    let b_seed: Bytes<32> = serde_json::from_value(v["b_seed"].clone()).unwrap();
    let a = SigningKey::from_bytes(&a_seed.0);
    let b = SigningKey::from_bytes(&b_seed.0);
    let mut tx = transaction("paint");
    let parcel = usize::from(tx.parcel);
    let initial_color = state.parcels[parcel].color;
    tx.action = Action::Transfer {
        recipient: crypto::public_key(&a),
    };
    crypto::sign(&mut tx, &a);
    let old_self_transfer = tx.clone();
    state.apply(&tx).unwrap();
    assert_eq!(state.parcels[parcel].owner, Some(crypto::public_key(&a)));
    assert_eq!(state.parcels[parcel].counter, 1);

    tx.counter = 1;
    tx.action = Action::Transfer {
        recipient: crypto::public_key(&b),
    };
    crypto::sign(&mut tx, &a);
    state.apply(&tx).unwrap();
    tx.counter = 2;
    tx.action = Action::Transfer {
        recipient: crypto::public_key(&a),
    };
    crypto::sign(&mut tx, &b);
    state.apply(&tx).unwrap();
    assert_eq!(state.parcels[parcel].owner, Some(crypto::public_key(&a)));
    assert_eq!(state.parcels[parcel].counter, 3);
    assert_eq!(state.parcels[parcel].color, initial_color);
    let before = state.clone();
    assert!(state.apply(&old_self_transfer).is_err());
    assert_eq!(state, before);
}
