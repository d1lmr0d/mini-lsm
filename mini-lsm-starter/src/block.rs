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

mod builder;
mod iterator;

pub use builder::BlockBuilder;
use bytes::Bytes;
pub use iterator::BlockIterator;

/// A block is the smallest unit of read and caching in LSM tree. It is a collection of sorted key-value pairs.
pub struct Block {
    pub(crate) data: Vec<u8>,
    pub(crate) offsets: Vec<u16>,
}

impl Block {
    /// Encode the internal data to the data layout illustrated in the course
    /// Note: You may want to recheck if any of the expected field is missing from your output
    pub fn encode(&self) -> Bytes {
        let mut buf = Vec::new();
        let num_entries = self.offsets.len() as u16;
        buf.extend_from_slice(&self.data);
        for offset in &self.offsets {
            buf.extend_from_slice(&offset.to_le_bytes());
        }
        buf.extend_from_slice(&num_entries.to_le_bytes());
        Bytes::from(buf)
    }

    /// Decode from the data layout, transform the input `data` to a single `Block`
    pub fn decode(data: &[u8]) -> Self {
        let num_entries = u16::from_le_bytes([data[data.len() - 2], data[data.len() - 1]]) as usize;
        let offsets_end = data.len() - 2;
        let offsets_start = offsets_end - num_entries * 2;
        let offsets: Vec<u16> = data[offsets_start..offsets_end]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let entries = data[..offsets_start].to_vec();
        Self {
            data: entries,
            offsets,
        }
    }
}
