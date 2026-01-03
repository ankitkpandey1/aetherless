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
    },
}

#[derive(Debug, Clone)]
pub struct PeerNode {
    pub id: String,
    pub rpc_addr: String, // IP:Port for HTTP/RPC
    pub last_seen: u64,
}

pub struct ClusterManager {
    node_id: String,
    bind_addr: String,
    peers: Arc<Mutex<HashMap<String, PeerNode>>>,
    socket: Arc<UdpSocket>,
}

impl ClusterManager {
    pub async fn new(bind_addr: &str, node_id: &str) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.set_broadcast(true)?; // Enable if needed, but mostly we unicast to known peers
        
        Ok(Self {
            node_id: node_id.to_string(),
            bind_addr: bind_addr.to_string(),
            peers: Arc::new(Mutex::new(HashMap::new())),
            socket: Arc::new(socket),
        })
    }

    /// Start the gossip loop
    pub async fn start(self: Arc<Self>, seeds: Vec<String>) {
        tracing::info!("Starting cluster manager on {}", self.bind_addr);
        
        // Seed logic: Send Hello to seeds
        for seed in seeds {
             self.send_to(GossipMessage::Hello {
                 node_id: self.node_id.clone(),
                 rpc_addr: self.bind_addr.clone(), // Simplified: use same addr for now
             }, &seed).await;
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
        let mut buf = [0u8; 1024];
        loop {
            if let Ok((len, addr)) = self.socket.recv_from(&mut buf).await {
                if let Ok(msg) = serde_json::from_slice::<GossipMessage>(&buf[..len]) {
                    self.handle_message(msg, addr).await;
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
                peers.insert(node_id, PeerNode {
                    id: String::new(), // set below
                    rpc_addr, // Should validate
                    last_seen: now,
                });
                // Send ack or Hello back?
            }
            GossipMessage::Heartbeat { node_id, .. } => {
                if let Some(peer) = peers.get_mut(&node_id) {
                    peer.last_seen = now;
                }
            }
            GossipMessage::Goodbye { node_id } => {
                tracing::info!("Node left: {}", node_id);
                peers.remove(&node_id);
            }
        }
    }

    async fn heartbeat_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            
            // Send heartbeat to all peers (or random subset)
            // Need peer list
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
                // If target is effectively us, skip?
                self.send_to(msg.clone(), &target).await;
            }
        }
    }

    async fn send_to(&self, msg: GossipMessage, addr: &str) {
        if let Ok(data) = serde_json::to_vec(&msg) {
             let _ = self.socket.send_to(&data, addr).await;
        }
    }
}
