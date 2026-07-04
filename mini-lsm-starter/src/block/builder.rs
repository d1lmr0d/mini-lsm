// Copyright (c) 2022-2025 Alex Chi Z
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(unused_variables)] // TODO(you): remove this lint after implementing this mod
#![allow(dead_code)] // TODO(you): remove this lint after implementing this mod

use bytes::BufMut;

use crate::key::{KeySlice, KeyVec};

use super::Block;

/// Builds a block.
pub struct BlockBuilder {
    /// Offsets of each key-value entries.
    offsets: Vec<u16>,
    /// All serialized key-value pairs in the block.
    data: Vec<u8>,
    /// The expected block size.
    block_size: usize,
    /// The first key in the block
    first_key: KeyVec,
}

impl BlockBuilder {
    /// Creates a new block builder.
    pub fn new(block_size: usize) -> Self {
        Self {
            offsets: Vec::new(),
            data: Vec::new(),
            block_size,
            first_key: KeyVec::new(),
        }
    }

    /// Adds a key-value pair to the block. Returns false when the block is full.
    /// You may find the `bytes::BufMut` trait useful for manipulating binary data.
    #[must_use]
    pub fn add(&mut self, key: KeySlice, value: &[u8]) -> bool {
        let is_first = self.is_empty();
        let entry_size = if is_first {
            key.len() + 2 + value.len() + 2
        } else {
            let overlap_len = key.common_prefix(self.first_key.as_key_slice());
            let rest_len = key.len() - overlap_len;
            rest_len + 4 + value.len() + 2
        };
        let current_block_size = self.data.len() + self.offsets.len() * 2;
        let expected_block_size = current_block_size + entry_size + 2;
        if !is_first {
            if expected_block_size > self.block_size {
                return false;
            }
        }
        self.offsets.push(self.data.len() as u16);
        if is_first {
            self.data.put_u16_le(key.len() as u16);
            self.data.put_slice(key.into_inner());
            self.first_key.set_from_slice(key);
        } else {
            let overlap_len = key.common_prefix(self.first_key.as_key_slice());
            let rest_len = key.len() - overlap_len;
            self.data.put_u16_le(overlap_len as u16);
            self.data.put_u16_le(rest_len as u16);
            self.data.put_slice(&key.into_inner()[overlap_len..]);
        }
        self.data.put_u16_le(value.len() as u16);
        self.data.put_slice(value);
        true
    }

    /// Check if there is no key-value pair in the block.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.offsets.is_empty()
    }

    /// Finalize the block.
    pub fn build(self) -> Block {
        Block {
            data: self.data,
            offsets: self.offsets,
        }
    }
}
