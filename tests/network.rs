use baneworkshop::{
    crypto,
    network::{
        lag::Lag,
        wire::{Framer, MAX_LINE, Message},
    },
    protocol::Bytes,
};
#[test]
fn network_document_genesis_and_signed_paint_examples() {
    let genesis = br#"{"type":"block","block":{"previous":"0000000000000000000000000000000000000000000000000000000000000000","height":0,"miner":"0000000000000000000000000000000000000000000000000000000000000000","timestamp":0,"nonce":0,"transactions":[]}}
"#;
    let Message::Block { block } = Framer::default().push(genesis).unwrap().remove(0) else {
        panic!("genesis");
    };
    assert_eq!(
        block.hash().to_string(),
        "56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce"
    );
    let paint = br#"{"type":"transaction","transaction":{"parcel":181,"counter":0,"sender":"8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c","action":{"type":"paint","color":"ff8000"},"signature":"f6e77abad4b93a31784bfaa4824108e5335f8f027e8df9fbfd20d55a8d0434bbc2017053a5345f0f76947ead36d832728283994276d9ca85380f4225acc3560a"}}
"#;
    let Message::Transaction { transaction } = Framer::default().push(paint).unwrap().remove(0)
    else {
        panic!("transaction");
    };
    crypto::verify(&transaction).unwrap();
    assert_eq!(
        transaction.id().to_string(),
        "f9b61b328fa0214ae3b25c7f32c124831770317aab1419483d966134a77123cd"
    );
}

#[test]
fn wire_rejects_missing_extra_and_mistyped_fields_at_every_level() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/prototype-v1.json")).unwrap();
    let block = serde_json::json!({"type":"block", "block":fixture["two"]["block"]});
    for path in [
        vec![],
        vec!["block"],
        vec!["block", "transactions", "0"],
        vec!["block", "transactions", "0", "action"],
    ] {
        for extra in [true, false] {
            let mut bad = block.clone();
            let mut object = &mut bad;
            for key in &path {
                object = if *key == "0" {
                    &mut object[0]
                } else {
                    &mut object[*key]
                };
            }
            let object = object.as_object_mut().unwrap();
            if extra {
                object.insert("unexpected".into(), true.into());
            } else {
                let field = object.keys().next().unwrap().clone();
                object.remove(&field);
            }
            let mut line = serde_json::to_vec(&bad).unwrap();
            line.push(b'\n');
            assert!(Framer::default().push(&line).is_err());
        }
    }
    for height in ["-1", "1.5", "1.0", "18446744073709551616", "\"1\""] {
        let line = format!(
            "{{\"type\":\"tip\",\"hash\":\"{}\",\"height\":{height}}}\n",
            Bytes([0; 32])
        );
        assert!(Framer::default().push(line.as_bytes()).is_err());
    }
    let mut line = serde_json::to_vec(&block).unwrap();
    line.extend(vec![b' '; MAX_LINE - line.len()]);
    line.push(b'\n');
    assert_eq!(Framer::default().push(&line).unwrap().len(), 1);
}
#[test]
fn partial_reads_coalescing_and_bounds() {
    let mut f = Framer::default();
    assert!(f.push(b"{\"type\":\"get_").unwrap().is_empty());
    let messages = f.push(b"tip\"}\n{\"type\":\"get_tip\"}\n").unwrap();
    assert_eq!(messages.len(), 2);
    assert!(matches!(messages[0], Message::GetTip {}));
    let mut f = Framer::default();
    assert!(f.push(&vec![b' '; MAX_LINE]).unwrap().is_empty());
    assert!(f.push(b" ").is_err());
    for bytes in [
        b"{\"type\":\"unknown\"}\n".as_slice(),
        b"\xff\n",
        b"{}\n",
        b"{\"type\":\"get_tip\",\"extra\":1}\n",
    ] {
        assert!(Framer::default().push(bytes).is_err());
    }
    let msg = Message::GetBlock {
        hash: Bytes([7; 32]),
    };
    let mut line = serde_json::to_vec(&msg).unwrap();
    line.push(b'\n');
    assert_eq!(Framer::default().push(&line).unwrap().len(), 1);
}
#[test]
fn deterministic_lag_has_loss_delay_and_disable() {
    let mut a = Lag::new(true, 42);
    let mut b = Lag::new(true, 42);
    let mut dropped = 0;
    for _ in 0..1000 {
        let next = a.schedule();
        assert_eq!(next, b.schedule());
        if let Some(delay) = next {
            assert!((20..=250).contains(&delay));
        } else {
            dropped += 1;
        }
    }
    assert!((60..=140).contains(&dropped));
    let mut disabled = Lag::new(false, 42);
    for _ in 0..100 {
        assert_eq!(disabled.schedule(), Some(0));
    }
}

