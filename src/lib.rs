pub mod chain;
pub mod cli;
pub mod config;
pub mod crypto;
pub mod mempool;
pub mod mining;
pub mod network;
pub mod node;
pub mod protocol;
pub mod render;
pub mod state;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("unknown parent {0}")]
    UnknownParent(protocol::Hash),
    #[error("block timestamp is too far in the future")]
    Future,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
