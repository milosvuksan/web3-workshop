//! Compatibility profiles have their own consensus state: signed payloads and
//! proof of work cannot be translated between chains with different rules.
pub mod grupa1;
pub mod http;

use crate::settings::SavedPeer;
use grupa1::Ledger;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct Compatibility {
    pub ledger: Mutex<Ledger>,
    pub peers: Mutex<BTreeMap<SocketAddr, PeerStatus>>,
}
#[derive(Clone, Default)]
pub struct PeerStatus {
    pub connected: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Config {
    pub state: Arc<Compatibility>,
    pub peers: Vec<SavedPeer>,
}
