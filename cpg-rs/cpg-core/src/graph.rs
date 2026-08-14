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
use crate::schema::{EdgeKind, Layer, NodeKind};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;

const MAGIC_V1: &[u8; 4] = b"CPG1";
const MAGIC_V2: &[u8; 4] = b"CPG2";
const FORMAT_VERSION: u16 = 1;
const CHECKSUM_ALGORITHM_CRC32: u8 = 1;
const CHECKSUM_VERSION: u8 = 1;
const FLAG_AUTHORITATIVE_AST: u32 = 1 << 0;
const FLAG_AUTHORITATIVE_SYMBOL_REF: u32 = 1 << 1;
const FLAG_AUTHORITATIVE_CALL_GRAPH: u32 = 1 << 2;
const FLAG_AUTHORITATIVE_CFG: u32 = 1 << 3;
const FLAG_AUTHORITATIVE_DDG: u32 = 1 << 4;
const FLAG_AUTHORITATIVE_SUMMARIES: u32 = 1 << 5;
const KNOWN_ENVELOPE_FLAGS: u32 = FLAG_AUTHORITATIVE_AST
    | FLAG_AUTHORITATIVE_SYMBOL_REF
    | FLAG_AUTHORITATIVE_CALL_GRAPH
    | FLAG_AUTHORITATIVE_CFG
    | FLAG_AUTHORITATIVE_DDG
    | FLAG_AUTHORITATIVE_SUMMARIES;
const ENVELOPE_LEN: usize = 24;

