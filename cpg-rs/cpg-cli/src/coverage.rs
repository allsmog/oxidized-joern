//! Per-scan coverage report: makes a zero-finding scan falsifiable. A scan
//! that reports 0 findings is only evidence of absence if its entries
//! actually matched methods, its spec names actually name call sites in the
//! graph, and call resolution isn't mostly holes. Everything here is computed
//! from the CPG + the finished findings — no instrumentation inside the taint
//! engine (which runs once per rule and must stay stateless).

use crate::rules::RulePack;
use cpg_core::{Cpg, Query};
use std::collections::HashSet;
use std::fmt::Write;

/// Strip the positional suffixes (`name@k`, `name@out<k>`, `name@recv`), the
/// bare-only prefix (`::name`), and a receiver qualification (`recv.Name` —
/// member calls are named by their trailing member, so the simple name is
/// what must exist in the graph) — the spellings `TaintSpec` parses —
/// leaving the callable name that must exist in the graph for the spec entry
/// to ever fire.
pub fn normalize_spec_name(raw: &str) -> &str {
    let s = raw.strip_prefix("::").unwrap_or(raw);
    let s = s.split_once('@').map_or(s, |(n, _)| n);
    s.rsplit_once('.').map_or(s, |(_, n)| n)
}

/// Cap a name list for display: full list up to `cap`, then `(+N more)`.
fn brief(names: &[&str], cap: usize) -> String {
    if names.len() <= cap {
        names.join(", ")
    } else {
        format!("{} (+{} more)", names[..cap].join(", "), names.len() - cap)
    }
}

