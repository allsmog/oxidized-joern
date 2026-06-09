//! The incremental driver — the end-to-end realisation of roadmap item #1.
//!
//! A [`Project`] owns the CPG, a frontend, the pass pipeline, and the summary
//! cache. Editing one file does the minimum work:
//!
//! 1. Hash the new source; if unchanged, do nothing.
//! 2. Delete exactly that file's subgraph and rebuild it (`O(changed file)`).
//! 3. Re-run the pass pipeline only on the changed file *and the caller files
//!    that reference symbols it defines or removes* — never the whole project.
//! 4. Invalidate and recompute only the affected dataflow summaries; everything
//!    else is served from the cache.
//!
//! This is the property the architecture review flagged as the highest-value
//! lever and the one neither Joern nor Fraunhofer's CPG has today.

use cpg_core::{Cpg, FileId, NodeId, NodeKind};
use cpg_analysis::{PassManager, SummaryStore};
use cpg_frontend::Frontend;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Per-phase timings for a full build.
#[derive(Debug, Clone, Copy)]
pub struct BuildStats {
    /// Total frontend phase (parallel workers + serial merge).
    pub parse_build: std::time::Duration,
    /// Parallel portion: parse + per-file subgraph construction.
    pub parallel_frontend: std::time::Duration,
    /// Serial portion: absorbing per-file subgraphs into the main graph.
    pub merge: std::time::Duration,
    pub passes: std::time::Duration,
    pub summaries: std::time::Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Content hash matched; no work done.
    Unchanged,
    /// Rebuilt. Reports how much work the edit actually cost.
    Rebuilt {
        files_reanalysed: usize,
        summaries_recomputed: usize,
    },
}

/// Creates fresh frontend instances. Each parallel build worker needs its own
/// parser state, so the project holds a factory rather than a single frontend.
pub type FrontendFactory = Box<dyn Fn() -> Box<dyn Frontend> + Send + Sync>;

pub struct Project {
    pub cpg: Cpg,
    factory: FrontendFactory,
    frontend: Box<dyn Frontend>,
    pipeline: PassManager,
    pub summaries: SummaryStore,
    file_hashes: HashMap<String, u64>,
    methods_by_file: HashMap<FileId, HashSet<String>>,
    /// Per-file method fqns + the global fqn -> node index, both maintained
    /// incrementally so the edit path never scans the graph for methods.
    method_fqns_by_file: HashMap<FileId, HashSet<String>>,
    node_of_fqn: HashMap<String, NodeId>,
    /// name -> method nodes, handed to passes via PassContext so call
    /// resolution never rebuilds a global index.
    method_nodes_by_name: HashMap<String, Vec<NodeId>>,
    method_nodes_by_file: HashMap<FileId, Vec<(String, NodeId)>>,
    /// Call names appearing in each file (to unhook the reverse index on edit).
    call_names_by_file: HashMap<FileId, HashSet<String>>,
    /// Reverse dependency index: callee name -> files containing such a call.
    /// Makes "which files does this edit affect?" O(affected), removing the
    /// last whole-graph scan from the edit path.
    callers_of_name: HashMap<String, HashSet<FileId>>,
}

impl Project {
    pub fn new(
        factory: impl Fn() -> Box<dyn Frontend> + Send + Sync + 'static,
        pipeline: PassManager,
    ) -> Self {
        let frontend = factory();
        Project {
            cpg: Cpg::new(),
            factory: Box::new(factory),
            frontend,
            pipeline,
            summaries: SummaryStore::new(),
            file_hashes: HashMap::new(),
            methods_by_file: HashMap::new(),
            method_fqns_by_file: HashMap::new(),
            node_of_fqn: HashMap::new(),
            method_nodes_by_name: HashMap::new(),
            method_nodes_by_file: HashMap::new(),
            call_names_by_file: HashMap::new(),
            callers_of_name: HashMap::new(),
        }
    }

    /// Load external library summaries (JSON, Fraunhofer-style).
    pub fn load_external_summaries(&mut self, json: &str) -> Result<usize, String> {
        self.summaries.load_external_json(json)
    }