/// The recorded 3.6-million-node benchmark occupies 184 MiB on disk at about
/// 53 bytes per node (`ARCHITECTURE.md`). One GiB leaves more than 5x measured
/// headroom (roughly 20 million nodes at that density) while bounding both the
/// input buffer and the larger decoded graph on ordinary production systems.
/// This operational ceiling is intentionally below the `u32` `NodeId` limit.
pub const MAX_CPG_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_STRINGS: u64 = 10_000_000;
const MAX_NODES: u64 = 25_000_000;
const MAX_EDGES: u64 = 100_000_000;
const MAX_FILES: u64 = 1_000_000;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 64 * 1024;
const MIN_NODE_PAYLOAD_BYTES: usize = 42;

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
    /// Layers supplied as authoritative facts by a project-wide frontend.
    /// The standard pass manager leaves these intact instead of replacing
    /// them with a lower-fidelity generic reconstruction.
    authoritative_layers: HashSet<Layer>,
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

    pub fn mark_layer_authoritative(&mut self, layer: Layer) {
        self.authoritative_layers.insert(layer);
    }

    pub fn is_layer_authoritative(&self, layer: Layer) -> bool {
        self.authoritative_layers.contains(&layer)
    }

    /// Serialise the whole graph to a CPG2 envelope (see `persist`). In-edges,
    /// per-file node lists and the free list are derived structures and are
    /// rebuilt on load rather than stored.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.try_to_bytes()
            .expect("graph exceeds the persisted CPG format limits")
    }

    fn try_to_bytes(&self) -> io::Result<Vec<u8>> {
        self.validate_for_persistence()?;
        let payload = self.payload_bytes();
        let total_len = ENVELOPE_LEN
            .checked_add(payload.len())
            .ok_or_else(|| invalid_input("CPG byte length overflow"))?;
        if u64::try_from(total_len).unwrap_or(u64::MAX) > MAX_CPG_BYTES {
            return Err(invalid_input(format!(
                "CPG is {total_len} bytes; maximum is {MAX_CPG_BYTES}"
            )));
        }

        let mut w = ByteWriter::new();
        w.buf.reserve(total_len);
        w.buf.extend_from_slice(MAGIC_V2);
        w.u16(FORMAT_VERSION);
        w.u8(CHECKSUM_ALGORITHM_CRC32);
        w.u8(CHECKSUM_VERSION);
        w.u32(self.envelope_flags());
        w.u64(payload.len() as u64);
        w.u32(crc32fast::hash(&payload));
        w.buf.extend_from_slice(&payload);
        Ok(w.buf)
    }

    fn payload_bytes(&self) -> Vec<u8> {
        let mut w = ByteWriter::new();

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
        let mut files: Vec<(&FileId, &String)> = self.path_of_file.iter().collect();
        files.sort_by_key(|(id, _)| id.0);
        for (id, path) in files {
            w.u32(id.0);
            w.bytes(path.as_bytes());
        }
        w.buf
    }

    fn envelope_flags(&self) -> u32 {
        self.authoritative_layers.iter().fold(0, |flags, layer| {
            flags
                | match layer {
                    Layer::Ast => FLAG_AUTHORITATIVE_AST,
                    Layer::SymbolRef => FLAG_AUTHORITATIVE_SYMBOL_REF,
                    Layer::CallGraph => FLAG_AUTHORITATIVE_CALL_GRAPH,
                    Layer::Cfg => FLAG_AUTHORITATIVE_CFG,
                    Layer::Ddg => FLAG_AUTHORITATIVE_DDG,
                    Layer::Summaries => FLAG_AUTHORITATIVE_SUMMARIES,
                }
        })
    }

    fn layers_from_flags(flags: u32) -> HashSet<Layer> {
        [
            (FLAG_AUTHORITATIVE_AST, Layer::Ast),
            (FLAG_AUTHORITATIVE_SYMBOL_REF, Layer::SymbolRef),
            (FLAG_AUTHORITATIVE_CALL_GRAPH, Layer::CallGraph),
            (FLAG_AUTHORITATIVE_CFG, Layer::Cfg),
            (FLAG_AUTHORITATIVE_DDG, Layer::Ddg),
            (FLAG_AUTHORITATIVE_SUMMARIES, Layer::Summaries),
        ]
        .into_iter()
        .filter_map(|(flag, layer)| (flags & flag != 0).then_some(layer))
        .collect()
    }

    fn validate_for_persistence(&self) -> io::Result<()> {
        check_encode_count("strings", self.strings.len(), MAX_STRINGS)?;
        check_encode_count("nodes", self.kind.len(), MAX_NODES)?;
        check_encode_count("files", self.path_of_file.len(), MAX_FILES)?;
        if self.path_of_file.len() as u64 != u64::from(self.next_file) {
            return Err(invalid_input(
                "file table is not dense or next_file is inconsistent",
            ));
        }

        for i in 0..self.strings.len() {
            let len = self.strings.resolve(Sym(i as u32)).len();
            if len > MAX_STRING_BYTES || u32::try_from(len).is_err() {
                return Err(invalid_input(format!(
                    "string {i} is {len} bytes; maximum is {MAX_STRING_BYTES}"
                )));
            }
        }
        for (id, path) in &self.path_of_file {
            if id.0 >= self.next_file {
                return Err(invalid_input(format!(
                    "file id {} is not below next_file {}",
                    id.0, self.next_file
                )));
            }
            if path.len() > MAX_PATH_BYTES {
                return Err(invalid_input(format!(
                    "file path for id {} is {} bytes; maximum is {MAX_PATH_BYTES}",
                    id.0,
                    path.len()
                )));
            }
        }

        let string_count = self.strings.len();
        let mut edge_count = 0_u64;
        for i in 0..self.kind.len() {
            if !self.path_of_file.contains_key(&self.file[i]) {
                return Err(invalid_input(format!(
                    "node {i} references unregistered file id {}",
                    self.file[i].0
                )));
            }
            for sym in [
                self.name[i],
                self.full_name[i],
                self.code[i],
                self.type_full_name[i],
                self.signature[i],
            ]
            .into_iter()
            .flatten()
            {
                if sym.0 as usize >= string_count {
                    return Err(invalid_input(format!(
                        "node {i} references invalid string index {}",
                        sym.0
                    )));
                }
            }
            if !self.live[i] && !self.out_edges[i].is_empty() {
                return Err(invalid_input(format!("dead node {i} has outgoing edges")));
            }
            edge_count = edge_count
                .checked_add(self.out_edges[i].len() as u64)
                .ok_or_else(|| invalid_input("edge count overflow"))?;
            if edge_count > MAX_EDGES {
                return Err(invalid_input(format!(
                    "edges count {edge_count} exceeds maximum {MAX_EDGES}"
                )));
            }
            if u32::try_from(self.out_edges[i].len()).is_err() {
                return Err(invalid_input(format!(
                    "node {i} has too many outgoing edges"
                )));
            }
            for edge in &self.out_edges[i] {
                let target = edge.other.0 as usize;
                if target >= self.kind.len() || !self.live[target] {
                    return Err(invalid_input(format!(
                        "edge from node {i} references invalid or dead node {}",
                        edge.other.0
                    )));
                }
            }
        }
        Ok(())
    }

    /// Reconstruct a graph from CPG2 output. Legacy CPG1 payloads remain
    /// readable, but pass through the same bounded, validating decoder.
    pub fn from_bytes(data: &[u8]) -> Result<Cpg, DecodeError> {
        if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_CPG_BYTES {
            return Err(DecodeError(format!(
                "CPG is {} bytes; maximum is {MAX_CPG_BYTES}",
                data.len()
            )));
        }
        let magic = data
            .get(..4)
            .ok_or_else(|| DecodeError("unexpected EOF while reading CPG magic".into()))?;
        if magic == MAGIC_V1 {
            return Self::decode_payload(&data[4..], 0);
        }
        if magic != MAGIC_V2 {
            return Err(DecodeError("bad magic; expected CPG1 or CPG2".into()));
        }

        let mut envelope = ByteReader::new(&data[4..]);
        let version = envelope.u16()?;
        if version != FORMAT_VERSION {
            return Err(DecodeError(format!(
                "unsupported CPG2 format version {version}; expected {FORMAT_VERSION}"
            )));
        }
        let algorithm = envelope.u8()?;
        let checksum_version = envelope.u8()?;
        if algorithm != CHECKSUM_ALGORITHM_CRC32 || checksum_version != CHECKSUM_VERSION {
            return Err(DecodeError(format!(
                "unsupported checksum algorithm/version {algorithm}/{checksum_version}"
            )));
        }
        let flags = envelope.u32()?;
        if flags & !KNOWN_ENVELOPE_FLAGS != 0 {
            return Err(DecodeError(format!(
                "unsupported CPG2 envelope flags 0x{flags:08x}"
            )));
        }
        let payload_len_u64 = envelope.u64()?;
        if payload_len_u64 > MAX_CPG_BYTES - ENVELOPE_LEN as u64 {
            return Err(DecodeError(format!(
                "CPG payload is {payload_len_u64} bytes; maximum is {}",
                MAX_CPG_BYTES - ENVELOPE_LEN as u64
            )));
        }
        let payload_len = usize::try_from(payload_len_u64)
            .map_err(|_| DecodeError("CPG payload does not fit this architecture".into()))?;
        let expected_checksum = envelope.u32()?;
        if envelope.position() + 4 != ENVELOPE_LEN {
            return Err(DecodeError("internal CPG2 envelope layout mismatch".into()));
        }
        if envelope.remaining() != payload_len {
            return Err(DecodeError(format!(
                "CPG2 payload length is {payload_len}, but {} bytes remain",
                envelope.remaining()
            )));
        }
        let payload = envelope.take(payload_len)?;
        let actual_checksum = crc32fast::hash(payload);
        if actual_checksum != expected_checksum {
            return Err(DecodeError(format!(
                "CPG2 checksum mismatch: expected {expected_checksum:08x}, got {actual_checksum:08x}"
            )));
        }
        Self::decode_payload(payload, flags)
    }

    fn decode_payload(payload: &[u8], flags: u32) -> Result<Cpg, DecodeError> {
        let mut r = ByteReader::new(payload);
        let str_count = read_count(&mut r, "strings", MAX_STRINGS, 4)?;
        let mut strings = Interner::new();
        for i in 0..str_count {
            let bytes = read_bounded_bytes(&mut r, "string", MAX_STRING_BYTES)?;
            let text = std::str::from_utf8(bytes)
                .map_err(|e| DecodeError(format!("string {i} is not UTF-8: {e}")))?;
            let sym = strings.intern(text);
            if sym.0 as usize != i {
                return Err(DecodeError(format!(
                    "duplicate string table entry at index {i}"
                )));
            }
        }

        let n = read_count(&mut r, "nodes", MAX_NODES, MIN_NODE_PAYLOAD_BYTES)?;
        let mut kind = Vec::with_capacity(n);
        for i in 0..n {
            let raw = r.u8()?;
            kind.push(
                NodeKind::from_u8(raw)
                    .ok_or_else(|| DecodeError(format!("invalid node kind {raw} at node {i}")))?,
            );
        }
        let file = (0..n)
            .map(|_| r.u32().map(FileId))
            .collect::<Result<Vec<_>, _>>()?;
        let name = read_sym_column(&mut r, n, str_count, "name")?;
        let full_name = read_sym_column(&mut r, n, str_count, "full_name")?;
        let code = read_sym_column(&mut r, n, str_count, "code")?;
        let type_full_name = read_sym_column(&mut r, n, str_count, "type_full_name")?;
        let signature = read_sym_column(&mut r, n, str_count, "signature")?;
        let line = (0..n).map(|_| r.opt_u32()).collect::<Result<Vec<_>, _>>()?;
        let order = (0..n).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?;
        let argument_index = (0..n).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?;
        let mut live = Vec::with_capacity(n);
        for i in 0..n {
            match r.u8()? {
                0 => live.push(false),
                1 => live.push(true),
                value => {
                    return Err(DecodeError(format!(
                        "invalid live byte {value} at node {i}; expected 0 or 1"
                    )))
                }
            }
        }

        let mut out_edges = Vec::with_capacity(n);
        let mut total_edges = 0_u64;
        for src in 0..n {
            let count = r.u32()? as usize;
            total_edges = total_edges
                .checked_add(count as u64)
                .ok_or_else(|| DecodeError("edge count overflow".into()))?;
            if total_edges > MAX_EDGES {
                return Err(DecodeError(format!(
                    "edges count {total_edges} exceeds maximum {MAX_EDGES}"
                )));
            }
            require_remaining(&r, count, 5, "edges")?;
            if !live[src] && count != 0 {
                return Err(DecodeError(format!("dead node {src} has outgoing edges")));
            }
            let mut edges = Vec::with_capacity(count);
            for _ in 0..count {
                let raw_kind = r.u8()?;
                let edge_kind = EdgeKind::from_u8(raw_kind).ok_or_else(|| {
                    DecodeError(format!("invalid edge kind {raw_kind} at node {src}"))
                })?;
                let target = r.u32()? as usize;
                if target >= n {
                    return Err(DecodeError(format!(
                        "edge from node {src} targets out-of-range node {target} (count {n})"
                    )));
                }
                if !live[target] {
                    return Err(DecodeError(format!(
                        "edge from node {src} targets dead node {target}"
                    )));
                }
                edges.push(HalfEdge {
                    kind: edge_kind,
                    other: NodeId(target as u32),
                });
            }
            out_edges.push(edges);
        }

        let next_file = r.u32()?;
        let file_count = read_count(&mut r, "files", MAX_FILES, 8)?;
        if file_count as u64 != u64::from(next_file) {
            return Err(DecodeError(format!(
                "file count {file_count} is inconsistent with next_file {next_file}"
            )));
        }
        let mut path_of_file = HashMap::with_capacity(file_count);
        let mut file_of_path = HashMap::with_capacity(file_count);
        let mut nodes_of_file = HashMap::with_capacity(file_count);
        for _ in 0..file_count {
            let id = FileId(r.u32()?);
            if id.0 >= next_file {
                return Err(DecodeError(format!(
                    "file id {} is not below next_file {next_file}",
                    id.0
                )));
            }
            let bytes = read_bounded_bytes(&mut r, "file path", MAX_PATH_BYTES)?;
            let path = std::str::from_utf8(bytes)
                .map_err(|e| DecodeError(format!("file path for id {} is not UTF-8: {e}", id.0)))?
                .to_string();
            if path_of_file.insert(id, path.clone()).is_some() {
                return Err(DecodeError(format!("duplicate file id {}", id.0)));
            }
            if file_of_path.insert(path.clone(), id).is_some() {
                return Err(DecodeError(format!("duplicate file path {path:?}")));
            }
            nodes_of_file.insert(id, Vec::new());
        }
        if r.remaining() != 0 {
            return Err(DecodeError(format!(
                "{} trailing payload bytes at offset {}",
                r.remaining(),
                r.position()
            )));
        }
        for (i, node_file) in file.iter().enumerate() {
            if !path_of_file.contains_key(node_file) {
                return Err(DecodeError(format!(
                    "node {i} references missing file id {}",
                    node_file.0
                )));
            }
        }

        let mut in_edges = vec![Vec::new(); n];
        for (src, edges) in out_edges.iter().enumerate() {
            for edge in edges {
                in_edges[edge.other.0 as usize].push(HalfEdge {
                    kind: edge.kind,
                    other: NodeId(src as u32),
                });
            }
        }
        let mut free_list = Vec::new();
        for i in 0..n {
            let node = NodeId(i as u32);
            if live[i] {
                nodes_of_file
                    .get_mut(&file[i])
                    .expect("validated file id")
                    .push(node);
            } else {
                free_list.push(node);
            }
        }

        Ok(Cpg {
            strings,
            kind,
            file,
            name,
            full_name,
            code,
            type_full_name,
            signature,
            line,
            order,
            argument_index,
            live,
            out_edges,
            in_edges,
            nodes_of_file,
            path_of_file,
            file_of_path,
            free_list,
            next_file,
            authoritative_layers: Self::layers_from_flags(flags),
        })
    }

    /// Save through a same-directory temporary file and atomically publish it.
    /// Existing Unix permissions are preserved when the platform allows it;
    /// newly created files use the platform's normal owner-writable defaults.
    pub fn save(&self, path: &str) -> io::Result<()> {
        let data = self.try_to_bytes()?;
        write_atomically(Path::new(path), &data)
    }

    /// Load from a file path.
    pub fn load(path: &str) -> io::Result<Cpg> {
        let path = Path::new(path);
        let file = File::open(path).map_err(|e| path_error("open", path, e))?;
        let metadata = file
            .metadata()
            .map_err(|e| path_error("inspect", path, e))?;
        if metadata.len() > MAX_CPG_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CPG file {} is {} bytes; maximum is {MAX_CPG_BYTES}",
                    path.display(),
                    metadata.len()
                ),
            ));
        }
        // The handle, not the path, is authoritative. A bounded reader closes
        // the metadata/read race if the opened file grows after this check.
        let data = read_bounded(file, MAX_CPG_BYTES).map_err(|e| path_error("read", path, e))?;
        Cpg::from_bytes(&data).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid CPG file {}: {}", path.display(), e.0),
            )
        })
    }

    /// Merge another graph into this one, remapping node ids, file ids and
    /// interned strings. This is the join step of the parallel build: workers
    /// build standalone per-file graphs concurrently, then the driver absorbs
    /// them serially (the merge is cheap relative to parsing+building).
    pub fn absorb(&mut self, donor: Cpg) {
        self.authoritative_layers
            .extend(donor.authoritative_layers.iter().copied());
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

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn path_error(operation: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("failed to {operation} {}: {error}", path.display()),
    )
}

