use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

/// Fixed-width bytes. Text boundaries accept exactly lowercase hexadecimal.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bytes<const N: usize>(pub [u8; N]);
impl<const N: usize> Default for Bytes<N> {
    fn default() -> Self {
        Self([0; N])
    }
}
impl<const N: usize> fmt::Display for Bytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}
impl<const N: usize> fmt::Debug for Bytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl<const N: usize> FromStr for Bytes<N> {
    type Err = crate::Error;
    fn from_str(s: &str) -> crate::Result<Self> {
        if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(crate::Error::Invalid("expected fixed-width lowercase hex"));
        }
        let mut bytes = [0; N];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| crate::Error::Invalid("invalid hex"))?;
        Ok(Self(bytes))
    }
}
impl<const N: usize> Serialize for Bytes<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de, const N: usize> Deserialize<'de> for Bytes<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
    }
}
pub type Hash = Bytes<32>;
pub type PublicKey = Bytes<32>;
pub type Signature = Bytes<64>;
pub type Color = Bytes<3>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Paint { color: Color },
    Transfer { recipient: PublicKey },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub parcel: u16,
    pub counter: u64,
    pub sender: PublicKey,
    pub action: Action,
    pub signature: Signature,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub previous: Hash,
    pub height: u64,
    pub miner: PublicKey,
    pub timestamp: u64,
    pub nonce: u64,
    pub transactions: Vec<Transaction>,
}
