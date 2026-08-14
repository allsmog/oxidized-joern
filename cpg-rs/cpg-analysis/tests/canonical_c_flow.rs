//! Security-outcome acceptance over the shipped, parity-validated C graph.
//! These fixtures intentionally exercise final findings as well as graph
//! construction: a byte-exact graph gate alone cannot prove scanner policy.

use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_core::{Cpg, Query};
use cpg_frontend::Frontend;

fn build_exact(files: &[(&str, &str)]) -> (Cpg, SummaryStore) {
    let mut frontend = cpg_lang_c::CFrontend::new();
    let mut cpg = frontend
        .build_project(files)
        .expect("C uses the canonical project frontend");
    let methods = cpg_analysis::pass::method_name_index(&cpg);
    let context = cpg_analysis::PassContext {
        methods_by_name: Some(&methods),
    };
    let file_ids = cpg.files();
    standard_pipeline().run_all(&mut cpg, &file_ids, &context);
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    (cpg, summaries)
}

fn method_hits(cpg: &Cpg, summaries: &SummaryStore, sanitizers: &[&str]) -> Vec<String> {
    let spec = TaintSpec::with_sanitizers(&["source"], &["sink"], sanitizers);
    let mut methods: Vec<String> = find_flows(cpg, summaries, &spec)
        .into_iter()
        .map(|finding| finding.method)
        .collect();
    methods.sort();
    methods.dedup();
    methods
}

#[test]
fn canonical_c_findings_cover_control_flow_kills_members_and_returns() {
    let source = r#"
char *source(void);
char *clean(void);
char *sanitize(char *value);
void sink(char *value);

struct Box { char *value; };

void branch(int condition) {
    char *value = clean();
    if (condition) { value = source(); }
    sink(value);
}

void killed(void) {
    char *value = source();
    value = clean();
    sink(value);
}

void looped(int condition) {
    char *value = clean();
    while (condition) { value = source(); condition = 0; }
    sink(value);
}

char *wrapped(void) { return source(); }
void returned(void) { sink(wrapped()); }

void member(struct Box *box) {
    box->value = source();
    sink(box->value);
}

char *shared;
void set_global(void) { shared = source(); }
void global(void) { set_global(); sink(shared); }
void shadow(char *shared) { sink(shared); }

void cleaned(void) { sink(sanitize(source())); }
"#;
    let (cpg, summaries) = build_exact(&[("outcomes.c", source)]);
    let hits = method_hits(&cpg, &summaries, &["sanitize"]);
    for expected in ["branch", "global", "looped", "member", "returned"] {
        assert!(
            hits.iter().any(|method| method == expected),
            "missing {expected} from {hits:?}"
        );
    }
    assert!(!hits.iter().any(|method| method == "killed"), "{hits:?}");
    assert!(!hits.iter().any(|method| method == "cleaned"), "{hits:?}");
    assert!(!hits.iter().any(|method| method == "shadow"), "{hits:?}");
}

#[test]
fn duplicate_translation_unit_helpers_keep_distinct_call_targets() {
    let (cpg, _) = build_exact(&[
        (
            "a.c",
            "static int helper(int x) { return x; } int entry_a(void) { return helper(1); }",
        ),
        (
            "b.c",
            "static int helper(int x) { return x + 1; } int entry_b(void) { return helper(2); }",
        ),
    ]);

    for (file, expected) in [("a.c", "a.c:helper"), ("b.c", "b.c:helper")] {
        let call = cpg
            .calls_named("helper")
            .into_iter()
            .find(|&node| cpg.path_of(cpg.file_of(node)) == Some(file))
            .unwrap_or_else(|| panic!("missing helper call in {file}"));
        let targets = cpg.call_targets(call);
        assert_eq!(targets.len(), 1, "{file} targets: {targets:?}");
        assert_eq!(cpg.full_name_of(targets[0]), Some(expected));
    }
}

#[test]
fn canonical_c_findings_cross_calls_and_recursive_summaries() {
    let source = r#"
char *source(void);
void sink(char *value);

char *identity(char *value) { return value; }
void cross_call(void) { sink(identity(source())); }

char *odd(int count, char *value);
char *even(int count, char *value) {
    if (count == 0) { return value; }
    return odd(count - 1, value);
}
char *odd(int count, char *value) {
    if (count == 0) { return value; }
    return even(count - 1, value);
}
void recursive(void) { sink(even(2, source())); }
"#;
    let (cpg, summaries) = build_exact(&[("calls.c", source)]);
    let hits = method_hits(&cpg, &summaries, &[]);
    assert!(hits.iter().any(|method| method == "cross_call"), "{hits:?}");
    assert!(hits.iter().any(|method| method == "recursive"), "{hits:?}");
}

#[test]
fn canonical_flow_survives_save_and_load_with_authoritative_identity() {
    let source = "char *source(void); void sink(char *); void f(void) { sink(source()); }";
    let (cpg, summaries) = build_exact(&[("roundtrip.c", source)]);
    let before = method_hits(&cpg, &summaries, &[]);
    let reopened = Cpg::from_bytes(&cpg.to_bytes()).expect("reopen exact graph");
    let mut reopened_summaries = SummaryStore::new();
    reopened_summaries.compute_all(&reopened);
    let after = method_hits(&reopened, &reopened_summaries, &[]);
    assert_eq!(before, vec!["f"]);
    assert_eq!(after, before);
    assert!(reopened.calls().len() >= 2);
}