fn check_encode_count(label: &str, count: usize, maximum: u64) -> io::Result<()> {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    if count > maximum {
        return Err(invalid_input(format!(
            "{label} count {count} exceeds maximum {maximum}"
        )));
    }
    Ok(())
}

fn require_remaining(
    reader: &ByteReader<'_>,
    count: usize,
    bytes_per_item: usize,
    label: &str,
) -> Result<(), DecodeError> {
    let required = count
        .checked_mul(bytes_per_item)
        .ok_or_else(|| DecodeError(format!("{label} byte length overflow for count {count}")))?;
    if required > reader.remaining() {
        return Err(DecodeError(format!(
            "impossible {label} count {count}: requires at least {required} bytes, {} remain",
            reader.remaining()
        )));
    }
    Ok(())
}

fn read_count(
    reader: &mut ByteReader<'_>,
    label: &str,
    maximum: u64,
    minimum_bytes_per_item: usize,
) -> Result<usize, DecodeError> {
    let count_u64 = reader.u64()?;
    if count_u64 > maximum {
        return Err(DecodeError(format!(
            "{label} count {count_u64} exceeds maximum {maximum}"
        )));
    }
    let count = usize::try_from(count_u64)
        .map_err(|_| DecodeError(format!("{label} count does not fit this architecture")))?;
    require_remaining(reader, count, minimum_bytes_per_item, label)?;
    Ok(count)
}

