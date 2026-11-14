use baneworkshop::{
    network::{
        lag::Lag,
        wire::{Framer, MAX_LINE, Message},
    },
    protocol::Bytes,
};
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
    assert_eq!(sync.wanted.len(), MAX_WAITING);
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
fn needed_ancestor_displaces_speculative_requests_at_capacity() {
    use baneworkshop::{
        network::sync::{MAX_WAITING, Sync},
        protocol::{Block, params::genesis},
    };
    let mut sync = Sync::default();
    for nonce in 0..MAX_WAITING as u64 {
        sync.want(Block { nonce, ..genesis() }.hash());
    }
    let parent = Bytes([0x42; 32]);
    sync.defer(Block {
        height: 2,
        previous: parent,
        ..genesis()
    });
    sync.want_parent(parent);
    assert_eq!(sync.wanted.len(), MAX_WAITING);
    assert!(sync.wanted.contains(&parent));
    // Speculative tips cannot evict the required ancestor.
    sync.want(Bytes([0x43; 32]));
    assert!(sync.wanted.contains(&parent));
}
