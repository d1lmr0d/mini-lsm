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

use anyhow::Result;

use super::StorageIterator;

enum Current {
    A,
    B,
}

/// Merges two iterators of different types into one. If the two iterators have the same key, only
/// produce the key once and prefer the entry from A.
pub struct TwoMergeIterator<A: StorageIterator, B: StorageIterator> {
    a: A,
    b: B,
    current: Option<Current>,
}

impl<
    A: 'static + StorageIterator,
    B: 'static + for<'a> StorageIterator<KeyType<'a> = A::KeyType<'a>>,
> TwoMergeIterator<A, B>
{
    pub fn create(a: A, b: B) -> Result<Self> {
        let mut iter = Self {
            a,
            b,
            current: None,
        };
        iter.skip_duplicates()?;
        iter.current = iter.match_current();
        Ok(iter)
    }

    fn match_current(&self) -> Option<Current> {
        match (self.a.is_valid(), self.b.is_valid()) {
            (false, false) => None,
            (true, false) => Some(Current::A),
            (false, true) => Some(Current::B),
            (true, true) => {
                if self.a.key() <= self.b.key() {
                    Some(Current::A)
                } else {
                    Some(Current::B)
                }
            }
        }
    }

    fn skip_duplicates(&mut self) -> Result<()> {
        while self.a.is_valid() && self.b.is_valid() && self.a.key() == self.b.key() {
            self.b.next()?;
        }
        Ok(())
    }
}

impl<
    A: 'static + StorageIterator,
    B: 'static + for<'a> StorageIterator<KeyType<'a> = A::KeyType<'a>>,
> StorageIterator for TwoMergeIterator<A, B>
{
    type KeyType<'a> = A::KeyType<'a>;

    fn key(&self) -> Self::KeyType<'_> {
        match self.current.as_ref().unwrap() {
            Current::A => self.a.key(),
            Current::B => self.b.key(),
        }
    }

    fn value(&self) -> &[u8] {
        match self.current.as_ref().unwrap() {
            Current::A => self.a.value(),
            Current::B => self.b.value(),
        }
    }

    fn is_valid(&self) -> bool {
        self.a.is_valid() || self.b.is_valid()
    }

    fn next(&mut self) -> Result<()> {
        let current = match self.current.take() {
            Some(c) => c,
            None => return Ok(()),
        };
        match current {
            Current::A => {
                self.a.next()?;
            }
            Current::B => {
                self.b.next()?;
            }
        }
        self.skip_duplicates()?;
        self.current = self.match_current();
        Ok(())
    }

    fn num_active_iterators(&self) -> usize {
        self.a.num_active_iterators() + self.b.num_active_iterators()
    }
}