    /// Adopt a graph loaded from disk: the persisted graph already holds all
    /// pass-produced edges (CFG/refs/calls), so we only rebuild the in-memory
    /// indices and recompute summaries (which are not persisted). Parsing is
    /// skipped entirely — the point of persistence. Per-file source hashes are
    /// not restored, so the first edit to any file always rebuilds it.
    pub fn reopen(&mut self, cpg: Cpg) {
        self.cpg = cpg;
        self.file_hashes.clear();
        self.methods_by_file.clear();
        self.method_fqns_by_file.clear();
        self.node_of_fqn.clear();
        self.method_nodes_by_name.clear();
        self.method_nodes_by_file.clear();
        self.call_names_by_file.clear();
        self.callers_of_name.clear();
        for f in self.cpg.files() {
            self.record_methods(f);
            self.record_calls(f);
        }
        self.summaries = SummaryStore::new();
        self.summaries.compute_all(&self.cpg);
    }

    /// Initial bulk build of a whole project. Parsing+building is the dominant
    /// phase (~70% of a cold build) and is embarrassingly parallel: each worker
    /// builds a standalone per-file graph with its own frontend instance, then
    /// the driver absorbs them serially (id/string remapping is cheap relative
    /// to parsing). Returns per-phase timings so perf work stays
    /// evidence-driven.
    pub fn build(&mut self, files: &[(&str, &str)]) -> BuildStats {
        use rayon::prelude::*;
        let t0 = std::time::Instant::now();
        let donors: Vec<Cpg> = files
            .par_iter()
            .map(|(path, src)| {
                let mut fe = (self.factory)();
                let mut g = Cpg::new();
                fe.build_file(&mut g, path, src);
                g
            })
            .collect();
        let parallel_done = t0.elapsed();
        let mut ids = Vec::new();
        for ((path, src), donor) in files.iter().zip(donors) {
            self.cpg.absorb(donor);
            let id = self.cpg.file_id(path);
            self.file_hashes.insert((*path).to_string(), hash(src));
            ids.push(id);
            self.record_methods(id);
            self.record_calls(id);
        }
        let parse_build = t0.elapsed();

        let t1 = std::time::Instant::now();
        let ctx = cpg_analysis::PassContext {
            methods_by_name: Some(&self.method_nodes_by_name),
        };
        self.pipeline.run_all(&mut self.cpg, &ids, &ctx);
        let passes = t1.elapsed();

        let t2 = std::time::Instant::now();
        self.summaries.compute_all(&self.cpg);
        let summaries = t2.elapsed();

        BuildStats {
            parse_build,
            parallel_frontend: parallel_done,
            merge: parse_build - parallel_done,
            passes,
            summaries,
        }
    }

    /// Apply an edit to a single file and re-analyse the minimum needed.
    pub fn update_file(&mut self, path: &str, source: &str) -> UpdateOutcome {
        let h = hash(source);
        if self.file_hashes.get(path) == Some(&h) {
            return UpdateOutcome::Unchanged;
        }

        let file = self.cpg.file_id(path);
        let old_methods = self.methods_by_file.get(&file).cloned().unwrap_or_default();

        // Rebuild just this file's subgraph.
        self.cpg.remove_file(file);
        self.frontend.build_file(&mut self.cpg, path, source);
        self.file_hashes.insert(path.to_string(), h);
        self.record_methods(file);
        self.record_calls(file);
        let new_methods = self.methods_by_file.get(&file).cloned().unwrap_or_default();

        // Files whose call resolution might be affected: callers of any method
        // name that appeared or disappeared in this file. Served by the reverse
        // index — O(affected), no graph scan.
        let mut to_reanalyse: HashSet<FileId> = HashSet::from([file]);
        for name in old_methods.union(&new_methods) {
            if let Some(callers) = self.callers_of_name.get(name) {
                to_reanalyse.extend(callers.iter().copied());
            }
        }

        let files: Vec<FileId> = to_reanalyse.iter().copied().collect();
        let ctx = cpg_analysis::PassContext {
            methods_by_name: Some(&self.method_nodes_by_name),
        };
        self.pipeline.run_all(&mut self.cpg, &files, &ctx);

        // Directly-changed methods: every method living in a re-analysed file
        // (served by the per-file index, no graph scan).
        let directly_changed: HashSet<String> = files
            .iter()
            .filter_map(|f| self.method_fqns_by_file.get(f))
            .flat_map(|s| s.iter().cloned())
            .collect();
        self.summaries
            .update_for_changed_methods(&self.cpg, directly_changed, &self.node_of_fqn);

        UpdateOutcome::Rebuilt {
            files_reanalysed: files.len(),
            summaries_recomputed: self.summaries.last_recomputed.len(),
        }
    }