fn read_bounded_bytes<'a>(
    reader: &mut ByteReader<'a>,
    label: &str,
    maximum: usize,
) -> Result<&'a [u8], DecodeError> {
    let len = reader.u32()? as usize;
    if len > maximum {
        return Err(DecodeError(format!(
            "{label} length {len} exceeds maximum {maximum}"
        )));
    }
    reader.take(len)
}

fn read_sym_column(
    reader: &mut ByteReader<'_>,
    count: usize,
    string_count: usize,
    label: &str,
) -> Result<Vec<Option<Sym>>, DecodeError> {
    let mut column = Vec::with_capacity(count);
    for node in 0..count {
        let sym = reader.opt_u32()?;
        if let Some(sym) = sym {
            if sym as usize >= string_count {
                return Err(DecodeError(format!(
                    "{label} at node {node} references string {sym}, but count is {string_count}"
                )));
            }
            column.push(Some(Sym(sym)));
        } else {
            column.push(None);
        }
    }
    Ok(column)
}

fn read_bounded<R: Read>(reader: R, maximum: u64) -> io::Result<Vec<u8>> {
    let limit = maximum
        .checked_add(1)
        .ok_or_else(|| invalid_input("CPG read limit overflow"))?;
    let mut reader = reader.take(limit);
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("CPG input exceeds maximum {maximum} bytes"),
        ));
    }
    Ok(data)
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn write_atomically(path: &Path, data: &[u8]) -> io::Result<()> {
    write_atomically_with(path, data, |file, destination| file.persist(destination))
}

