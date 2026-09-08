use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_core::{Cpg, Query};
use cpg_frontend::Frontend;

fn build(source: &str) -> (Cpg, SummaryStore) {
    let mut cpg = cpg_lang_c::CFrontend::new()
        .build_project(&[("blocks.c", source)])
        .unwrap();
    let files = cpg.files();
    standard_pipeline().run_all(&mut cpg, &files, &Default::default());
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    (cpg, summaries)
}

#[test]
fn standalone_block_findings_match_live_joern_outcomes() {
    let (cpg, summaries) = build(include_str!(
        "../../joern-parity/tests/fixtures/standalone-blocks/loop-tail-diagnostic/blocks.c"
    ));
    let findings = find_flows(&cpg, &summaries, &TaintSpec::new(&["source"], &["sink"]));
    let mut entries: Vec<_> = cpg
        .methods()
        .into_iter()
        .filter_map(|method| cpg.name_of(method))
        .filter(|name| name.ends_with("_entry"))
        .collect();
    entries.sort();
    let actual: String = entries
        .into_iter()
        .map(|entry| {
            format!(
                "RESULT|{entry}|{}\n",
                findings.iter().any(|finding| finding.method == entry)
            )
        })
        .collect();
    assert_eq!(
        actual,
        include_str!(
            "../../joern-parity/tests/fixtures/standalone-blocks/loop-tail-diagnostic/outcomes.txt"
        )
    );
    let bare = findings
        .iter()
        .find(|finding| finding.method == "bare_entry")
        .unwrap();
    assert_eq!(bare.path.first().unwrap().code, "source()");
    assert_eq!(bare.path.last().unwrap().code, "sink(source())");
    assert_eq!(bare.sink_line, Some(5));
}

#[test]
fn standalone_blocks_keep_definite_kills_and_sanitizer_cuts() {
    let source = r#"
char *source(void);
char *sanitize(char *value);
void sink(char *value);
void optional(int condition) {
    char *value = source();
    { if (condition) { value = "safe"; } }
    sink(value);
}
void definite(void) {
    char *value = source();
    { { value = "safe"; } }
    sink(value);
}
void cleaned(void) {
    char *value = source();
    { value = sanitize(value); }
    sink(value);
}
void nested_cleaned(void) { { sink(sanitize(source())); } }
"#;
    let (cpg, mut summaries) = build(source);
    for stored in [false, true] {
        if stored {
            summaries.set_sanitizers(["sanitize"]);
            summaries.compute_all(&cpg);
        }
        let sanitizers: &[&str] = if stored { &[] } else { &["sanitize"] };
        let findings = find_flows(
            &cpg,
            &summaries,
            &TaintSpec::with_sanitizers(&["source"], &["sink"], sanitizers),
        );
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.method.as_str())
                .collect::<Vec<_>>(),
            ["optional"],
            "stored={stored}: {findings:?}"
        );
    }
}
