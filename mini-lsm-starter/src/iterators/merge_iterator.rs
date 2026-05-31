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

use std::cmp::{self};
use std::collections::BinaryHeap;
use std::collections::binary_heap::PeekMut;

use anyhow::Result;

use crate::key::KeySlice;

use super::StorageIterator;

struct HeapWrapper<I: StorageIterator>(pub usize, pub Box<I>);

impl<I: StorageIterator> PartialEq for HeapWrapper<I> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == cmp::Ordering::Equal
    }
}

impl<I: StorageIterator> Eq for HeapWrapper<I> {}

impl<I: StorageIterator> PartialOrd for HeapWrapper<I> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<I: StorageIterator> Ord for HeapWrapper<I> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.1
            .key()
            .cmp(&other.1.key())
            .then(self.0.cmp(&other.0))
            .reverse()
    }
}

/// Merge multiple iterators of the same type. If the same key occurs multiple times in some
/// iterators, prefer the one with smaller index.
pub struct MergeIterator<I: StorageIterator> {
    iters: BinaryHeap<HeapWrapper<I>>,
    current: Option<HeapWrapper<I>>,
}

impl<I: StorageIterator> MergeIterator<I> {
    pub fn create(iters: Vec<Box<I>>) -> Self {
        if iters.is_empty() {
            return Self {
                iters: BinaryHeap::new(),
                current: None,
            };
        }
        let mut heap = BinaryHeap::new();
        for (i, iter) in iters.into_iter().enumerate() {
            if iter.is_valid() {
                heap.push(HeapWrapper(i, iter));
            }
        }
        let current = match heap.pop() {
            Some(c) => c,
            None => {
                return Self {
                    iters: BinaryHeap::new(),
                    current: None,
                };
            }
        };
        Self {
            iters: heap,
            current: Some(current),
        }
    }
}

impl<I: 'static + for<'a> StorageIterator<KeyType<'a> = KeySlice<'a>>> StorageIterator
    for MergeIterator<I>
{
    type KeyType<'a> = KeySlice<'a>;

    fn key(&self) -> KeySlice<'_> {
        let current = self.current.as_ref().unwrap();
        current.1.key()
    }

    fn value(&self) -> &[u8] {
        let current = self.current.as_ref().unwrap();
        current.1.value()
    }

    fn is_valid(&self) -> bool {
        match self.current.as_ref() {
            Some(c) => c.1.is_valid(),
            None => false,
        }
    }

    fn next(&mut self) -> Result<()> {
        if self.current.is_none() {
            return Ok(());
        }
        if let Some(mut current) = self.current.take() {
            while let Some(mut inner_iter) = self.iters.peek_mut() {
                if current.1.key() != inner_iter.1.key() {
                    break;
                }
                if let err @ Err(_) = inner_iter.1.next() {
                    PeekMut::pop(inner_iter);
                    return err;
                }
                if !inner_iter.1.is_valid() {
                    PeekMut::pop(inner_iter);
                }
            }
            current.1.next()?;
            if current.1.is_valid() {
                self.iters.push(current);
            }
            self.current = self.iters.pop();
        }
        Ok(())
    }

    fn num_active_iterators(&self) -> usize {
        let current_num = self
            .current
            .as_ref()
            .map_or(0, |c| c.1.num_active_iterators());
        let iters_num = self
            .iters
            .iter()
            .fold(0, |acc, i| acc + i.1.num_active_iterators());
        current_num + iters_num
    }
}
