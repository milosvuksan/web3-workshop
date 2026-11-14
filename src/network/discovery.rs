//! Optional, non-consensus LAN discovery extension from NETWORK.md.
use crate::{
    crypto,
    protocol::{Hash, PublicKey, params::genesis},
};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

#[derive(Clone, Copy)]
pub struct Discovery {
    pub port: u16,
    pub identity: PublicKey,
}
impl Discovery {
    pub fn bind(self) -> std::io::Result<UdpSocket> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&SocketAddr::from(([0, 0, 0, 0], self.port)).into())?;
        UdpSocket::from_std(socket.into())
    }
    pub fn announcement(self, tcp_port: u16) -> String {
        format!(
            "PIXELCHAIN v1 {} {tcp_port} {}",
            genesis().hash(),
            self.identity
        )
    }
    pub fn parse(self, bytes: &[u8], source: SocketAddr) -> Option<(PublicKey, SocketAddr)> {
        let text = std::str::from_utf8(bytes).ok()?;
        let words: Vec<_> = text.split(' ').collect();
        if words.len() != 5 || words[0] != "PIXELCHAIN" || words[1] != "v1" {
            return None;
        }
        let chain: Hash = words[2].parse().ok()?;
        if chain != genesis().hash() || !words[3].bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let port = words[3].parse::<std::num::NonZeroU16>().ok()?.get();
        let identity: PublicKey = words[4].parse().ok()?;
        if identity == self.identity || crypto::valid_key(identity).is_err() {
            return None;
        }
        Some((identity, SocketAddr::new(source.ip(), port)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    #[test]
    fn packets_validate_chain_key_and_port_and_use_source_ip() {
        let local = Discovery {
            port: 7000,
            identity: crypto::public_key(&SigningKey::from_bytes(&[1; 32])),
        };
        let remote = Discovery {
            identity: crypto::public_key(&SigningKey::from_bytes(&[2; 32])),
            ..local
        };
        let source = "192.0.2.3:9999".parse().unwrap();
        let packet = remote.announcement(7002);
        assert_eq!(
            local.parse(packet.as_bytes(), source),
            Some((remote.identity, "192.0.2.3:7002".parse().unwrap()))
        );
        for bad in [
            local.announcement(7001),
            remote.announcement(0),
            packet.replace("7002", "65536"),
            packet.replace("7002", "+7002"),
            packet.replace("v1", "v2"),
            packet.replace(&genesis().hash().to_string(), &"0".repeat(64)),
            packet.replace(&remote.identity.to_string(), &"0".repeat(64)),
            format!("{packet}\n"),
            format!("{packet} extra"),
            packet.replace(' ', "  "),
        ] {
            assert!(
                local.parse(bad.as_bytes(), source).is_none(),
                "accepted {bad}"
            );
        }
    }
}