    fn record_methods(&mut self, file: FileId) {
        // Unhook the file's previous entries from the global indices.
        if let Some(old) = self.method_fqns_by_file.remove(&file) {
            for fqn in old {
                self.node_of_fqn.remove(&fqn);
            }
        }
        if let Some(old) = self.method_nodes_by_file.remove(&file) {
            for (name, node) in old {
                if let Some(v) = self.method_nodes_by_name.get_mut(&name) {
                    v.retain(|&m| m != node);
                }
            }
        }
        let mut names: HashSet<String> = HashSet::new();
        let mut fqns: HashSet<String> = HashSet::new();
        let mut named_nodes: Vec<(String, NodeId)> = Vec::new();
        let nodes: Vec<NodeId> = self.cpg.nodes_in_file(file).to_vec();
        for n in nodes {
            if self.cpg.is_live(n) && self.cpg.kind_of(n) == NodeKind::Method {
                if let Some(name) = self.cpg.name_of(n) {
                    names.insert(name.to_string());
                    named_nodes.push((name.to_string(), n));
                    self.method_nodes_by_name
                        .entry(name.to_string())
                        .or_default()
                        .push(n);
                }
                if let Some(fqn) = self.cpg.full_name_of(n) {
                    fqns.insert(fqn.to_string());
                    self.node_of_fqn.insert(fqn.to_string(), n);
                }
            }
        }
        self.methods_by_file.insert(file, names);
        self.method_fqns_by_file.insert(file, fqns);
        self.method_nodes_by_file.insert(file, named_nodes);
    }

    /// Refresh the reverse-dependency index for one file: unhook its previous
    /// call names, then register the current ones.
    fn record_calls(&mut self, file: FileId) {
        if let Some(old) = self.call_names_by_file.remove(&file) {
            for name in old {
                if let Some(set) = self.callers_of_name.get_mut(&name) {
                    set.remove(&file);
                }
            }
        }
        let names: HashSet<String> = self
            .cpg
            .nodes_in_file(file)
            .iter()
            .filter(|&&n| self.cpg.is_live(n) && self.cpg.kind_of(n) == NodeKind::Call)
            .filter_map(|&n| self.cpg.name_of(n).map(|s| s.to_string()))
            .collect();
        for name in &names {
            self.callers_of_name
                .entry(name.clone())
                .or_default()
                .insert(file);
        }
        self.call_names_by_file.insert(file, names);
    }

    pub fn summary_of(&self, fqn: &str) -> Option<&cpg_analysis::FunctionSummary> {
        self.summaries.get(fqn)
    }

    /// Interprocedural source→sink taint query over the current summaries.
    /// Because it reads the (incrementally-maintained) summary cache, results
    /// reflect the latest edits without any extra recomputation here.
    pub fn find_taint(&self, sources: &[&str], sinks: &[&str]) -> Vec<cpg_analysis::Finding> {
        let spec = cpg_analysis::TaintSpec::new(sources, sinks);
        cpg_analysis::find_flows(&self.cpg, &self.summaries, &spec)
    }
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_analysis::{standard_pipeline, Point};
    use cpg_lang_c::CFrontend;

    fn project() -> Project {
        Project::new(|| Box::new(CFrontend::new()), standard_pipeline())
    }

    #[test]
    fn interprocedural_summary_via_callee() {
        // wrap(y) returns id(y); id(x) returns x. So wrap's summary must say
        // Param(0) -> Return, derived through id's summary (summaries-first).
        let mut p = project();
        p.build(&[(
            "m.c",
            r#"
                int id(int x) { return x; }
                int wrap(int y) { return id(y); }
            "#,
        )]);
        let wrap = p.summary_of("wrap").expect("wrap summarised");
        assert!(wrap.flows.iter().any(|f| f.from == Point::Param(0) && f.to == Point::Return));
    }

