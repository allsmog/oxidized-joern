//! Canonical adapter for the shipped C graph.
//!
//! This deliberately constructs a `cpg_incremental::Project` with
//! `cpg_lang_c::CFrontend` and `cpg_analysis::standard_pipeline`, exactly as
//! the released CLI does. During convergence the historical standalone dump
//! remains the required oracle path; `--migration-report` makes every
//! difference visible without normalising semantic gaps away.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy)]
pub enum Mode {
    Production,
    MigrationReport,
}

pub fn dump_paths(paths: &[String]) -> String {
    let sources: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read production parity input {path}: {e}"));
            let name = Path::new(path)
                .file_name()
                .unwrap_or_else(|| panic!("input has no filename: {path}"))
                .to_string_lossy()
                .into_owned();
            (name, source)
        })
        .collect();
    dump_sources(&sources)
}

pub fn dump_sources(sources: &[(String, String)]) -> String {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    project.build(&refs);
    cpg_lang_c::import::canonical_dump(&project.cpg)
}

pub fn migration_report(standalone: &str, production: &str) -> String {
    let old = sections(standalone);
    let new = sections(production);
    let mut report = String::new();
    let mut total_removed = 0usize;
    let mut total_added = 0usize;
    for name in ["AST", "NODES", "EDGES", "FLOWS"] {
        let a = &old[name];
        let b = &new[name];
        let common = a.intersection(b).count();
        let removed = a.difference(b).count();
        let added = b.difference(a).count();
        total_removed += removed;
        total_added += added;
        report.push_str(&format!(
            "SECTION {name} standalone={} production={} common={common} removed={removed} added={added} exact={}\n",
            a.len(),
            b.len(),
            removed == 0 && added == 0
        ));
    }
    report.push_str(&format!(
        "TOTAL removed={total_removed} added={total_added} exact={}\n",
        total_removed == 0 && total_added == 0
    ));
    report
}

fn sections(text: &str) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut result: BTreeMap<&'static str, BTreeSet<String>> = [
        ("AST", BTreeSet::new()),
        ("NODES", BTreeSet::new()),
        ("EDGES", BTreeSet::new()),
        ("FLOWS", BTreeSet::new()),
    ]
    .into_iter()
    .collect();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (section, value) = if let Some(value) = line.strip_prefix("NODES|") {
            ("NODES", value)
        } else if let Some(value) = line.strip_prefix("EDGES|") {
            ("EDGES", value)
        } else if let Some(value) = line.strip_prefix("FLOWS|") {
            ("FLOWS", value)
        } else {
            ("AST", line.strip_prefix("AST|").unwrap_or(line))
        };
        result.get_mut(section).unwrap().insert(value.to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_dump_is_deterministic_and_uses_standard_layers() {
        let sources = vec![
            (
                "b.c".to_string(),
                "int twice(int x) { return x + x; }".to_string(),
            ),
            (
                "a.c".to_string(),
                "int main(void) { return twice(2); }".to_string(),
            ),
        ];
        let first = dump_sources(&sources);
        let second = dump_sources(&sources);
        assert_eq!(first, second);
        assert!(first.contains("METHOD NAME=main"));
        assert!(first.contains("EDGES|CFG "));
        assert!(first.contains("FLOWS|REACHING_DEF["));
    }

    #[test]
    fn migration_report_is_stable_and_sectioned() {
        let report = migration_report(
            "METHOD NAME=a\nNODES|FILE NAME=a.c\n",
            "METHOD NAME=b\nNODES|FILE NAME=a.c\n",
        );
        assert!(report.contains(
            "SECTION AST standalone=1 production=1 common=0 removed=1 added=1 exact=false"
        ));
        assert!(report.contains(
            "SECTION NODES standalone=1 production=1 common=1 removed=0 added=0 exact=true"
        ));
        assert!(report.ends_with("TOTAL removed=1 added=1 exact=false\n"));
    }
}
