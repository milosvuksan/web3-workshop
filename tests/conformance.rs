use baneworkshop::{
    crypto,
    protocol::{Block, Bytes, Transaction, params::genesis, validation::valid_pow},
    state::State,
};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;
fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/prototype-v1.json")).unwrap()
}
#[test]
fn standard_crypto_vectors() {
    assert_eq!(
        crypto::hash(b"abc").to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let seed: Bytes<32> = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
        .parse()
        .unwrap();
    let key = SigningKey::from_bytes(&seed.0);
    assert_eq!(
        crypto::public_key(&key).to_string(),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    let expected: Bytes<64> = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b".parse().unwrap();
    let signature = ed25519_dalek::Signature::from_bytes(&expected.0);
    assert_eq!(key.sign(b"").to_bytes(), expected.0);
    key.verifying_key().verify_strict(b"", &signature).unwrap();
    assert!(
        key.verifying_key()
            .verify_strict(b"changed", &signature)
            .is_err()
    );
}
#[test]
fn nine_project_vectors_and_pow_fixtures() {
    let v = vectors();
    for name in ["a", "b"] {
        let seed: Bytes<32> = serde_json::from_value(v[format!("{name}_seed")].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(crypto::public_key(&SigningKey::from_bytes(&seed.0))).unwrap(),
            v[format!("{name}_public")]
        );
    }
    for name in ["paint", "transfer", "b_paint", "wrong_signer"] {
        let tx: Transaction = serde_json::from_value(v[name]["transaction"].clone()).unwrap();
        assert_eq!(hex::encode(tx.signing_bytes()), v[name]["signing_hex"]);
        assert_eq!(hex::encode(tx.canonical_bytes()), v[name]["canonical_hex"]);
        assert_eq!(serde_json::to_value(tx.id()).unwrap(), v[name]["id"]);
        crypto::verify(&tx).unwrap();
    }
    for name in [
        "genesis",
        "one",
        "two",
        "three",
        "fork2",
        "fork3",
        "fork4",
        "invalid",
        "duplicate",
    ] {
        let b: Block = serde_json::from_value(v[name]["block"].clone()).unwrap();
        assert_eq!(hex::encode(b.canonical_bytes()), v[name]["canonical_hex"]);
        assert_eq!(serde_json::to_value(b.hash()).unwrap(), v[name]["hash"]);
        if name != "genesis" {
            assert!(valid_pow(b.hash()));
        } else {
            assert_eq!(b, genesis());
        }
    }
    let one: Block = serde_json::from_value(v["one"]["block"].clone()).unwrap();
    let mut state = State::default();
    assert_eq!(
        state.reward(one.hash(), one.timestamp, one.miner).unwrap() as u64,
        v["reward_one"].as_u64().unwrap()
    );
    let collision = &v["occupied_retry"];
    let hash = serde_json::from_value(collision["hash"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(state.reward(hash, 0, one.miner)).unwrap(),
        collision["reward"]
    );
}
#[test]
fn malformed_text_and_fields_rejected() {
    for text in ["00", &"AA".repeat(32), &"gg".repeat(32)] {
        assert!(text.parse::<Bytes<32>>().is_err());
    }
    let mut tx = vectors()["paint"]["transaction"].clone();
    tx["signature"] = Value::String("00".repeat(63));
    assert!(serde_json::from_value::<Transaction>(tx).is_err());
    let mut tx = vectors()["paint"]["transaction"].clone();
    tx["parcel"] = Value::from(65536);
    assert!(serde_json::from_value::<Transaction>(tx).is_err());
    let mut tx = vectors()["paint"]["transaction"].clone();
    tx["action"]["color"] = Value::String("01020304".into());
    assert!(serde_json::from_value::<Transaction>(tx).is_err());
}

#[test]
fn json_preserves_full_width_unsigned_integers() {
    // SPEC sections 2/9 require all 64 bits to survive wire decoding. Use values
    // above the exact-integer range of IEEE-754 doubles, including u64::MAX.
    let v = vectors();
    let mut block: Block = serde_json::from_value(v["two"]["block"].clone()).unwrap();
    block.height = (1_u64 << 53) + 1;
    block.timestamp = u64::MAX - 1;
    block.nonce = u64::MAX;
    block.transactions[0].counter = (1_u64 << 63) + 1;
    let message = baneworkshop::network::wire::Message::Block {
        block: block.clone(),
    };
    let mut line = serde_json::to_vec(&message).unwrap();
    line.push(b'\n');
    let decoded = baneworkshop::network::wire::Framer::default()
        .push(&line)
        .unwrap();
    let [baneworkshop::network::wire::Message::Block { block: decoded }] = decoded.as_slice()
    else {
        panic!("expected one complete block message");
    };
    assert_eq!(decoded, &block);
    assert_eq!(decoded.canonical_bytes(), block.canonical_bytes());
    assert_eq!(
        decoded.transactions[0].signing_bytes(),
        block.transactions[0].signing_bytes()
    );
}
