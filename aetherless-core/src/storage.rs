// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Ephemeral in-memory key-value storage.
//!
//! Provides a simple thread-safe storage for functions to share state.
//! In a real distributed system, this would be backed by a consensus algorithm
//! or a distributed hash table. For now, it's local to the node, but
//! capable of being synced via gossip (future).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct Storage {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn put(&self, key: String, value: Vec<u8>) {
        let mut lock = self.data.write().unwrap();
        lock.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let lock = self.data.read().unwrap();
        lock.get(key).cloned()
    }

    pub fn delete(&self, key: &str) {
        let mut lock = self.data.write().unwrap();
        lock.remove(key);
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
