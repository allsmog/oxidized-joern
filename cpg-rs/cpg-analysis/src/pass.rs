//! Pass framework with declared read/write layers.
//!
//! Every pass declares the [`Layer`]s it consumes and produces. Two payoffs:
//!
//! 1. **Ordering is derived, not hand-wired.** The manager topologically sorts
//!    passes so a producer of `Cfg` runs before a consumer of `Cfg`.
//! 2. **Incrementality is mechanical.** Because passes run per-file and declare
//!    their layers, the driver can re-run the pass pipeline on just the files
//!    that changed instead of the whole project. This is the property the
//!    discussion flagged as the single highest-value lever and the one neither
//!    Joern nor Fraunhofer's CPG currently has.

use cpg_core::{Cpg, FileId, Layer, NodeId};
use std::collections::{HashMap, HashSet, VecDeque};

/// Shared lookups handed to passes by the driver. Indices the project already
/// maintains incrementally are *borrowed* here, so a pass that needs e.g. the
/// global method-name index doesn't rebuild it (O(methods)) on every pipeline
/// run — the difference between an O(project) and an O(affected) edit path.
#[derive(Default)]
pub struct PassContext<'a> {
    /// Method name -> defining method nodes, project-wide.
    pub methods_by_name: Option<&'a HashMap<String, Vec<NodeId>>>,
}

impl PassContext<'_> {
    pub fn empty() -> Self {
        Self::default()
    }
}

pub trait Pass {
    fn name(&self) -> &'static str;
    fn reads(&self) -> &'static [Layer];
    fn writes(&self) -> &'static [Layer];
    /// The edge kind this pass produces, if any. The manager clears these for a
    /// file before re-running the pass so incremental re-runs stay idempotent.
    fn output_edge(&self) -> Option<cpg_core::EdgeKind> {
        None
    }
    /// Re-derive this pass's output for a single file's subgraph.
    fn run_file(&self, cpg: &mut Cpg, file: FileId, ctx: &PassContext);

    /// Run over many files at once. The default loops `run_file`, but passes
    /// with expensive shared setup (e.g. a global symbol index) override this
    /// to build that state once instead of once per file — the difference
    /// between O(files × methods) and O(files + methods) on a full build.
    fn run_batch(&self, cpg: &mut Cpg, files: &[FileId], ctx: &PassContext) {
        for &f in files {
            self.run_file(cpg, f, ctx);
        }
    }
}

#[derive(Default)]
pub struct PassManager {
    passes: Vec<Box<dyn Pass>>,
}

impl PassManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, pass: Box<dyn Pass>) -> &mut Self {
        self.passes.push(pass);
        self
    }

    /// Topologically order passes: A precedes B if A writes a layer B reads.
    fn ordered(&self) -> Vec<usize> {
        let n = self.passes.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indeg = vec![0usize; n];
        for (a, pa) in self.passes.iter().enumerate() {
            for (b, pb) in self.passes.iter().enumerate() {
                if a == b {
                    continue;
                }
                let produces_for_b = pa
                    .writes()
                    .iter()
                    .any(|w| pb.reads().iter().any(|r| r == w));
                if produces_for_b {
                    adj[a].push(b);
                    indeg[b] += 1;
                }
            }
        }
        let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(u) = q.pop_front() {
            order.push(u);
            for &v in &adj[u] {
                indeg[v] -= 1;
                if indeg[v] == 0 {
                    q.push_back(v);
                }
            }
        }
        // If a cycle slipped in, fall back to declaration order for the rest.
        if order.len() != n {
            for i in 0..n {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        order
    }

    /// Clear a pass's prior output edges for one file's live nodes.
    fn clear_output(&self, cpg: &mut Cpg, file: FileId, p: usize) {
        if let Some(ek) = self.passes[p].output_edge() {
            let nodes: Vec<cpg_core::NodeId> = cpg.nodes_in_file(file).to_vec();
            for n in nodes {
                if cpg.is_live(n) {
                    cpg.remove_out_edges_of_kind(n, ek);
                }
            }
        }
    }

    /// Run the full pipeline over every given file. Idempotent: clears each
    /// pass's prior output for a file before recomputing it.
    pub fn run_all(&self, cpg: &mut Cpg, files: &[FileId], ctx: &PassContext) {
        let timing = std::env::var_os("CPG_PASS_TIMING").is_some();
        for &p in &self.ordered() {
            let t0 = std::time::Instant::now();
            for &f in files {
                self.clear_output(cpg, f, p);
            }
            let cleared = t0.elapsed();
            self.passes[p].run_batch(cpg, files, ctx);
            if timing {
                eprintln!(
                    "pass {}: {:?} (clear {:?})",
                    self.passes[p].name(),
                    t0.elapsed(),
                    cleared
                );
            }
        }
    }

    /// Run the pipeline over only the changed files. Returns the set of layers
    /// that were rewritten, so downstream consumers (e.g. the summary cache)
    /// know what to invalidate.
    pub fn run_incremental(&self, cpg: &mut Cpg, changed: &[FileId], ctx: &PassContext) -> HashSet<Layer> {
        let mut dirtied = HashSet::new();
        for &p in &self.ordered() {
            for &f in changed {
                self.clear_output(cpg, f, p);
                self.passes[p].run_file(cpg, f, ctx);
            }
            dirtied.extend(self.passes[p].writes().iter().copied());
        }
        dirtied
    }

    pub fn pass_names(&self) -> Vec<&'static str> {
        self.ordered().iter().map(|&i| self.passes[i].name()).collect()
    }
}

/// Collect AST descendants of `root` (inclusive), via Ast edges. Shared helper
/// used by several passes to walk a method's subtree.
pub fn ast_descendants(cpg: &Cpg, root: cpg_core::NodeId) -> Vec<cpg_core::NodeId> {
    use cpg_core::EdgeKind;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if !seen.insert(n) {
            continue;
        }
        out.push(n);
        for c in cpg.out_kind(n, EdgeKind::Ast) {
            stack.push(c);
        }
    }
    out
}

/// Index methods by name across the whole graph (for call resolution).
pub fn method_name_index(cpg: &Cpg) -> HashMap<String, Vec<cpg_core::NodeId>> {
    use cpg_core::{NodeKind, Query};
    let mut idx: HashMap<String, Vec<cpg_core::NodeId>> = HashMap::new();
    for m in cpg.nodes_of_kind(NodeKind::Method) {
        if let Some(name) = cpg.name_of(m) {
            idx.entry(name.to_string()).or_default().push(m);
        }
    }
    idx
}
