//! Columnar CPG storage with file-partitioned, mutable subgraphs.
//!
//! Design choices and why:
//!
//! * **Columnar properties.** Each property is a `Vec` indexed by `NodeId`,
//!   not a field on a per-node struct. This keeps hot scans (e.g. "every Call
//!   named `strcpy`") cache-friendly and lets unused properties cost nothing.
//!
//! * **String interning.** All textual properties are `Sym` handles, so the
//!   N-th `int` parameter does not store the string "int" N times.
//!
//! * **File-partitioned, mutable adjacency.** Edges live in per-node
//!   out/in adjacency lists rather than a frozen CSR. CSR is faster to scan but
//!   cannot be mutated cheaply; incremental re-analysis needs to *delete one
//!   file's subgraph and rebuild it* without touching the other 99.99% of the
//!   graph. Adjacency lists make that O(size-of-changed-file). A `freeze()`
//!   step (future work, documented in ARCHITECTURE.md) can compact a quiescent
//!   graph into CSR for query-heavy, read-only workloads.
//!
//! * **Tombstones + free list.** Deleted nodes are marked dead and their ids
//!   recycled, so a long-lived server process editing files repeatedly does not
//!   grow unbounded.

use crate::intern::{Interner, Sym};
use crate::persist::{ByteReader, ByteWriter, DecodeError};
use crate::schema::{EdgeKind, NodeKind};
use std::collections::HashMap;

const MAGIC: &[u8; 4] = b"CPG1";

/// Stable handle to a node. Index into the columnar arrays.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// Identifies the source file a node belongs to (the incrementality unit).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId(pub u32);

/// One end of an edge as seen from a node's adjacency list.
#[derive(Clone, Copy, Debug)]
pub struct HalfEdge {
    pub kind: EdgeKind,
    pub other: NodeId,
}

/// The columnar property store + adjacency, partitioned by file.
#[derive(Default)]
pub struct Cpg {
    pub strings: Interner,

    // --- columnar node properties, indexed by NodeId.0 ---
    kind: Vec<NodeKind>,
    file: Vec<FileId>,
    name: Vec<Option<Sym>>,
    full_name: Vec<Option<Sym>>,
    code: Vec<Option<Sym>>,
    type_full_name: Vec<Option<Sym>>,
    signature: Vec<Option<Sym>>,
    line: Vec<Option<u32>>,
    order: Vec<i32>,
    argument_index: Vec<i32>,
    live: Vec<bool>,

    // --- mutable adjacency ---
    out_edges: Vec<Vec<HalfEdge>>,
    in_edges: Vec<Vec<HalfEdge>>,

    // --- file partitioning + recycling ---
    nodes_of_file: HashMap<FileId, Vec<NodeId>>,
    path_of_file: HashMap<FileId, String>,
    file_of_path: HashMap<String, FileId>,
    free_list: Vec<NodeId>,
    next_file: u32,
}

impl Cpg {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> Sym {
        self.strings.intern(s)
    }

    /// Register (or look up) a file and return its id.
    pub fn file_id(&mut self, path: &str) -> FileId {
        if let Some(&id) = self.file_of_path.get(path) {
            return id;
        }
        let id = FileId(self.next_file);
        self.next_file += 1;
        self.path_of_file.insert(id, path.to_string());
        self.file_of_path.insert(path.to_string(), id);
        self.nodes_of_file.insert(id, Vec::new());
        id
    }

    pub fn path_of(&self, file: FileId) -> Option<&str> {
        self.path_of_file.get(&file).map(|s| s.as_str())
    }

    /// Create a node belonging to `file`. Reuses a tombstoned slot if available.
    pub fn add_node(&mut self, kind: NodeKind, file: FileId) -> NodeId {
        let id = if let Some(reused) = self.free_list.pop() {
            let i = reused.0 as usize;
            self.kind[i] = kind;
            self.file[i] = file;
            self.name[i] = None;
            self.full_name[i] = None;
            self.code[i] = None;
            self.type_full_name[i] = None;
            self.signature[i] = None;
            self.line[i] = None;
            self.order[i] = 0;
            self.argument_index[i] = -1;
            self.live[i] = true;
            self.out_edges[i].clear();
            self.in_edges[i].clear();
            reused
        } else {
            let id = NodeId(self.kind.len() as u32);
            self.kind.push(kind);
            self.file.push(file);
            self.name.push(None);
            self.full_name.push(None);
            self.code.push(None);
            self.type_full_name.push(None);
            self.signature.push(None);
            self.line.push(None);
            self.order.push(0);
            self.argument_index.push(-1);
            self.live.push(true);
            self.out_edges.push(Vec::new());
            self.in_edges.push(Vec::new());
            id
        };
        self.nodes_of_file.entry(file).or_default().push(id);
        id
    }