    #[test]
    fn unchanged_edit_is_a_noop() {
        let mut p = project();
        p.build(&[("a.c", "int f(int x){return x;}")]);
        let out = p.update_file("a.c", "int f(int x){return x;}");
        assert_eq!(out, UpdateOutcome::Unchanged);
    }

    #[test]
    fn edit_reanalyses_only_affected_files() {
        let mut p = project();
        p.build(&[
            ("a.c", "int helper(int x){ return x; }"),
            ("b.c", "int unrelated(int z){ return z; }"),
            ("c.c", "int caller(int q){ return helper(q); }"),
        ]);
        // Editing a.c (defines `helper`) should pull in c.c (calls helper) but
        // NOT b.c (unrelated). 3 files total; we expect 2 reanalysed.
        let out = p.update_file("a.c", "int helper(int x){ return x; }  // touched");
        match out {
            UpdateOutcome::Rebuilt { files_reanalysed, .. } => {
                assert_eq!(files_reanalysed, 2, "should touch a.c + c.c, not b.c");
            }
            _ => panic!("expected rebuild"),
        }
    }

    #[test]
    fn interprocedural_taint_source_to_sink() {
        // tainted = getenv("X"); system(wrap(tainted));  where wrap returns its
        // arg. The flow getenv -> wrap(...) -> system must be found through the
        // summary of wrap (interprocedural), not just direct argument matching.
        let mut p = project();
        p.build(&[(
            "v.c",
            r#"
                char* wrap(char* s) { return s; }
                void handle() {
                    char* tainted = getenv("X");
                    system(wrap(tainted));
                }
            "#,
        )]);
        let findings = p.find_taint(&["getenv"], &["system"]);
        assert_eq!(findings.len(), 1, "expected one source->sink flow: {findings:?}");
        assert_eq!(findings[0].sink, "system");
        assert_eq!(findings[0].origin, "getenv");
        // The witness path runs from the getenv source to the system sink.
        let path = &findings[0].path;
        assert!(path.len() >= 2, "expected a multi-step witness: {path:?}");
        assert!(path.first().unwrap().code.contains("getenv"));
        assert!(path.last().unwrap().code.contains("system"));
    }

    #[test]
    fn taint_finding_clears_after_fix() {
        // Editing wrap to stop returning its argument should break the flow —
        // the summary invalidation must propagate to the taint query.
        let mut p = project();
        p.build(&[(
            "v.c",
            "char* wrap(char* s){ return s; } void h(){ system(wrap(getenv(\"X\"))); }",
        )]);
        assert_eq!(p.find_taint(&["getenv"], &["system"]).len(), 1);

        p.update_file(
            "v.c",
            "char* wrap(char* s){ return \"safe\"; } void h(){ system(wrap(getenv(\"X\"))); }",
        );
        assert_eq!(
            p.find_taint(&["getenv"], &["system"]).len(),
            0,
            "flow should be gone after wrap no longer returns its arg"
        );
    }

    #[test]
    fn python_summaries_through_shared_engine() {
        // The identical driver + dataflow engine, fed by the Python frontend:
        // wrap(y) returns ident(y), so Param(0) -> Return must be derived
        // through ident's summary. No engine changes for the new language.
        use cpg_lang_python::PythonFrontend;
        let mut p = Project::new(|| Box::new(PythonFrontend::new()), standard_pipeline());
        p.build(&[(
            "m.py",
            "def ident(x):\n    return x\n\ndef wrap(y):\n    return ident(y)\n",
        )]);
        let wrap = p.summary_of("wrap").expect("wrap summarised");
        assert!(wrap.flows.iter().any(|f| f.from == Point::Param(0) && f.to == Point::Return));
    }

    #[test]
    fn external_json_summary_drives_taint() {
        let mut p = project();
        // Declare that libc's `strdup` flows param0 -> return.
        p.load_external_summaries(
            r#"[{"functionDeclaration":{"language":"C","methodName":"strdup"},
                 "dataFlows":[{"from":"param0","to":"return"}]}]"#,
        )
        .unwrap();
        p.build(&[("d.c", "char* g(char* s){ return strdup(s); }")]);
        let g = p.summary_of("g").unwrap();
        assert!(g.flows.iter().any(|f| f.from == Point::Param(0) && f.to == Point::Return));
    }
}
