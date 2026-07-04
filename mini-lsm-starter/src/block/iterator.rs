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

use std::sync::Arc;

use crate::key::{KeySlice, KeyVec};

use super::Block;

const LEN_SIZE: usize = 2;

/// Iterates on a block.
pub struct BlockIterator {
    /// The internal `Block`, wrapped by an `Arc`
    block: Arc<Block>,
    /// The current key, empty represents the iterator is invalid
    key: KeyVec,
    /// the current value range in the block.data, corresponds to the current key
    value_range: (usize, usize),
    /// Current index of the key-value pair, should be in range of [0, num_of_elements)
    idx: usize,
    /// The first key in the block
    first_key: KeyVec,
}

impl BlockIterator {
    fn new(block: Arc<Block>) -> Self {
        let mut first_key = KeyVec::new();
        if !block.offsets.is_empty() {
            let (key, _, _) = Self::decode_key_at(0, &block, &first_key);
            first_key = key;
        }
        Self {
            block,
            key: KeyVec::new(),
            value_range: (0, 0),
            idx: 0,
            first_key,
        }
    }

    /// Creates a block iterator and seek to the first entry.
    pub fn create_and_seek_to_first(block: Arc<Block>) -> Self {
        let mut iter = BlockIterator::new(block);
        iter.seek_to_first();
        iter
    }

    /// Creates a block iterator and seek to the first key that >= `key`.
    pub fn create_and_seek_to_key(block: Arc<Block>, key: KeySlice) -> Self {
        let mut iter = BlockIterator::new(block);
        iter.seek_to_key(key);
        iter
    }

    /// Returns the key of the current entry.
    pub fn key(&self) -> KeySlice<'_> {
        self.key.as_key_slice()
    }

    /// Returns the value of the current entry.
    pub fn value(&self) -> &[u8] {
        &self.block.data[self.value_range.0..self.value_range.1]
    }

    /// Returns true if the iterator is valid.
    /// Note: You may want to make use of `key`
    pub fn is_valid(&self) -> bool {
        !self.key().is_empty()
    }

    /// Seeks to the first key in the block.
    pub fn seek_to_first(&mut self) {
        self.idx = 0;
        self.update_current_pair();
    }

    /// Move to the next key in the block.
    pub fn next(&mut self) {
        self.idx += 1;
        self.update_current_pair();
    }

    fn decode_key_at(idx: usize, block: &Block, first_key: &KeyVec) -> (KeyVec, usize, usize) {
        let offset = block.offsets[idx] as usize;
        if idx == 0 {
            let key_len = u16::from_le_bytes([block.data[offset], block.data[offset + 1]]) as usize;
            let key = KeyVec::from_vec(
                block.data[offset + LEN_SIZE..offset + LEN_SIZE + key_len].to_vec(),
            );
            let value_len = u16::from_le_bytes([
                block.data[offset + LEN_SIZE + key_len],
                block.data[offset + LEN_SIZE + key_len + 1],
            ]) as usize;
            let value_start = offset + LEN_SIZE + key_len + LEN_SIZE;
            (key, value_start, value_start + value_len)
        } else {
            let overlap_len =
                u16::from_le_bytes([block.data[offset], block.data[offset + 1]]) as usize;
            let rest_len = u16::from_le_bytes([
                block.data[offset + LEN_SIZE],
                block.data[offset + LEN_SIZE + 1],
            ]) as usize;
            let rest_offset = offset + LEN_SIZE * 2;
            let mut key = KeyVec::from_vec(first_key.raw_ref()[..overlap_len].to_vec());
            key.append(&block.data[rest_offset..rest_offset + rest_len]);
            let value_len = u16::from_le_bytes([
                block.data[offset + LEN_SIZE * 2 + rest_len],
                block.data[offset + LEN_SIZE * 2 + rest_len + 1],
            ]) as usize;
            let value_start = offset + LEN_SIZE * 2 + rest_len + LEN_SIZE;
            (key, value_start, value_start + value_len)
        }
    }

    fn update_current_pair(&mut self) {
        if self.idx >= self.block.offsets.len() {
            self.key.clear();
            self.value_range = (0, 0);
            return;
        }
        let (key, value_start, value_end) =
            Self::decode_key_at(self.idx, &self.block, &self.first_key);
        self.key = key;
        self.value_range = (value_start, value_end);
        if self.idx == 0 {
            self.first_key = self.key.clone();
        }
    }

    /// Seek to the first key that >= `key`.
    /// Note: You should assume the key-value pairs in the block are sorted when being added by
    /// callers.
    pub fn seek_to_key(&mut self, key: KeySlice) {
        let mut left = 0;
        let mut right = self.block.offsets.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let (mid_key, _, _) = Self::decode_key_at(mid, &self.block, &self.first_key);
            if mid_key.as_key_slice() < key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        self.idx = left;
        self.update_current_pair();
    }
}
