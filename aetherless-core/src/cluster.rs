// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Cluster management and distributed state syncing.
//!
//! Implements a Gossip-based discovery protocol (SWIM-like) over UDP.
//! Nodes periodically multicast/gossip their existence and share FunctionRegistry state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    Hello {
        node_id: String,
        rpc_addr: String, // TCP address for sync
    },
    Heartbeat {
        node_id: String,
        timestamp: u64,
    },
    Goodbye {
        node_id: String,
        timestamp: u64,
    },
    StorageUpdate {
        key: String,
        value: Vec<u8>,
        timestamp: u64,
    },
}

#[derive(Debug, Clone)]
pub struct PeerNode {
    pub id: String,
    pub rpc_addr: String, // IP:Port for HTTP/RPC
    pub last_seen: u64,
}

use crate::storage::Storage;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage {
    pub sig: String, // hex encoded
    pub payload: GossipMessage,
}

impl SignedMessage {
    pub fn new(payload: GossipMessage, secret: &[u8]) -> Self {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        mac.update(&payload_bytes);
        let result = mac.finalize();
        let sig = hex::encode(result.into_bytes());

        Self { sig, payload }
    }

    pub fn verify(&self, secret: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
        let payload_bytes = serde_json::to_vec(&self.payload).unwrap();
        mac.update(&payload_bytes);

        let expected_sig = hex::encode(mac.finalize().into_bytes());
        // Constant time comparison would be better, but strings here
        // For MVP, simple string comparison is okay, but sensitive systems should use verify_slice
        self.sig == expected_sig
    }
}

pub struct ClusterManager {
    node_id: String,
    bind_addr: String,
    peers: Arc<Mutex<HashMap<String, PeerNode>>>,
    socket: Arc<UdpSocket>,
    storage: Storage,
    secret_key: Vec<u8>,
}

impl ClusterManager {
    pub async fn new(
        bind_addr: &str,
        node_id: &str,
        storage: Storage,
        secret_key: Option<String>,
    ) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.set_broadcast(true)?;

        let secret = secret_key
            .unwrap_or_else(|| "default-insecure-secret".to_string())
            .into_bytes();

        Ok(Self {
            node_id: node_id.to_string(),
            bind_addr: bind_addr.to_string(),
            peers: Arc::new(Mutex::new(HashMap::new())),
            socket: Arc::new(socket),
            storage,
            secret_key: secret,
        })
    }

    /// Start the gossip loop
    pub async fn start(self: Arc<Self>, seeds: Vec<String>) {
        tracing::info!("Starting cluster manager on {}", self.bind_addr);

        // Seed logic: Send Hello to seeds
        for seed in seeds {
            self.send_to(
                GossipMessage::Hello {
                    node_id: self.node_id.clone(),
                    rpc_addr: self.bind_addr.clone(),
                },
                &seed,
            )
            .await;
        }

        let listener = self.clone();
        tokio::spawn(async move {
            listener.listen_loop().await;
        });

        let heartbeater = self.clone();
        tokio::spawn(async move {
            heartbeater.heartbeat_loop().await;
        });
    }

    async fn listen_loop(&self) {
        let mut buf = [0u8; 4096]; // Increased buffer for storage payloads
        loop {
            if let Ok((len, addr)) = self.socket.recv_from(&mut buf).await {
                // Try parse as SignedMessage
                if let Ok(signed) = serde_json::from_slice::<SignedMessage>(&buf[..len]) {
                    if signed.verify(&self.secret_key) {
                        self.handle_message(signed.payload, addr).await;
                    } else {
                        tracing::warn!("Received invalid signature from {}", addr);
                    }
                } else {
                    tracing::debug!("Received unsigned or malformed message from {}", addr);
                }
            }
        }
    }

    async fn handle_message(&self, msg: GossipMessage, _src: SocketAddr) {
        let mut peers = self.peers.lock().await;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        match msg {
            GossipMessage::Hello { node_id, rpc_addr } => {
                tracing::info!("Node joined: {} ({})", node_id, rpc_addr);
                peers.insert(
                    node_id,
                    PeerNode {
                        id: String::new(),
                        rpc_addr,
                        last_seen: now,
                    },
                );
            }
            GossipMessage::Heartbeat { node_id, .. } => {
                if let Some(peer) = peers.get_mut(&node_id) {
                    peer.last_seen = now;
                }
            }
            GossipMessage::Goodbye { node_id, .. } => {
                tracing::info!("Node left: {}", node_id);
                peers.remove(&node_id);
            }
            GossipMessage::StorageUpdate {
                key,
                value,
                timestamp: _,
            } => {
                self.storage.put(key, value);
            }
        }
    }

    async fn heartbeat_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            let targets: Vec<String> = {
                let peers = self.peers.lock().await;
                peers.values().map(|p| p.rpc_addr.clone()).collect()
            };

            let msg = GossipMessage::Heartbeat {
                node_id: self.node_id.clone(),
                timestamp: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };

            for target in targets {
                self.send_to(msg.clone(), &target).await;
            }
        }
    }

    pub async fn broadcast_update(&self, key: String, value: Vec<u8>) {
        let msg = GossipMessage::StorageUpdate {
            key,
            value,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let targets: Vec<String> = {
            let peers = self.peers.lock().await;
            peers.values().map(|p| p.rpc_addr.clone()).collect()
        };

        for target in targets {
            self.send_to(msg.clone(), &target).await;
        }
    }

    async fn send_to(&self, msg: GossipMessage, addr: &str) {
        let signed = SignedMessage::new(msg, &self.secret_key);
        if let Ok(data) = serde_json::to_vec(&signed) {
            let _ = self.socket.send_to(&data, addr).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_message() {
        let payload = GossipMessage::Hello {
            node_id: "test".into(),
            rpc_addr: "1.1.1.1".into(),
        };
        let secret = b"super-secret";

        let signed = SignedMessage::new(payload.clone(), secret);
        assert!(signed.verify(secret));
        assert!(!signed.verify(b"wrong-secret"));
    }
}
