//! Workspace segment metadata.
//!
//! The mutable graph can now expose a manifest-shaped view of per-file segments.
//! The current implementation records a stable u64 digest for each file source
//! and the live nodes owned by that file. A later mmap writer can consume the
//! same descriptors.

use crate::{Cpg, FileId, NodeId};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SegmentDigest(pub u64);

impl SegmentDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        SegmentDigest(h.finish())
    }
}

#[derive(Clone, Debug)]
pub struct SegmentKey {
    pub path: String,
    pub digest: SegmentDigest,
}

#[derive(Clone, Debug)]
pub struct SegmentDescriptor {
    pub key: SegmentKey,
    pub file: FileId,
    pub nodes: Vec<NodeId>,
}

#[derive(Clone, Debug, Default)]
pub struct SegmentManifest {
    by_file: HashMap<FileId, SegmentDescriptor>,
    by_digest: HashMap<SegmentDigest, FileId>,
}

impl SegmentManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_file(&mut self, cpg: &Cpg, file: FileId, source: &str) -> Option<&SegmentDescriptor> {
        let path = cpg.path_of(file)?.to_string();
        let digest = SegmentDigest::from_bytes(source.as_bytes());
        let nodes = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n))
            .collect();
        let descriptor = SegmentDescriptor { key: SegmentKey { path, digest }, file, nodes };
        self.by_digest.insert(digest, file);
        self.by_file.insert(file, descriptor);
        self.by_file.get(&file)
    }

    pub fn descriptor(&self, file: FileId) -> Option<&SegmentDescriptor> {
        self.by_file.get(&file)
    }

    pub fn by_digest(&self, digest: SegmentDigest) -> Option<&SegmentDescriptor> {
        self.by_digest.get(&digest).and_then(|file| self.by_file.get(file))
    }

    pub fn remove_file(&mut self, file: FileId) -> Option<SegmentDescriptor> {
        let removed = self.by_file.remove(&file)?;
        self.by_digest.remove(&removed.key.digest);
        Some(removed)
    }

    pub fn len(&self) -> usize {
        self.by_file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_file.is_empty()
    }
}
