//! External-API inventory (`cpg apis`) — the input to IRIS-style LLM taint
//! spec inference (arXiv:2405.17238).
//!
//! An "external API" is a called name with no defining method body in the
//! CPG: the standard library, third-party dependencies, and generated stubs.
//! Those are exactly the functions a taint analysis needs specifications
//! for, and exactly what IRIS has an LLM label as source/sink/sanitizer per
//! CWE. The inventory groups calls by name, reconstructs the qualified
//! prefix from call-site text (`exec.Command(`, `folly::Subprocess(`), and
//! carries enough context — arity, receiver-type hints, example call sites —
//! for a model to label each API without opening the repository.

use cpg_core::{Cpg, NodeId, Query};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize)]
pub struct ApiEntry {
    /// Simple callable name — the key taint specs match on.
    pub name: String,
    /// Qualified spellings seen at call sites (`os/exec.Command`,
    /// `folly::gen::from`), most frequent first. Empty for bare calls.
    pub qualified: Vec<String>,
    /// Receiver-type hints observed on the call sites (from local type
    /// inference), most frequent first.
    pub receiver_types: Vec<String>,
    /// Distinct argument counts observed.
    pub arities: Vec<usize>,
    /// Number of call sites.
    pub count: usize,
    /// Up to `max_examples` example call sites.
    pub examples: Vec<ApiExample>,
}

#[derive(Serialize)]
pub struct ApiExample {
    pub file: String,
    pub line: Option<u32>,
    pub code: String,
}

/// The qualified spelling of a call, recovered from its source text: the
/// prefix up to the opening parenthesis, if it contains a qualifier
/// (`a::b`, `a.b`) and looks like a plain callee (no spaces/operators).
fn qualified_spelling(code: &str, name: &str) -> Option<String> {
    let head = code.split('(').next()?.trim();
    if !head.ends_with(name) || head == name {
        return None;
    }
    if head.contains(' ') || head.contains('\n') {
        return None;
    }
    Some(head.to_string())
}

/// Build the inventory. `internal` is the set of names with a defining
/// method in the CPG; calls resolving to them are project code, not APIs.
pub fn inventory(cpg: &Cpg, max_examples: usize) -> Vec<ApiEntry> {
    let internal: HashSet<&str> = cpg
        .methods()
        .into_iter()
        .filter_map(|m| cpg.name_of(m))
        .collect();

    struct Acc {
        qualified: HashMap<String, usize>,
        recv: HashMap<String, usize>,
        arities: HashSet<usize>,
        count: usize,
        examples: Vec<(NodeId, String, Option<u32>, String)>,
    }
    let mut by_name: HashMap<String, Acc> = HashMap::new();

    for c in cpg.calls() {
        let Some(name) = cpg.name_of(c) else { continue };
        // Operators, assignments, and calls the project itself defines are
        // not external APIs.
        if name.len() <= 1
            || !name
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        {
            continue;
        }
        if internal.contains(name) {
            continue;
        }
        let acc = by_name.entry(name.to_string()).or_insert_with(|| Acc {
            qualified: HashMap::new(),
            recv: HashMap::new(),
            arities: HashSet::new(),
            count: 0,
            examples: Vec::new(),
        });
        acc.count += 1;
        acc.arities.insert(cpg.arguments_of(c).len());
        let code = cpg.code_of(c).unwrap_or("");
        if let Some(q) = qualified_spelling(code, name) {
            *acc.qualified.entry(q).or_insert(0) += 1;
        }
        if let Some(t) = cpg.type_full_name_of(c).filter(|t| !t.is_empty()) {
            *acc.recv.entry(t.to_string()).or_insert(0) += 1;
        }
        if acc.examples.len() < max_examples {
            let file = cpg.path_of(cpg.file_of(c)).unwrap_or("").to_string();
            acc.examples
                .push((c, file, cpg.line_of(c), code.to_string()));
        }
    }

    let mut entries: Vec<ApiEntry> = by_name
        .into_iter()
        .map(|(name, acc)| {
            let sort_desc = |m: HashMap<String, usize>| {
                let mut v: Vec<(String, usize)> = m.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                v.into_iter().map(|(k, _)| k).collect::<Vec<_>>()
            };
            let mut arities: Vec<usize> = acc.arities.into_iter().collect();
            arities.sort_unstable();
            ApiEntry {
                name,
                qualified: sort_desc(acc.qualified),
                receiver_types: sort_desc(acc.recv),
                arities,
                count: acc.count,
                examples: acc
                    .examples
                    .into_iter()
                    .map(|(_, file, line, code)| ApiExample {
                        file,
                        line,
                        code: code.chars().take(200).collect(),
                    })
                    .collect(),
            }
        })
        .collect();
    entries.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_spelling_extraction() {
        assert_eq!(
            qualified_spelling("exec.Command(\"sh\")", "Command"),
            Some("exec.Command".to_string())
        );
        assert_eq!(
            qualified_spelling("folly::gen::from(v)", "from"),
            Some("folly::gen::from".to_string())
        );
        assert_eq!(qualified_spelling("puts(x)", "puts"), None);
        assert_eq!(qualified_spelling("a + b(x)", "b"), None);
    }
}
