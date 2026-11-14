use super::{Block, Bytes};
pub const WIDTH: usize = 32;
pub const HEIGHT: usize = 32;
pub const PARCELS: usize = WIDTH * HEIGHT;
pub const DIFFICULTY: u32 = 23;
pub const MAX_TRANSACTIONS: usize = 256;
pub const MAX_FUTURE_SECONDS: u64 = 120;
pub const TX_DOMAIN: &[u8] = b"PIXEL-TX-v1\0";
pub const ID_DOMAIN: &[u8] = b"PIXEL-ID-v1\0";
pub const BLOCK_DOMAIN: &[u8] = b"PIXEL-BLOCK-v1\0";
pub fn genesis() -> Block {
    Block { previous: Bytes::default(), height: 0, miner: Bytes::default(), timestamp: 0, nonce: 0, transactions: vec![] }
}
