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

    for (file, call_name, expected, definition) in [
        (
            "a.c",
            "helper",
            "helper",
            "static int helper(int x) { return x; }",
        ),
        (
            "b.c",
            "helper<duplicate>0",
            "helper<duplicate>0",
            "static int helper(int x) { return x + 1; }",
        ),
    ] {
        let call = cpg
            .calls_named(call_name)
            .into_iter()
            .find(|&node| cpg.path_of(cpg.file_of(node)) == Some(file))
            .unwrap_or_else(|| panic!("missing helper call in {file}"));
        let targets = cpg.call_targets(call);
        assert_eq!(targets.len(), 1, "{file} targets: {targets:?}");
        assert_eq!(cpg.full_name_of(targets[0]), Some(expected));
        assert_eq!(cpg.path_of(cpg.file_of(targets[0])), Some(file));
        assert_eq!(cpg.code_of(targets[0]), Some(definition));
        assert_eq!(cpg.line_of(targets[0]), Some(1));
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

#[test]
fn canonical_c_summary_preserves_parameter_on_optional_overwrite() {
    let source = r#"
char *source(void);
void sink(char *value);
char *wrap(char *value, int condition) {
    if (condition) { value = "safe"; }
    return value;
}

void entry(int condition) { sink(wrap(source(), condition)); }
"#;
    let (cpg, summaries) = build_exact(&[("optional_overwrite.c", source)]);
    let summary = summaries.get("wrap").expect("wrap summary");
    let hits = method_hits(&cpg, &summaries, &[]);
    assert!(
        summary.flows_to_return().any(|index| index == 0),
        "the unchanged branch returns parameter 0: {summary:?}; findings: {hits:?}"
    );
    assert!(hits.iter().any(|method| method == "entry"), "{hits:?}");
}

#[test]
fn canonical_c_return_dependencies_distinguish_may_flow_from_definite_kills() {
    // The first five control-flow outcomes were also checked with live
    // Joern v4.0.555 reachableBy over the same source/sink call pattern.
    let cases = [
        ("optional", "if (condition) { value = \"safe\"; } return value;", true),
        ("definite", "value = \"safe\"; return value;", false),
        ("loop_optional", "while (condition) { value = \"safe\"; condition = 0; } return value;", true),
        ("both_branches", "if (condition) { value = \"safe\"; } else { value = \"also safe\"; } return value;", false),
        ("alternative", "char *out = \"safe\"; if (condition) { out = value; } else { out = \"safe\"; } return out;", true),
        ("do_definite", "do { value = \"safe\"; } while (condition); return value;", false),
        ("loop_carried", "char *out = \"safe\"; while (condition) { out = value; condition = 0; } return out;", true),
        ("exit_is_not_return", "char *out = value; return \"safe\";", false),
        ("named_call_kill", "return replace(value);", false),
    ];
    for (name, body, expected) in cases {
        let source = format!(
            "char *source(void);\nvoid sink(char *value);\n\
             char *replace(char *value) {{ return \"safe\"; }}\n\
             char *wrap(char *value, int condition) {{ {body} }}\n\
             void entry(int condition) {{ sink(wrap(source(), condition)); }}\n"
        );
        let (cpg, summaries) = build_exact(&[("return_dependencies.c", &source)]);
        let summary = summaries.get("wrap").expect("wrap summary");
        let hits = method_hits(&cpg, &summaries, &[]);
        assert_eq!(
            summary.flows_to_return().any(|index| index == 0),
            expected,
            "{name}: {summary:?}"
        );
        assert_eq!(
            hits.iter().any(|method| method == "entry"),
            expected,
            "{name}: {hits:?}"
        );
    }
}

#[test]
fn canonical_c_return_dependencies_preserve_optional_source_results() {
    for (assignment, expected) in [
        ("if (condition) { value = \"safe\"; }", true),
        ("value = \"safe\";", false),
    ] {
        let source = format!(
            "char *source(void);\nvoid sink(char *value);\n\
             char *wrap(int condition) {{\n\
                 char *value = source();\n\
                 {assignment}\n\
                 return value;\n\
             }}\n\
             void entry(int condition) {{ sink(wrap(condition)); }}\n"
        );
        let (cpg, summaries) = build_exact(&[("source_returns.c", &source)]);
        let summary = summaries.get("wrap").expect("wrap summary");
        assert_eq!(
            summary.raw_call_returns().any(|name| name == "source"),
            expected
        );
        let hits = method_hits(&cpg, &summaries, &[]);
        assert_eq!(
            hits.iter().any(|method| method == "entry"),
            expected,
            "{hits:?}"
        );
    }
}

#[test]
fn canonical_c_return_dependencies_keep_sanitizer_and_external_summary_policy() {
    let source = r#"
char *source(void);
char *external(char *value);
void sink(char *value);
char *sanitize(char *value) { return value; }
char *cleaned(char *value) { return sanitize(value); }
char *optional_clean(char *value, int condition) {
    if (condition) { value = sanitize(value); }
    return value;
}
char *modeled(char *value) { return external(value); }
void cleaned_entry(void) { sink(cleaned(source())); }
void optional_entry(int condition) { sink(optional_clean(source(), condition)); }
void external_entry(void) { sink(modeled(source())); }
"#;
    let (cpg, mut summaries) = build_exact(&[("sanitized_returns.c", source)]);
    summaries
        .load_external_json(
            r#"[{"functionDeclaration":{"language":"C","methodName":"external"},
             "dataFlows":[{"from":"param0","to":"return"}]}]"#,
        )
        .unwrap();
    summaries.compute_all(&cpg);

    // Sanitizer policy added only at query time must cut nested summary hops.
    let hits = method_hits(&cpg, &summaries, &["sanitize"]);
    assert!(
        !hits.iter().any(|method| method == "cleaned_entry"),
        "{hits:?}"
    );
    assert!(
        hits.iter().any(|method| method == "optional_entry"),
        "{hits:?}"
    );
    assert!(
        hits.iter().any(|method| method == "external_entry"),
        "{hits:?}"
    );

    summaries.set_sanitizers(["sanitize"]);
    summaries.compute_all(&cpg);
    let cleaned = summaries.get("cleaned").expect("cleaned summary");
    assert_eq!(cleaned.flows_to_return().count(), 0);
    assert!(cleaned
        .sanitized_flows_to_return()
        .any(|(index, via)| index == 0 && via.name() == "sanitize"));
    let optional = summaries.get("optional_clean").expect("optional summary");
    assert!(optional.flows_to_return().any(|index| index == 0));
    let hits = method_hits(&cpg, &summaries, &[]);
    assert!(
        !hits.iter().any(|method| method == "cleaned_entry"),
        "{hits:?}"
    );
    assert!(
        hits.iter().any(|method| method == "optional_entry"),
        "{hits:?}"
    );
    assert!(
        hits.iter().any(|method| method == "external_entry"),
        "{hits:?}"
    );
}