/// Render the coverage report. `curated`/`idl` are the entry names handed to
/// the scan; `finding_methods` the full names of methods any rule reported a
/// finding in.
pub fn coverage_report(
    cpg: &Cpg,
    curated: &[String],
    idl: &[String],
    pack: &RulePack,
    finding_methods: &HashSet<String>,
) -> String {
    let mut out = String::new();

    // Call resolution: operators (`=`, `+`) and other non-callable names
    // never resolve by design — measure only calls that name something a
    // call-graph edge could point at.
    let calls = cpg.calls();
    let named: Vec<_> = calls
        .iter()
        .copied()
        .filter(|&c| {
            cpg.name_of(c)
                .and_then(|n| n.chars().next())
                .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        })
        .collect();
    let unresolved = named.iter().filter(|&&c| cpg.call_target(c).is_none()).count();
    let pct = if named.is_empty() { 0.0 } else { 100.0 * unresolved as f64 / named.len() as f64 };
    let _ = writeln!(
        out,
        "coverage: {} named calls, {unresolved} unresolved ({pct:.0}%) — unresolved calls \
         cross into summaries/stubs, not spliced bodies",
        named.len()
    );

    // Entry matching. Curated entries are trusted verbatim by the engine
    // (simple OR full name, taint.rs entry loop) — replicate that rule so a
    // typo'd --entry is no longer silent. IDL entries additionally pass a
    // handler-shape guard downstream, so name-level matching here is an
    // upper bound.
    let mut simple: HashSet<&str> = HashSet::new();
    let mut full: HashSet<&str> = HashSet::new();
    let mut finding_simple: HashSet<&str> = HashSet::new();
    for m in cpg.methods() {
        let n = cpg.name_of(m).unwrap_or("");
        let f = cpg.full_name_of(m).unwrap_or(n);
        simple.insert(n);
        full.insert(f);
        if finding_methods.contains(f) {
            finding_simple.insert(n);
            finding_simple.insert(f);
        }
    }
    let is_match = |e: &str| simple.contains(e) || full.contains(e);
    if !curated.is_empty() {
        let unmatched: Vec<&str> =
            curated.iter().map(String::as_str).filter(|e| !is_match(e)).collect();
        let quiet = curated.len() - unmatched.len()
            - curated
                .iter()
                .filter(|e| is_match(e) && finding_simple.contains(e.as_str()))
                .count();
        let _ = writeln!(
            out,
            "coverage: curated entries {} requested, {} matched, {} matched-but-quiet (no finding)",
            curated.len(),
            curated.len() - unmatched.len(),
            quiet
        );
        if !unmatched.is_empty() {
            let _ = writeln!(
                out,
                "coverage: UNMATCHED curated entries (typo or missing from graph): {}",
                brief(&unmatched, 15)
            );
        }
    }
    if !idl.is_empty() {
        let matched = idl.iter().filter(|e| is_match(e)).count();
        let _ = writeln!(
            out,
            "coverage: IDL-mined entries {} requested, {matched} name-matched \
             (handler-shape guard applies after this)",
            idl.len()
        );
    }

    // Spec names that name ZERO call sites: that source/sink cannot fire on
    // this graph no matter what the dataflow does. Aggregated across rules.
    let mut dead_sources: Vec<&str> = Vec::new();
    let mut dead_sinks: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for rule in &pack.rules {
        for (list, dead) in
            [(&rule.sources, &mut dead_sources), (&rule.sinks, &mut dead_sinks)]
        {
            for raw in list.iter() {
                // Pseudo-sinks (`<shellform>`) and assignment sinks
                // (`=account`) match SHAPES/store keys, not call names —
                // no call site is ever named after them.
                if raw.starts_with('<') || raw.starts_with('=') {
                    continue;
                }
                // A `recv.Name` spec entry can fire two ways: by simple name
                // (ts engine: trailing member) or by the dotted spelling
                // verbatim (C frontend: full callee text). Live if either
                // names a call site.
                let stripped = raw.strip_prefix("::").unwrap_or(raw);
                let stripped = stripped.split_once('@').map_or(stripped, |(n, _)| n);
                let name = normalize_spec_name(raw);
                let live = !cpg.calls_named(name).is_empty()
                    || (stripped != name && !cpg.calls_named(stripped).is_empty());
                if seen.insert(name) && !live {
                    dead.push(name);
                }
            }
        }
    }
    if !dead_sources.is_empty() {
        dead_sources.sort_unstable();
        let _ = writeln!(
            out,
            "coverage: sources with zero call sites: {}",
            brief(&dead_sources, 15)
        );
    }
    if !dead_sinks.is_empty() {
        dead_sinks.sort_unstable();
        let _ =
            writeln!(out, "coverage: sinks with zero call sites: {}", brief(&dead_sinks, 15));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rule;
    use cpg_core::CpgBuilder;

    fn rule(sources: &[&str], sinks: &[&str]) -> Rule {
        Rule {
            id: "T-1".into(),
            name: "t".into(),
            description: String::new(),
            severity: "high".into(),
            cwe: None,
            sources: sources.iter().map(|s| s.to_string()).collect(),
            sinks: sinks.iter().map(|s| s.to_string()).collect(),
            sanitizers: vec![],
            entry_methods: vec![],
            source_idents: vec![],
            authz: vec![],
            confiners: vec![],
        }
    }

    #[test]
    fn normalizes_every_spec_spelling() {
        assert_eq!(normalize_spec_name("read@out1"), "read");
        assert_eq!(normalize_spec_name("Read@out*"), "Read");
        assert_eq!(normalize_spec_name("ExecContext@1"), "ExecContext");
        assert_eq!(normalize_spec_name("::system"), "system");
        assert_eq!(normalize_spec_name("Invoke@recv"), "Invoke");
        assert_eq!(normalize_spec_name("getenv"), "getenv");
        assert_eq!(normalize_spec_name("os.Create@0"), "Create");
        assert_eq!(normalize_spec_name("yaml.load@0"), "load");
    }

    #[test]
    fn reports_unmatched_entries_and_dead_spec_names() {
        let mut cpg = Cpg::new();
        let f = cpg.file_id("a.go");
        let mut b = CpgBuilder::new(&mut cpg, f);
        let m = b.method("Handle", "Svc::Handle", "", Some(1));
        let c = b.call("Query", "Query(x)", Some(2));
        b.contains(m, c);
        let pack = RulePack { rules: vec![rule(&["getenv"], &["Query@1", "Exec"])], entry_globs: vec![] };
        let report = coverage_report(
            &cpg,
            &["Handle".to_string(), "NoSuchEntry".to_string()],
            &[],
            &pack,
            &HashSet::new(),
        );
        assert!(report.contains("2 requested, 1 matched"), "{report}");
        assert!(report.contains("UNMATCHED curated entries"), "{report}");
        assert!(report.contains("NoSuchEntry"), "{report}");
        // Query resolves via @1 normalization and exists; Exec and getenv do not.
        assert!(report.contains("sources with zero call sites: getenv"), "{report}");
        assert!(report.contains("sinks with zero call sites: Exec"), "{report}");
        assert!(!report.contains("Query,"), "{report}");
    }
}