    pub fn add_edge(&mut self, src: NodeId, dst: NodeId, kind: EdgeKind) {
        self.out_edges[src.0 as usize].push(HalfEdge { kind, other: dst });
        self.in_edges[dst.0 as usize].push(HalfEdge { kind, other: src });
    }

    /// Remove all out-edges of a given kind from `n` (and their mirror in-edges).
    /// Used to make a re-runnable pass idempotent: clear its prior output for a
    /// file before recomputing, so incremental re-runs don't duplicate edges.
    pub fn remove_out_edges_of_kind(&mut self, n: NodeId, kind: EdgeKind) {
        let i = n.0 as usize;
        let removed: Vec<NodeId> = self.out_edges[i]
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.other)
            .collect();
        self.out_edges[i].retain(|e| e.kind != kind);
        for other in removed {
            let j = other.0 as usize;
            self.in_edges[j].retain(|e| !(e.kind == kind && e.other == n));
        }
    }

    /// Delete every node belonging to `file` and all incident edges. This is
    /// the core incremental primitive: it touches only the changed file's
    /// nodes plus the neighbours that referenced them.
    pub fn remove_file(&mut self, file: FileId) {
        let nodes = self.nodes_of_file.remove(&file).unwrap_or_default();
        for &n in &nodes {
            let i = n.0 as usize;
            // Unhook this node from each neighbour's opposite-direction list.
            let outs = std::mem::take(&mut self.out_edges[i]);
            for e in &outs {
                let nb = &mut self.in_edges[e.other.0 as usize];
                nb.retain(|h| h.other != n);
            }
            let ins = std::mem::take(&mut self.in_edges[i]);
            for e in &ins {
                let nb = &mut self.out_edges[e.other.0 as usize];
                nb.retain(|h| h.other != n);
            }
            self.live[i] = false;
            self.free_list.push(n);
        }
        self.nodes_of_file.insert(file, Vec::new());
    }

    // --- property setters (builder-facing) ---
    pub fn set_name(&mut self, n: NodeId, s: Sym) {
        self.name[n.0 as usize] = Some(s);
    }
    pub fn set_full_name(&mut self, n: NodeId, s: Sym) {
        self.full_name[n.0 as usize] = Some(s);
    }
    pub fn set_code(&mut self, n: NodeId, s: Sym) {
        self.code[n.0 as usize] = Some(s);
    }
    pub fn set_type_full_name(&mut self, n: NodeId, s: Sym) {
        self.type_full_name[n.0 as usize] = Some(s);
    }
    pub fn set_signature(&mut self, n: NodeId, s: Sym) {
        self.signature[n.0 as usize] = Some(s);
    }
    pub fn set_line(&mut self, n: NodeId, line: u32) {
        self.line[n.0 as usize] = Some(line);
    }
    pub fn set_order(&mut self, n: NodeId, order: i32) {
        self.order[n.0 as usize] = order;
    }
    pub fn set_argument_index(&mut self, n: NodeId, idx: i32) {
        self.argument_index[n.0 as usize] = idx;
    }

    // --- property getters ---
    pub fn kind_of(&self, n: NodeId) -> NodeKind {
        self.kind[n.0 as usize]
    }
    pub fn file_of(&self, n: NodeId) -> FileId {
        self.file[n.0 as usize]
    }
    pub fn is_live(&self, n: NodeId) -> bool {
        self.live[n.0 as usize]
    }
    pub fn name_of(&self, n: NodeId) -> Option<&str> {
        self.name[n.0 as usize].map(|s| self.strings.resolve(s))
    }
    pub fn full_name_of(&self, n: NodeId) -> Option<&str> {
        self.full_name[n.0 as usize].map(|s| self.strings.resolve(s))
    }
    pub fn code_of(&self, n: NodeId) -> Option<&str> {
        self.code[n.0 as usize].map(|s| self.strings.resolve(s))
    }
    pub fn type_full_name_of(&self, n: NodeId) -> Option<&str> {
        self.type_full_name[n.0 as usize].map(|s| self.strings.resolve(s))
    }
    pub fn signature_of(&self, n: NodeId) -> Option<&str> {
        self.signature[n.0 as usize].map(|s| self.strings.resolve(s))
    }
    pub fn line_of(&self, n: NodeId) -> Option<u32> {
        self.line[n.0 as usize]
    }
    pub fn order_of(&self, n: NodeId) -> i32 {
        self.order[n.0 as usize]
    }
    pub fn argument_index_of(&self, n: NodeId) -> i32 {
        self.argument_index[n.0 as usize]
    }

    // --- adjacency access ---
    pub fn out(&self, n: NodeId) -> &[HalfEdge] {
        &self.out_edges[n.0 as usize]
    }
    pub fn in_(&self, n: NodeId) -> &[HalfEdge] {
        &self.in_edges[n.0 as usize]
    }

    /// Outgoing neighbours along a single edge kind.
    pub fn out_kind(&self, n: NodeId, kind: EdgeKind) -> impl Iterator<Item = NodeId> + '_ {
        self.out_edges[n.0 as usize]
            .iter()
            .filter(move |e| e.kind == kind)
            .map(|e| e.other)
    }
    pub fn in_kind(&self, n: NodeId, kind: EdgeKind) -> impl Iterator<Item = NodeId> + '_ {
        self.in_edges[n.0 as usize]
            .iter()
            .filter(move |e| e.kind == kind)
            .map(|e| e.other)
    }

    /// All live nodes (skips tombstones).
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.kind.len() as u32)
            .map(NodeId)
            .filter(move |n| self.live[n.0 as usize])
    }

    /// All registered file ids.
    pub fn files(&self) -> Vec<FileId> {
        self.path_of_file.keys().copied().collect()
    }

    pub fn nodes_in_file(&self, file: FileId) -> &[NodeId] {
        self.nodes_of_file
            .get(&file)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Count of live nodes (diagnostics / tests).
    pub fn live_count(&self) -> usize {
        self.live.iter().filter(|&&l| l).count()
    }

    /// Serialise the whole graph to bytes (see `persist` for the format).
    /// In-edges, per-file node lists and the free list are derived structures
    /// and are rebuilt on load rather than stored.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = ByteWriter::new();
        w.buf.extend_from_slice(MAGIC);

        // String table.
        w.u64(self.strings.len() as u64);
        for i in 0..self.strings.len() {
            w.bytes(self.strings.resolve(Sym(i as u32)).as_bytes());
        }

        // Node columns.
        let n = self.kind.len();
        w.u64(n as u64);
        for i in 0..n {
            w.u8(self.kind[i].to_u8());
        }
        for i in 0..n {
            w.u32(self.file[i].0);
        }
        for col in [
            &self.name,
            &self.full_name,
            &self.code,
            &self.type_full_name,
            &self.signature,
        ] {
            for v in col {
                w.opt_u32(v.map(|s| s.0));
            }
        }
        for v in &self.line {
            w.opt_u32(*v);
        }
        for v in &self.order {
            w.i32(*v);
        }
        for v in &self.argument_index {
            w.i32(*v);
        }
        for v in &self.live {
            w.u8(*v as u8);
        }

        // Out-edges (in-edges are rebuilt from these).
        for i in 0..n {
            w.u32(self.out_edges[i].len() as u32);
            for e in &self.out_edges[i] {
                w.u8(e.kind.to_u8());
                w.u32(e.other.0);
            }
        }

        // File table.
        w.u32(self.next_file);
        w.u64(self.path_of_file.len() as u64);
        for (id, path) in &self.path_of_file {
            w.u32(id.0);
            w.bytes(path.as_bytes());
        }
        w.buf
    }

    /// Reconstruct a graph from `to_bytes` output.
    pub fn from_bytes(data: &[u8]) -> Result<Cpg, DecodeError> {
        let mut r = ByteReader::new(data);
        let magic = (0..4).map(|_| r.u8()).collect::<Result<Vec<_>, _>>()?;
        if magic.as_slice() != MAGIC {
            return Err(DecodeError("bad magic; not a CPG1 file".into()));
        }
        let mut cpg = Cpg::new();

        let str_count = r.u64()? as usize;
        for _ in 0..str_count {
            let b = r.bytes()?;
            let s = std::str::from_utf8(b).map_err(|e| DecodeError(e.to_string()))?;
            cpg.strings.intern(s);
        }

        let n = r.u64()? as usize;
        let read_col_u8 = |r: &mut ByteReader, n: usize| -> Result<Vec<u8>, DecodeError> {
            (0..n).map(|_| r.u8()).collect()
        };
        cpg.kind = read_col_u8(&mut r, n)?
            .into_iter()
            .map(NodeKind::from_u8)
            .collect();
        cpg.file = (0..n)
            .map(|_| r.u32().map(FileId))
            .collect::<Result<_, _>>()?;
        let mut sym_col = || -> Result<Vec<Option<Sym>>, DecodeError> {
            (0..n).map(|_| r.opt_u32().map(|o| o.map(Sym))).collect()
        };
        // Order matches the write side: name, full_name, code, type_full_name, signature.
        cpg.name = sym_col()?;
        cpg.full_name = sym_col()?;
        cpg.code = sym_col()?;
        cpg.type_full_name = sym_col()?;
        cpg.signature = sym_col()?;
        cpg.line = (0..n).map(|_| r.opt_u32()).collect::<Result<_, _>>()?;
        cpg.order = (0..n).map(|_| r.i32()).collect::<Result<_, _>>()?;
        cpg.argument_index = (0..n).map(|_| r.i32()).collect::<Result<_, _>>()?;
        cpg.live = (0..n)
            .map(|_| Ok(r.u8()? != 0))
            .collect::<Result<_, DecodeError>>()?;

        cpg.out_edges = Vec::with_capacity(n);
        cpg.in_edges = vec![Vec::new(); n];
        for _ in 0..n {
            let m = r.u32()? as usize;
            let mut outs = Vec::with_capacity(m);
            for _ in 0..m {
                let kind = EdgeKind::from_u8(r.u8()?);
                let other = NodeId(r.u32()?);
                outs.push(HalfEdge { kind, other });
            }
            cpg.out_edges.push(outs);
        }
        // Rebuild in-edges as the mirror of out-edges.
        for src in 0..n {
            for e in cpg.out_edges[src].clone() {
                cpg.in_edges[e.other.0 as usize].push(HalfEdge {
                    kind: e.kind,
                    other: NodeId(src as u32),
                });
            }
        }

        // File table.
        cpg.next_file = r.u32()?;
        let file_count = r.u64()? as usize;
        for _ in 0..file_count {
            let id = FileId(r.u32()?);
            let path = std::str::from_utf8(r.bytes()?)
                .map_err(|e| DecodeError(e.to_string()))?
                .to_string();
            cpg.path_of_file.insert(id, path.clone());
            cpg.file_of_path.insert(path, id);
            cpg.nodes_of_file.insert(id, Vec::new());
        }

        // Rebuild per-file node lists and the free list from liveness.
        for i in 0..n {
            let node = NodeId(i as u32);
            if cpg.live[i] {
                cpg.nodes_of_file.entry(cpg.file[i]).or_default().push(node);
            } else {
                cpg.free_list.push(node);
            }
        }
        Ok(cpg)
    }

    /// Save to a file path.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }

    /// Load from a file path.
    pub fn load(path: &str) -> std::io::Result<Cpg> {
        let data = std::fs::read(path)?;
        Cpg::from_bytes(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.0))
    }

    /// Merge another graph into this one, remapping node ids, file ids and
    /// interned strings. This is the join step of the parallel build: workers
    /// build standalone per-file graphs concurrently, then the driver absorbs
    /// them serially (the merge is cheap relative to parsing+building).
    pub fn absorb(&mut self, donor: Cpg) {
        // Remap donor files onto this graph's file table by path.
        let mut file_map: HashMap<FileId, FileId> = HashMap::new();
        for (donor_file, path) in &donor.path_of_file {
            file_map.insert(*donor_file, self.file_id(path));
        }

        // Donor ids are dense (freshly built, no tombstones), so a flat Vec
        // remap beats a HashMap by a wide margin on large merges. The same
        // applies to strings: the donor's interner already deduped them, so we
        // hash each *distinct* donor string once into a sym→sym memo instead of
        // re-hashing per node occurrence.
        let donor_nodes: Vec<NodeId> = donor.nodes().collect();
        let cap = donor.kind.len();
        let mut node_map: Vec<NodeId> = vec![NodeId(u32::MAX); cap];
        let mut sym_map: Vec<Option<Sym>> = vec![None; donor.strings.len()];
        for &n in &donor_nodes {
            let i = n.0 as usize;
            let f = file_map[&donor.file[i]];
            let nn = self.add_node(donor.kind[i], f);
            let mut map_sym = |slf: &mut Self, s: Sym| -> Sym {
                if let Some(m) = sym_map[s.0 as usize] {
                    return m;
                }
                let m = slf.strings.intern(donor.strings.resolve(s));
                sym_map[s.0 as usize] = Some(m);
                m
            };
            if let Some(s) = donor.name[i] {
                let s = map_sym(self, s);
                self.set_name(nn, s);
            }
            if let Some(s) = donor.full_name[i] {
                let s = map_sym(self, s);
                self.set_full_name(nn, s);
            }
            if let Some(s) = donor.code[i] {
                let s = map_sym(self, s);
                self.set_code(nn, s);
            }
            if let Some(s) = donor.type_full_name[i] {
                let s = map_sym(self, s);
                self.set_type_full_name(nn, s);
            }
            if let Some(s) = donor.signature[i] {
                let s = map_sym(self, s);
                self.set_signature(nn, s);
            }
            if let Some(l) = donor.line[i] {
                self.set_line(nn, l);
            }
            self.set_order(nn, donor.order[i]);
            self.set_argument_index(nn, donor.argument_index[i]);
            node_map[i] = nn;
        }
        // Out-edges only; add_edge mirrors the in-edge.
        for &n in &donor_nodes {
            for e in donor.out(n) {
                self.add_edge(node_map[n.0 as usize], node_map[e.other.0 as usize], e.kind);
            }
        }
    }
}