fn write_atomically_with<F>(path: &Path, data: &[u8], publish: F) -> io::Result<()>
where
    F: FnOnce(NamedTempFile, &Path) -> Result<File, tempfile::PersistError>,
{
    if path.file_name().is_none() {
        return Err(invalid_input(format!(
            "CPG destination {} has no file name",
            path.display()
        )));
    }
    let parent = parent_directory(path);
    let existing_permissions = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(path_error("inspect", path, error)),
    };
    let mut file = tempfile::Builder::new()
        .prefix(".cpg-tmp-")
        .tempfile_in(parent)
        .map_err(|e| path_error("create temporary file for", path, e))?;
    if let Some(permissions) = existing_permissions {
        if let Err(error) = file.as_file().set_permissions(permissions) {
            return Err(cleanup_temporary_after_failure(
                file,
                path,
                "preserve permissions for",
                error,
            ));
        }
    }
    if let Err(error) = file.write_all(data) {
        return Err(cleanup_temporary_after_failure(file, path, "write", error));
    }
    if let Err(error) = file.flush() {
        return Err(cleanup_temporary_after_failure(file, path, "flush", error));
    }
    if let Err(error) = file.as_file().sync_all() {
        return Err(cleanup_temporary_after_failure(file, path, "sync", error));
    }
    match publish(file, path) {
        Ok(published_file) => {
            drop(published_file);
            sync_parent_directory(parent)
                .map_err(|e| path_error("sync parent directory for", path, e))
        }
        Err(error) => Err(cleanup_temporary_after_failure(
            error.file,
            path,
            "atomically publish",
            error.error,
        )),
    }
}

