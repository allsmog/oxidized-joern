//! Direct source-to-sink outcomes measured against pinned Joern v4.0.555.
use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_core::{Cpg, Query};
use cpg_frontend::Frontend;

fn build(source: &str) -> (Cpg, SummaryStore) {
    let mut frontend = cpg_lang_c::CFrontend::new();
    let mut cpg = frontend
        .build_project(&[("direct_flows.c", source)])
        .unwrap();
    let files = cpg.files();
    standard_pipeline().run_all(&mut cpg, &files, &Default::default());
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    (cpg, summaries)
}

#[test]
fn direct_c_matches_live_joern_outcomes() {
    let (cpg, summaries) = build(include_str!("fixtures/direct-flow/direct_flows.c"));
    let spec = TaintSpec::new(&["source"], &["sink"]);
    let findings = find_flows(&cpg, &summaries, &spec);
    let mut entries: Vec<_> = cpg
        .methods()
        .into_iter()
        .filter_map(|m| cpg.name_of(m))
        .filter(|n| n.ends_with("_entry"))
        .collect();
    entries.sort();
    let actual: String = entries
        .into_iter()
        .map(|entry| {
            format!(
                "RESULT|{entry}|{}\n",
                findings.iter().any(|f| f.method == entry)
            )
        })
        .collect();
    assert_eq!(actual, include_str!("fixtures/direct-flow/expected.txt"));
}

#[test]
fn direct_c_keeps_query_and_summary_sanitizer_cuts() {
    let source = r#"
char *source(void);
char *sanitize(char *value);
void sink(char *value);
char *identity(char *value) { return value; }
char *cleaned(char *value) { return identity(sanitize(value)); }
void optional(int condition) {
    char *value = source();
    if (condition) { value = sanitize(value); }
    sink(value);
}
void sanitizer_named_local(int condition) {
    char *sanitize = source();
    if (condition) { sanitize = "safe"; }
    sink(sanitize);
}
void all_branches(int condition) {
    char *value = source();
    if (condition) { value = sanitize(value); } else { value = "safe"; }
    sink(value);
}
void definite(void) { char *value = source(); value = sanitize(value); sink(value); }
void nested(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    sink(identity(cleaned(value)));
}
void receive(char *value, int condition) {
    if (condition) { value = sanitize(value); } else { value = "safe"; }
    sink(value);
}
void handoff(int condition) { receive(source(), condition); }
"#;
    let (cpg, mut summaries) = build(source);
    for store_policy in [false, true] {
        if store_policy {
            summaries.set_sanitizers(["sanitize"]);
            summaries.compute_all(&cpg);
        }
        let sanitizers: &[&str] = if store_policy { &[] } else { &["sanitize"] };
        let spec = TaintSpec::with_sanitizers(&["source"], &["sink"], sanitizers);
        let findings = find_flows(&cpg, &summaries, &spec);
        let mut methods: Vec<_> = findings.iter().map(|f| f.method.as_str()).collect();
        methods.sort();
        assert_eq!(
            methods,
            vec!["optional", "sanitizer_named_local"],
            "store_policy={store_policy}: {findings:?}"
        );
        assert!(findings[0]
            .path
            .iter()
            .all(|step| !step.code.contains("sanitize(")));
    }
}

#[test]
fn direct_c_witness_uses_live_definition_and_splices_summary_hops() {
    let (cpg, summaries) = build(
        r#"
char *source(void);
void sink(char *value);
char *identity(char *value) { return value; }
void entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    sink(identity(value));
}
"#,
    );
    let spec = TaintSpec::new(&["source"], &["sink"]);
    let findings = find_flows(&cpg, &summaries, &spec);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let path = &findings[0].path;
    assert_eq!(path.first().unwrap().code, "source()");
    assert_eq!(path.last().unwrap().code, "sink(identity(value))");
    assert!(
        path.iter().all(|step| !step.code.contains("\"safe\"")),
        "{path:?}"
    );
    assert!(
        path.iter().any(|step| matches!(&step.provenance,
        cpg_analysis::Provenance::SummaryFlow { callee_fqn } if callee_fqn == "identity")),
        "{path:?}"
    );
    assert!(
        path.iter()
            .any(|step| step.depth == 1 && step.code == "return value;"),
        "{path:?}"
    );
    assert_eq!(find_flows(&cpg, &summaries, &spec), findings);
}

#[test]
fn direct_c_does_not_cross_method_or_translation_unit_names() {
    let mut frontend = cpg_lang_c::CFrontend::new();
    let cpg = frontend
        .build_project(&[
            (
                "producer.c",
                "char *source(void); static void entry(void) { char *value=source(); }",
            ),
            (
                "consumer.c",
                "void sink(char *); static void entry(char *value) { sink(value); }",
            ),
        ])
        .unwrap();
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    assert!(find_flows(&cpg, &summaries, &TaintSpec::new(&["source"], &["sink"])).is_empty());
}