#[test]
fn bounded_orphan_storage_prioritizes_ancestry() {
    use baneworkshop::{
        network::sync::{MAX_WAITING, Sync},
        protocol::{Block, params::genesis},
    };
    let mut sync = Sync::default();
    for height in 2..MAX_WAITING as u64 + 2 {
        let b = Block {
            height,
            ..genesis()
        };
        sync.want(b.hash());
        sync.defer(b);
    }
    assert_eq!(sync.waiting.len(), MAX_WAITING);
    assert!(sync.wanted.is_empty());
    let ancestor = Block {
        height: 1,
        ..genesis()
    };
    let hash = ancestor.hash();
    sync.defer(ancestor);
    assert_eq!(sync.waiting.len(), MAX_WAITING);
    assert!(sync.waiting.contains_key(&hash));
    let too_new = Block {
        height: 9999,
        ..genesis()
    };
    let hash = too_new.hash();
    sync.defer(too_new);
    assert_eq!(sync.waiting.len(), MAX_WAITING);
    assert!(!sync.waiting.contains_key(&hash));
}

#[test]
fn duplicate_orphan_does_not_evict_other_pending_blocks() {
    use baneworkshop::{
        network::sync::{MAX_WAITING, Sync},
        protocol::{Block, params::genesis},
    };
    let mut sync = Sync::default();
    for height in 1..=MAX_WAITING as u64 {
        sync.defer(Block {
            height,
            ..genesis()
        });
    }
    let before: Vec<_> = sync.waiting.keys().copied().collect();
    sync.defer(Block {
        height: 1,
        ..genesis()
    });
    assert_eq!(sync.waiting.keys().copied().collect::<Vec<_>>(), before);
}

#[test]
fn wanted_capacity_evicts_oldest_request_without_refreshing_duplicates() {
    use baneworkshop::{
        network::sync::{MAX_WAITING, Sync},
        protocol::{Block, params::genesis},
    };
    let mut sync = Sync::default();
    let hashes: Vec<_> = (0..MAX_WAITING as u64)
        .map(|nonce| Block { nonce, ..genesis() }.hash())
        .collect();
    for hash in &hashes {
        sync.want(*hash);
    }
    sync.want(hashes[0]);
    let parent = Bytes([0x42; 32]);
    sync.defer(Block {
        height: 2,
        previous: parent,
        ..genesis()
    });
    sync.want_parent(parent);
    assert_eq!(sync.wanted.len(), MAX_WAITING);
    assert!(sync.wanted.contains(&parent));
    assert!(!sync.wanted.contains(&hashes[0]));
    assert!(sync.wanted.contains(&hashes[1]));
    sync.want(Bytes([0x43; 32]));
    assert!(sync.wanted.contains(&parent));
    assert!(!sync.wanted.contains(&hashes[1]));
    sync.forget(&hashes[2]);
    sync.want(hashes[2]);
    sync.want(Bytes([0x44; 32]));
    assert!(sync.wanted.contains(&hashes[2]));
    assert!(!sync.wanted.contains(&hashes[3]));
}

#[test]
fn retries_rotate_in_batches_and_keep_progress_when_requests_are_removed() {
    use baneworkshop::network::sync::Sync;
    let mut sync = Sync::default();
    let hashes: Vec<_> = (1..=10).map(|n| Bytes([n; 32])).collect();
    for hash in &hashes {
        sync.want(*hash);
    }
    assert_eq!(sync.retry_requests(), hashes[..4]);
    for hash in &hashes[..4] {
        sync.forget(hash);
    }
    assert_eq!(sync.retry_requests(), hashes[4..8]);
    assert_eq!(
        sync.retry_requests(),
        vec![hashes[8], hashes[9], hashes[4], hashes[5]]
    );
    for hash in &hashes {
        sync.forget(hash);
    }
    assert!(sync.retry_requests().is_empty());
    sync.want(hashes[0]);
    assert_eq!(sync.retry_requests(), vec![hashes[0]]);
}

#[test]
fn pending_blocks_are_known_and_do_not_remain_wanted() {
    use baneworkshop::{network::sync::Sync, protocol::params::genesis};
    let mut sync = Sync::default();
    let block = genesis();
    let hash = block.hash();
    sync.want(hash);
    sync.defer(block);
    sync.want(hash);
    sync.want_parent(hash);
    assert!(sync.wanted.is_empty());
    assert!(sync.retry_requests().is_empty());
}