fn cleanup_temporary_after_failure(
    file: NamedTempFile,
    destination: &Path,
    operation: &str,
    error: io::Error,
) -> io::Error {
    let temporary_path = file.path().to_path_buf();
    let kind = error.kind();
    let message = format!("failed to {operation} {}: {error}", destination.display());
    match file.close() {
        Ok(()) => io::Error::new(kind, message),
        Err(cleanup_error) => io::Error::new(
            kind,
            format!(
                "{message}; also failed to remove temporary file {}: {cleanup_error}",
                temporary_path.display()
            ),
        ),
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::builder::CpgBuilder;
    use crate::traversal::Query;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};

    const PAYLOAD_STRING_BYTE: usize = 12;
    const PAYLOAD_NODE_KIND: usize = 21;
    const PAYLOAD_NODE_FILE: usize = 22;
    const PAYLOAD_NAME_SYM: usize = 26;
    const PAYLOAD_LIVE: usize = 58;
    const PAYLOAD_EDGE_COUNT: usize = 59;
    const PAYLOAD_EDGE_KIND: usize = 63;
    const PAYLOAD_EDGE_TARGET: usize = 64;
    const PAYLOAD_NEXT_FILE: usize = 68;
    const PAYLOAD_FILE_COUNT: usize = 72;
    const PAYLOAD_FILE_ID: usize = 80;
    const PAYLOAD_PATH_BYTE: usize = 88;

    fn sample_cpg(method_name: &str) -> Cpg {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("a.c");
        let mut builder = CpgBuilder::new(&mut cpg, file);
        let root = builder.file_node("a.c");
        let method = builder.method(method_name, method_name, "int(void)", Some(1));
        builder.contains(root, method);
        cpg
    }

    /// One string, one live node with one self-edge, and one registered file.
    /// The fixed shape makes field-targeted corruptions easy to audit.
    fn valid_payload() -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.u64(1);
        writer.bytes(b"x");
        writer.u64(1);
        writer.u8(NodeKind::Method.to_u8());
        writer.u32(0);
        writer.opt_u32(Some(0));
        for _ in 0..4 {
            writer.opt_u32(None);
        }
        writer.opt_u32(None);
        writer.i32(0);
        writer.i32(-1);
        writer.u8(1);
        writer.u32(1);
        writer.u8(EdgeKind::Ast.to_u8());
        writer.u32(0);
        writer.u32(1);
        writer.u64(1);
        writer.u32(0);
        writer.bytes(b"a.c");
        assert_eq!(writer.buf.len(), 91);
        writer.buf
    }

    fn cpg2(payload: &[u8]) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.buf.extend_from_slice(MAGIC_V2);
        writer.u16(FORMAT_VERSION);
        writer.u8(CHECKSUM_ALGORITHM_CRC32);
        writer.u8(CHECKSUM_VERSION);
        writer.u32(0);
        writer.u64(payload.len() as u64);
        writer.u32(crc32fast::hash(payload));
        writer.buf.extend_from_slice(payload);
        writer.buf
    }

    fn legacy_cpg1(payload: &[u8]) -> Vec<u8> {
        let mut bytes = MAGIC_V1.to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    fn with_payload_mutation(mut mutate: impl FnMut(&mut Vec<u8>)) -> Vec<u8> {
        let mut payload = valid_payload();
        mutate(&mut payload);
        cpg2(&payload)
    }

    fn assert_decode_error_without_panic(label: &str, bytes: &[u8]) {
        let result = std::panic::catch_unwind(|| Cpg::from_bytes(bytes));
        assert!(result.is_ok(), "{label} panicked");
        assert!(result.unwrap().is_err(), "{label} unexpectedly decoded");
    }

    #[test]
    fn persistence_envelope_accepts_cpg2_and_validated_cpg1() {
        let mut cpg = sample_cpg("main");
        cpg.mark_layer_authoritative(Layer::Cfg);
        cpg.mark_layer_authoritative(Layer::Ddg);
        let bytes = cpg.to_bytes();
        assert_eq!(&bytes[..4], MAGIC_V2);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), FORMAT_VERSION);
        let reopened = Cpg::from_bytes(&bytes).unwrap();
        assert_eq!(reopened.methods().len(), 1);
        assert!(reopened.is_layer_authoritative(Layer::Cfg));
        assert!(reopened.is_layer_authoritative(Layer::Ddg));
        assert!(!reopened.is_layer_authoritative(Layer::CallGraph));

        let legacy = legacy_cpg1(&cpg.payload_bytes());
        let legacy = Cpg::from_bytes(&legacy).unwrap();
        assert_eq!(legacy.methods().len(), 1);
        assert!(!legacy.is_layer_authoritative(Layer::Cfg));
    }

    #[test]
    fn persistence_envelope_rejects_incompatible_or_corrupt_files() {
        let valid = cpg2(&valid_payload());
        let mut cases = Vec::new();

        cases.push(("empty", Vec::new()));

        let mut bad_magic = valid.clone();
        bad_magic[..4].copy_from_slice(b"NOPE");
        cases.push(("magic", bad_magic));

        let mut bad_version = valid.clone();
        bad_version[4..6].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        cases.push(("version", bad_version));

        let mut bad_algorithm = valid.clone();
        bad_algorithm[6] = 99;
        cases.push(("checksum algorithm", bad_algorithm));

        let mut bad_checksum_version = valid.clone();
        bad_checksum_version[7] = CHECKSUM_VERSION + 1;
        cases.push(("checksum version", bad_checksum_version));

        let mut bad_flags = valid.clone();
        bad_flags[8..12].copy_from_slice(&(1_u32 << 31).to_le_bytes());
        cases.push(("flags", bad_flags));

        let mut short_length = valid.clone();
        short_length[12..20].copy_from_slice(&90_u64.to_le_bytes());
        cases.push(("short declared length", short_length));

        let mut long_length = valid.clone();
        long_length[12..20].copy_from_slice(&92_u64.to_le_bytes());
        cases.push(("long declared length", long_length));

        let mut bad_checksum = valid.clone();
        bad_checksum[20] ^= 0x80;
        cases.push(("checksum", bad_checksum));

        let mut envelope_trailing = valid.clone();
        envelope_trailing.push(0);
        cases.push(("envelope trailing byte", envelope_trailing));

        let payload_trailing = with_payload_mutation(|payload| payload.push(0));
        cases.push(("payload trailing byte", payload_trailing));

        for (label, bytes) in cases {
            assert_decode_error_without_panic(label, &bytes);
        }
    }

    #[test]
    fn persistence_malformed_fields_are_rejected_before_graph_construction() {
        let mut cases = Vec::new();

        cases.push((
            "string count",
            with_payload_mutation(|p| p[0..8].copy_from_slice(&u64::MAX.to_le_bytes())),
        ));
        cases.push((
            "string length",
            with_payload_mutation(|p| p[8..12].copy_from_slice(&u32::MAX.to_le_bytes())),
        ));
        cases.push((
            "invalid UTF-8 string",
            with_payload_mutation(|p| p[PAYLOAD_STRING_BYTE] = 0xff),
        ));
        cases.push((
            "node count",
            with_payload_mutation(|p| p[13..21].copy_from_slice(&u64::MAX.to_le_bytes())),
        ));
        cases.push((
            "node kind",
            with_payload_mutation(|p| p[PAYLOAD_NODE_KIND] = 0xff),
        ));
        cases.push((
            "missing node file",
            with_payload_mutation(|p| {
                p[PAYLOAD_NODE_FILE..PAYLOAD_NODE_FILE + 4].copy_from_slice(&1_u32.to_le_bytes())
            }),
        ));
        cases.push((
            "symbol index",
            with_payload_mutation(|p| {
                p[PAYLOAD_NAME_SYM..PAYLOAD_NAME_SYM + 4].copy_from_slice(&1_u32.to_le_bytes())
            }),
        ));
        cases.push(("live byte", with_payload_mutation(|p| p[PAYLOAD_LIVE] = 2)));
        cases.push((
            "edge count",
            with_payload_mutation(|p| {
                p[PAYLOAD_EDGE_COUNT..PAYLOAD_EDGE_COUNT + 4]
                    .copy_from_slice(&u32::MAX.to_le_bytes())
            }),
        ));
        cases.push((
            "edge kind",
            with_payload_mutation(|p| p[PAYLOAD_EDGE_KIND] = 0xff),
        ));
        cases.push((
            "edge target",
            with_payload_mutation(|p| {
                p[PAYLOAD_EDGE_TARGET..PAYLOAD_EDGE_TARGET + 4]
                    .copy_from_slice(&1_u32.to_le_bytes())
            }),
        ));
        cases.push((
            "next file",
            with_payload_mutation(|p| {
                p[PAYLOAD_NEXT_FILE..PAYLOAD_NEXT_FILE + 4].copy_from_slice(&0_u32.to_le_bytes())
            }),
        ));
        cases.push((
            "file count",
            with_payload_mutation(|p| {
                p[PAYLOAD_FILE_COUNT..PAYLOAD_FILE_COUNT + 8]
                    .copy_from_slice(&u64::MAX.to_le_bytes())
            }),
        ));
        cases.push((
            "file id",
            with_payload_mutation(|p| {
                p[PAYLOAD_FILE_ID..PAYLOAD_FILE_ID + 4].copy_from_slice(&1_u32.to_le_bytes())
            }),
        ));
        cases.push((
            "invalid UTF-8 path",
            with_payload_mutation(|p| p[PAYLOAD_PATH_BYTE] = 0xff),
        ));

        let duplicate_file_id = with_payload_mutation(|p| {
            p[PAYLOAD_NEXT_FILE..PAYLOAD_NEXT_FILE + 4].copy_from_slice(&2_u32.to_le_bytes());
            p[PAYLOAD_FILE_COUNT..PAYLOAD_FILE_COUNT + 8].copy_from_slice(&2_u64.to_le_bytes());
            p.extend_from_slice(&0_u32.to_le_bytes());
            p.extend_from_slice(&3_u32.to_le_bytes());
            p.extend_from_slice(b"b.c");
        });
        cases.push(("duplicate file id", duplicate_file_id));

        let duplicate_path = with_payload_mutation(|p| {
            p[PAYLOAD_NEXT_FILE..PAYLOAD_NEXT_FILE + 4].copy_from_slice(&2_u32.to_le_bytes());
            p[PAYLOAD_FILE_COUNT..PAYLOAD_FILE_COUNT + 8].copy_from_slice(&2_u64.to_le_bytes());
            p.extend_from_slice(&1_u32.to_le_bytes());
            p.extend_from_slice(&3_u32.to_le_bytes());
            p.extend_from_slice(b"a.c");
        });
        cases.push(("duplicate file path", duplicate_path));

        let mut legacy_trailing = legacy_cpg1(&valid_payload());
        legacy_trailing.push(0);
        cases.push(("legacy trailing byte", legacy_trailing));

        for (label, bytes) in cases {
            assert_decode_error_without_panic(label, &bytes);
        }
    }

    #[test]
    fn persistence_malformed_bytes_never_panic() {
        let valid = cpg2(&valid_payload());
        for end in 0..valid.len() {
            let label = format!("truncation at {end}");
            assert_decode_error_without_panic(&label, &valid[..end]);
        }
        for index in 0..valid.len() {
            for replacement in [0_u8, 1, 0x7f, 0xff] {
                let mut mutated = valid.clone();
                mutated[index] = replacement;
                let result = std::panic::catch_unwind(|| Cpg::from_bytes(&mutated));
                assert!(
                    result.is_ok(),
                    "single-byte mutation panicked at {index} with {replacement:#04x}"
                );
            }
        }
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "cpg-core-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            TestDir(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn assert_only_destination_remains(directory: &Path, destination: &Path) {
        let entries = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(entries, [destination]);
    }

    fn assert_no_temporary_files(directory: &Path) {
        let temporary = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".cpg-tmp-"))
            .collect::<Vec<_>>();
        assert!(
            temporary.is_empty(),
            "temporary files remain: {temporary:?}"
        );
    }

    #[test]
    fn persistence_atomic_overwrite_and_failed_publication_are_complete() {
        let directory = TestDir::new("atomic");
        let destination = directory.0.join("graph.cpg");
        let destination_str = destination.to_str().unwrap();
        let old = sample_cpg("old");
        old.save(destination_str).unwrap();
        let old_bytes = std::fs::read(&destination).unwrap();

        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&destination).unwrap().permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(&destination, permissions).unwrap();
        }

        let new = sample_cpg("new");
        let new_bytes = new.try_to_bytes().unwrap();
        let error = write_atomically_with(&destination, &new_bytes, |file, _| {
            Err(tempfile::PersistError {
                error: io::Error::other("injected publish failure"),
                file,
            })
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("injected publish failure"));
        assert_eq!(std::fs::read(&destination).unwrap(), old_bytes);
        assert_only_destination_remains(&directory.0, &destination);

        // Exercise the real platform replace primitive too: replacing a
        // non-empty directory with a file must fail, and the owned temp must
        // still be removed.
        let blocked_destination = directory.0.join("blocked.cpg");
        std::fs::create_dir(&blocked_destination).unwrap();
        let sentinel = blocked_destination.join("old-graph-still-here");
        std::fs::write(&sentinel, b"old").unwrap();
        let error = write_atomically(&blocked_destination, &new_bytes).unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"old");
        assert_no_temporary_files(&directory.0);

        new.save(destination_str).unwrap();
        let restored = Cpg::load(destination_str).unwrap();
        assert_eq!(restored.method_named("new").len(), 1);
        assert_eq!(&std::fs::read(&destination).unwrap()[..4], MAGIC_V2);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_no_temporary_files(&directory.0);
    }

    #[test]
    fn persistence_load_reader_rejects_growth_beyond_size_cap() {
        assert_eq!(
            read_bounded(std::io::Cursor::new(vec![0_u8; 16]), 16)
                .unwrap()
                .len(),
            16
        );
        let input = std::io::Cursor::new(vec![0_u8; 17]);
        let error = read_bounded(input, 16).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds maximum 16 bytes"));
    }

    #[test]
    fn persistence_load_rejects_oversized_file_before_allocating() {
        let directory = TestDir::new("oversized-load");
        let path = directory.0.join("oversized.cpg");
        File::create(&path)
            .unwrap()
            .set_len(MAX_CPG_BYTES + 1)
            .unwrap();
        let error = match Cpg::load(path.to_str().unwrap()) {
            Ok(_) => panic!("oversized CPG unexpectedly loaded"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("maximum"));
    }

    #[test]
    fn persistence_atomic_concurrent_writers_never_mix_graphs() {
        let directory = TestDir::new("atomic-concurrent");
        let destination = directory.0.join("graph.cpg");
        let barrier = Arc::new(Barrier::new(8));
        let mut writers = Vec::new();
        for index in 0..8 {
            let destination = destination.clone();
            let barrier = Arc::clone(&barrier);
            writers.push(std::thread::spawn(move || {
                let name = if index % 2 == 0 { "alpha" } else { "beta" };
                let graph = sample_cpg(name);
                barrier.wait();
                graph.save(destination.to_str().unwrap())
            }));
        }
        for writer in writers {
            writer.join().unwrap().unwrap();
        }

        let restored = Cpg::load(destination.to_str().unwrap()).unwrap();
        assert_eq!(
            restored.method_named("alpha").len() + restored.method_named("beta").len(),
            1
        );
        assert_only_destination_remains(&directory.0, &destination);
    }
}
