use crate::protocol::{Block, Hash, PublicKey, Transaction};
use serde::{Deserialize, Serialize};
pub const VERSION: u32 = 1;
pub const MAX_LINE: usize = 262_144;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Hello {
        version: u32,
        genesis: Hash,
        identity: PublicKey,
    },
    GetTip {},
    Tip {
        hash: Hash,
        height: u64,
    },
    GetBlock {
        hash: Hash,
    },
    Block {
        block: Block,
    },
    Transaction {
        transaction: Transaction,
    },
}
/// Bounded incremental framing, including partial reads and coalesced messages.
#[derive(Default)]
pub struct Framer {
    buffer: Vec<u8>,
}
impl Framer {
    pub fn push(&mut self, bytes: &[u8]) -> crate::Result<Vec<Message>> {
        let mut messages = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let message = serde_json::from_slice(&self.buffer)?;
                self.buffer.clear();
                messages.push(message);
            } else {
                if self.buffer.len() == MAX_LINE {
                    return Err(crate::Error::Invalid("wire line too long"));
                }
                self.buffer.push(byte);
            }
        }
        Ok(messages)
    }
}