#[test]
fn merging_c_keeps_generic_finding_generation_unchanged() {
    let mut frontend = cpg_lang_ts::TsFrontend::python();
    let mut generic = Cpg::new();
    frontend.build_file(
        &mut generic,
        "generic.py",
        "def run():\n    value = source()\n    sink(value)\n",
    );
    let files = generic.files();
    standard_pipeline().run_all(&mut generic, &files, &Default::default());
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&generic);
    let spec = TaintSpec::new(&["source"], &["sink"]);
    let before = find_flows(&generic, &summaries, &spec);
    assert_eq!(before.len(), 1, "{before:?}");
    let (mut merged, _) = build("int native(int value) { return value; }");
    merged.absorb(generic);
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&merged);
    assert_eq!(find_flows(&merged, &summaries, &spec), before);
}

#[test]
fn recursive_entry_queries_keep_distinct_ancestor_contexts() {
    let (cpg, summaries) = build(
        r#"
void sink(char *value);
void left_entry(char *value) { left(value); }
void right_entry(char *value) { right(value); }
void left(char *value) { common(value); sink(value); }
void right(char *value) { common(value); }
void common(char *value) { left(value); }
"#,
    );
    let mut spec = TaintSpec::new(&[], &["sink"]);
    spec.source_methods = ["left_entry", "right_entry"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let findings = find_flows(&cpg, &summaries, &spec);
    let mut methods: Vec<_> = findings
        .iter()
        .map(|finding| finding.method.as_str())
        .collect();
    methods.sort();
    assert_eq!(methods, ["left_entry", "right_entry"], "{findings:?}");
    // Both visits to common have depth 2. Under left_entry, left is already
    // an ancestor so common cannot revisit it. Under right_entry it can,
    // and that newly available path must not reuse the earlier negative.
    let right = findings
        .iter()
        .find(|finding| finding.method == "right_entry")
        .unwrap();
    assert_eq!(right.path.last().unwrap().code, "sink(value)");
    assert_eq!(right.path.last().unwrap().depth, 3);
    assert_eq!(find_flows(&cpg, &summaries, &spec), findings);
}

#[test]
fn repeated_handoffs_keep_witnesses_and_definite_kills() {
    let mut source = String::from("char *source(void);\nvoid sink(char *value);\n");
    source.push_str("void level7(char *value) { sink(value); }\n");
    for level in (0..7).rev() {
        source.push_str(&format!("void level{level}(char *value) {{\n"));
        for _ in 0..4 {
            source.push_str(&format!("level{}(value);\n", level + 1));
        }
        source.push_str("}\n");
    }
    source.push_str("void entry(void) { level0(source()); }\n");
    source.push_str("void killed(void) { char *value=source(); value=\"safe\"; level0(value); }\n");
    let (cpg, summaries) = build(&source);
    let spec = TaintSpec::new(&["source"], &["sink"]);
    let findings = find_flows(&cpg, &summaries, &spec);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].method, "entry");
    assert_eq!(findings[0].path.last().unwrap().code, "sink(value)");
    assert_eq!(findings[0].path.last().unwrap().depth, 8);
    assert_eq!(find_flows(&cpg, &summaries, &spec), findings);
}

#[test]
fn parameter_path_keeps_priority_over_unrelated_policy_source() {
    let (cpg, summaries) = build(
        r#"
char *source(void);
void read_value(char **value);
void first_sink(char *value);
void second_sink(char *value);
void receive(char *value) {
    char *other;
    read_value(&other);
    first_sink(other);
    second_sink(value);
}
void entry(void) { receive(source()); }
"#,
    );
    let mut spec = TaintSpec::new(&["source"], &["first_sink", "second_sink"]);
    spec.out_param_sources.insert("read_value".into(), 0);
    let findings = find_flows(&cpg, &summaries, &spec);
    let mut hits: Vec<_> = findings
        .iter()
        .map(|finding| (finding.method.as_str(), finding.sink.as_str()))
        .collect();
    hits.sort();
    assert_eq!(
        hits,
        [("entry", "second_sink"), ("receive", "first_sink")],
        "{findings:?}"
    );
}

#[test]
fn repeated_queries_do_not_reuse_sanitizer_cuts() {
    let (cpg, summaries) = build(
        r#"
char *source(void);
void sink(char *value);
char *identity(char *value) { return value; }
void receive(char *value) { sink(identity(value)); }
void entry(void) { receive(source()); }
"#,
    );
    let clean = TaintSpec::with_sanitizers(&["source"], &["sink"], &["identity"]);
    let raw = TaintSpec::new(&["source"], &["sink"]);
    assert!(find_flows(&cpg, &summaries, &clean).is_empty());
    let findings = find_flows(&cpg, &summaries, &raw);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].method, "entry");
    assert!(find_flows(&cpg, &summaries, &clean).is_empty());
    assert_eq!(find_flows(&cpg, &summaries, &raw), findings);
}
