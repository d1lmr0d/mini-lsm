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

mod leveled;
mod simple_leveled;
mod tiered;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
pub use leveled::{LeveledCompactionController, LeveledCompactionOptions, LeveledCompactionTask};
use serde::{Deserialize, Serialize};
pub use simple_leveled::{
    SimpleLeveledCompactionController, SimpleLeveledCompactionOptions, SimpleLeveledCompactionTask,
};
pub use tiered::{TieredCompactionController, TieredCompactionOptions, TieredCompactionTask};

use crate::iterators::StorageIterator;
use crate::iterators::concat_iterator::SstConcatIterator;
use crate::iterators::merge_iterator::MergeIterator;
use crate::iterators::two_merge_iterator::TwoMergeIterator;
use crate::key::KeySlice;
use crate::lsm_storage::{LsmStorageInner, LsmStorageState};
use crate::table::{SsTable, SsTableBuilder, SsTableIterator};

#[derive(Debug, Serialize, Deserialize)]
pub enum CompactionTask {
    Leveled(LeveledCompactionTask),
    Tiered(TieredCompactionTask),
    Simple(SimpleLeveledCompactionTask),
    ForceFullCompaction {
        l0_sstables: Vec<usize>,
        l1_sstables: Vec<usize>,
    },
}

impl CompactionTask {
    fn compact_to_bottom_level(&self) -> bool {
        match self {
            CompactionTask::ForceFullCompaction { .. } => true,
            CompactionTask::Leveled(task) => task.is_lower_level_bottom_level,
            CompactionTask::Simple(task) => task.is_lower_level_bottom_level,
            CompactionTask::Tiered(task) => task.bottom_tier_included,
        }
    }
}

pub(crate) enum CompactionController {
    Leveled(LeveledCompactionController),
    Tiered(TieredCompactionController),
    Simple(SimpleLeveledCompactionController),
    NoCompaction,
}

impl CompactionController {
    pub fn generate_compaction_task(&self, snapshot: &LsmStorageState) -> Option<CompactionTask> {
        match self {
            CompactionController::Leveled(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Leveled),
            CompactionController::Simple(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Simple),
            CompactionController::Tiered(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Tiered),
            CompactionController::NoCompaction => unreachable!(),
        }
    }

    pub fn apply_compaction_result(
        &self,
        snapshot: &LsmStorageState,
        task: &CompactionTask,
        output: &[usize],
        in_recovery: bool,
    ) -> (LsmStorageState, Vec<usize>) {
        match (self, task) {
            (CompactionController::Leveled(ctrl), CompactionTask::Leveled(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output, in_recovery)
            }
            (CompactionController::Simple(ctrl), CompactionTask::Simple(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output)
            }
            (CompactionController::Tiered(ctrl), CompactionTask::Tiered(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output)
            }
            _ => unreachable!(),
        }
    }
}

impl CompactionController {
    pub fn flush_to_l0(&self) -> bool {
        matches!(
            self,
            Self::Leveled(_) | Self::Simple(_) | Self::NoCompaction
        )
    }
}

#[derive(Debug, Clone)]
pub enum CompactionOptions {
    /// Leveled compaction with partial compaction + dynamic level support (= RocksDB's Leveled
    /// Compaction)
    Leveled(LeveledCompactionOptions),
    /// Tiered compaction (= RocksDB's universal compaction)
    Tiered(TieredCompactionOptions),
    /// Simple leveled compaction
    Simple(SimpleLeveledCompactionOptions),
    /// In no compaction mode (week 1), always flush to L0
    NoCompaction,
}

impl LsmStorageInner {
    fn compact(&self, _task: &CompactionTask) -> Result<Vec<Arc<SsTable>>> {
        match _task {
            CompactionTask::ForceFullCompaction {
                l0_sstables,
                l1_sstables,
            } => {
                let snapshot = {
                    let state = self.state.read();
                    Arc::clone(&state)
                };
                let mut l0_iters = Vec::new();
                for l0_sstable in snapshot.l0_sstables.iter() {
                    let l0_sst = snapshot.sstables[l0_sstable].clone();
                    let l0_iter = SsTableIterator::create_and_seek_to_first(l0_sst)?;
                    l0_iters.push(Box::new(l0_iter));
                }
                let l0_iter = MergeIterator::create(l0_iters);
                let mut l1_iters = Vec::new();
                for l1_sstable in snapshot.levels[0].1.iter() {
                    let l1_sst = snapshot.sstables[l1_sstable].clone();
                    l1_iters.push(l1_sst);
                }
                let l1_iter = SstConcatIterator::create_and_seek_to_first(l1_iters)?;
                let two_merge_iter = TwoMergeIterator::create(l0_iter, l1_iter)?;
                if _task.compact_to_bottom_level() {
                    self.compact_from_iter_to_bottom_level(two_merge_iter)
                } else {
                    self.compact_from_iter(two_merge_iter)
                }
            }
            _ => unimplemented!(),
        }
    }