#[test]
fn canonical_c_matches_live_joern_return_flow_outcomes() {
    let source = include_str!("fixtures/return-flow/return_flows.c");
    let expected = include_str!("fixtures/return-flow/expected.txt");
    let (cpg, summaries) = build_exact(&[("return_flows.c", source)]);
    let hits = method_hits(&cpg, &summaries, &[]);
    let mut entries: Vec<_> = cpg
        .methods()
        .into_iter()
        .filter_map(|method| cpg.name_of(method))
        .filter(|name| name.ends_with("_entry"))
        .collect();
    entries.sort();
    let actual: String = entries
        .into_iter()
        .map(|entry| format!("RESULT|{entry}|{}\n", hits.iter().any(|hit| hit == entry)))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn merging_c_preserves_generic_accessor_summaries() {
    let mut frontend = cpg_lang_ts::TsFrontend::java();
    let mut generic = Cpg::new();
    frontend.build_file(
        &mut generic,
        "Generic.java",
        "class Box { String value; } class Generic { String get(Box box) { return box.value; } }",
    );
    let files = generic.files();
    standard_pipeline().run_all(&mut generic, &files, &Default::default());
    let method = generic.method_named("get")[0];
    let fqn = generic.full_name_of(method).unwrap().to_string();
    let mut before = SummaryStore::new();
    before.compute_all(&generic);
    assert!(before
        .get(&fqn)
        .unwrap()
        .flows_to_return()
        .any(|index| index == 0));

    let (mut merged, _) = build_exact(&[("native.c", "int native(int value) { return value; }")]);
    merged.absorb(generic);
    let mut after = SummaryStore::new();
    after.compute_all(&merged);
    assert_eq!(
        after.get(&fqn).unwrap().flows,
        before.get(&fqn).unwrap().flows
    );
}
