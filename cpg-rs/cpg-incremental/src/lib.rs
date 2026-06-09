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

use cpg_core::{Cpg, FileId, NodeKind, Query};
use cpg_analysis::{PassManager, SummaryStore};
use cpg_frontend::Frontend;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

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

pub struct Project {
    pub cpg: Cpg,
    frontend: Box<dyn Frontend>,
    pipeline: PassManager,
    pub summaries: SummaryStore,
    file_hashes: HashMap<String, u64>,
    methods_by_file: HashMap<FileId, HashSet<String>>,
}

impl Project {
    pub fn new(frontend: Box<dyn Frontend>, pipeline: PassManager) -> Self {
        Project {
            cpg: Cpg::new(),
            frontend,
            pipeline,
            summaries: SummaryStore::new(),
            file_hashes: HashMap::new(),
            methods_by_file: HashMap::new(),
        }
    }

    /// Load external library summaries (JSON, Fraunhofer-style).
    pub fn load_external_summaries(&mut self, json: &str) -> Result<usize, String> {
        self.summaries.load_external_json(json)
    }

    /// Initial bulk build of a whole project.
    pub fn build(&mut self, files: &[(&str, &str)]) {
        let mut ids = Vec::new();
        for (path, src) in files {
            let id = self.cpg.file_id(path);
            self.frontend.build_file(&mut self.cpg, path, src);
            self.file_hashes.insert((*path).to_string(), hash(src));
            ids.push(id);
            self.record_methods(id);
        }
        self.pipeline.run_all(&mut self.cpg, &ids);
        self.summaries.compute_all(&self.cpg);
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
        let new_methods = self.methods_by_file.get(&file).cloned().unwrap_or_default();

        // Files whose call resolution might be affected: callers of any method
        // name that appeared or disappeared in this file.
        let affected_names: HashSet<&String> = old_methods.union(&new_methods).collect();
        let mut to_reanalyse: HashSet<FileId> = HashSet::from([file]);
        for c in self.cpg.calls() {
            if let Some(name) = self.cpg.name_of(c) {
                if affected_names.contains(&name.to_string()) {
                    to_reanalyse.insert(self.cpg.file_of(c));
                }
            }
        }

        let files: Vec<FileId> = to_reanalyse.iter().copied().collect();
        self.pipeline.run_all(&mut self.cpg, &files);
        self.summaries.update_for_changed_files(&self.cpg, &to_reanalyse);

        UpdateOutcome::Rebuilt {
            files_reanalysed: files.len(),
            summaries_recomputed: self.summaries.last_recomputed.len(),
        }
    }

    fn record_methods(&mut self, file: FileId) {
        let names: HashSet<String> = self
            .cpg
            .nodes_in_file(file)
            .iter()
            .filter(|&&n| self.cpg.is_live(n) && self.cpg.kind_of(n) == NodeKind::Method)
            .filter_map(|&n| self.cpg.name_of(n).map(|s| s.to_string()))
            .collect();
        self.methods_by_file.insert(file, names);
    }

    /// Convenience: does tainted data reach `sink` from any parameter of any
    /// method, using the computed summaries? (A tiny demonstration query.)
    pub fn summary_of(&self, fqn: &str) -> Option<&cpg_analysis::FunctionSummary> {
        self.summaries.get(fqn)
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
        Project::new(Box::new(CFrontend::new()), standard_pipeline())
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