    fn compact_from_iter(
        &self,
        mut iter: impl for<'a> StorageIterator<KeyType<'a> = KeySlice<'a>>,
    ) -> Result<Vec<Arc<SsTable>>> {
        let mut new_sst = Vec::new();
        let mut sst_builder: Option<SsTableBuilder> = None;
        while iter.is_valid() {
            if sst_builder.is_none() {
                sst_builder = Some(SsTableBuilder::new(self.options.block_size));
            }
            let sst_builder_inner = sst_builder.as_mut().unwrap();
            sst_builder_inner.add(iter.key(), iter.value());
            iter.next()?;
            if sst_builder_inner.estimated_size() >= self.options.target_sst_size {
                let id = self.next_sst_id();
                let path = self.path_of_sst(id);
                let builder = sst_builder.take().unwrap();
                let sst = Arc::new(builder.build(id, Some(self.block_cache.clone()), path)?);
                new_sst.push(sst);
            }
        }
        if let Some(builder) = sst_builder {
            let id = self.next_sst_id();
            let path = self.path_of_sst(id);
            let sst = Arc::new(builder.build(id, Some(self.block_cache.clone()), path)?);
            new_sst.push(sst);
        }
        Ok(new_sst)
    }

    fn compact_from_iter_to_bottom_level(
        &self,
        mut iter: impl for<'a> StorageIterator<KeyType<'a> = KeySlice<'a>>,
    ) -> Result<Vec<Arc<SsTable>>> {
        let mut new_sst = Vec::new();
        let mut sst_builder: Option<SsTableBuilder> = None;
        while iter.is_valid() {
            if iter.value().is_empty() {
                iter.next()?;
                continue;
            }
            if sst_builder.is_none() {
                sst_builder = Some(SsTableBuilder::new(self.options.block_size));
            }
            let sst_builder_inner = sst_builder.as_mut().unwrap();
            sst_builder_inner.add(iter.key(), iter.value());
            iter.next()?;
            if sst_builder_inner.estimated_size() >= self.options.target_sst_size {
                let id = self.next_sst_id();
                let path = self.path_of_sst(id);
                let builder = sst_builder.take().unwrap();
                let sst = Arc::new(builder.build(id, Some(self.block_cache.clone()), path)?);
                new_sst.push(sst);
            }
        }
        if let Some(builder) = sst_builder {
            let id = self.next_sst_id();
            let path = self.path_of_sst(id);
            let sst = Arc::new(builder.build(id, Some(self.block_cache.clone()), path)?);
            new_sst.push(sst);
        }
        Ok(new_sst)
    }

    pub fn force_full_compaction(&self) -> Result<()> {
        let snapshot = {
            let state = self.state.read();
            Arc::clone(&state)
        };
        let l0_sstables = snapshot.l0_sstables.clone();
        let l1_sstables = snapshot.levels[0].1.clone();
        let compaction_task = CompactionTask::ForceFullCompaction {
            l0_sstables: l0_sstables.clone(),
            l1_sstables: l1_sstables.clone(),
        };
        let new_sstables = self.compact(&compaction_task)?;
        let new_sst_ids: Vec<usize> = new_sstables.iter().map(|sst| sst.sst_id()).collect();
        let l0_set: HashSet<usize> = l0_sstables.iter().cloned().collect();
        {
            let state_lock = self.state_lock.lock();
            let mut new_state = snapshot.as_ref().clone();
            for sst in l0_sstables.iter().chain(l1_sstables.iter()) {
                let result = new_state.sstables.remove(sst);
                assert!(result.is_some());
            }
            for sst in &new_sstables {
                let result = new_state.sstables.insert(sst.sst_id(), sst.clone());
                assert!(result.is_none());
            }
            new_state.l0_sstables = new_state
                .l0_sstables
                .iter()
                .filter(|id| !l0_set.contains(id))
                .cloned()
                .collect();
            new_state.levels[0].1 = new_sst_ids;
            *self.state.write() = Arc::new(new_state);
        }
        for sst in l0_sstables.iter().chain(l1_sstables.iter()) {
            std::fs::remove_file(self.path_of_sst(*sst))?;
        }
        Ok(())
    }

    fn trigger_compaction(&self) -> Result<()> {
        unimplemented!()
    }

    pub(crate) fn spawn_compaction_thread(
        self: &Arc<Self>,
        rx: crossbeam_channel::Receiver<()>,
    ) -> Result<Option<std::thread::JoinHandle<()>>> {
        if let CompactionOptions::Leveled(_)
        | CompactionOptions::Simple(_)
        | CompactionOptions::Tiered(_) = self.options.compaction_options
        {
            let this = self.clone();
            let handle = std::thread::spawn(move || {
                let ticker = crossbeam_channel::tick(Duration::from_millis(50));
                loop {
                    crossbeam_channel::select! {
                        recv(ticker) -> _ => if let Err(e) = this.trigger_compaction() {
                            eprintln!("compaction failed: {}", e);
                        },
                        recv(rx) -> _ => return
                    }
                }
            });
            return Ok(Some(handle));
        }
        Ok(None)
    }

    fn trigger_flush(&self) -> Result<()> {
        let state = self.state.read();
        if state.imm_memtables.len() + 1 > self.options.num_memtable_limit {
            drop(state);
            self.force_flush_next_imm_memtable()?;
        }
        Ok(())
    }

    pub(crate) fn spawn_flush_thread(
        self: &Arc<Self>,
        rx: crossbeam_channel::Receiver<()>,
    ) -> Result<Option<std::thread::JoinHandle<()>>> {
        let this = self.clone();
        let handle = std::thread::spawn(move || {
            let ticker = crossbeam_channel::tick(Duration::from_millis(50));
            loop {
                crossbeam_channel::select! {
                    recv(ticker) -> _ => if let Err(e) = this.trigger_flush() {
                        eprintln!("flush failed: {}", e);
                    },
                    recv(rx) -> _ => return
                }
            }
        });
        Ok(Some(handle))
    }
}
