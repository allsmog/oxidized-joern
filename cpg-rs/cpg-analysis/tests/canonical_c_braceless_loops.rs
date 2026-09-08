use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_frontend::Frontend;

#[test]
fn braceless_loops_preserve_findings_sanitizers_and_call_locations() {
    let source = r#"
char *source(void);
char *sanitize(char *value);
void sink(char *value);
void while_hit(int count) {
    while (count)
        sink(source());
}
void do_hit(int count) {
    do
        sink(source());
    while (count);
}
void for_hit(int count) {
    for (; count; count--)
        sink(source());
}
void while_clean(int count) { while (count) sink(sanitize(source())); }
void do_clean(int count) { do sink(sanitize(source())); while (count); }
void for_clean(int count) { for (; count; count--) sink(sanitize(source())); }
"#;
    let mut cpg = cpg_lang_c::CFrontend::new()
        .build_project(&[("loops.c", source)])
        .unwrap();
    let methods = cpg_analysis::pass::method_name_index(&cpg);
    let context = cpg_analysis::PassContext {
        methods_by_name: Some(&methods),
    };
    let files = cpg.files();
    standard_pipeline().run_all(&mut cpg, &files, &context);
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    let spec = TaintSpec::with_sanitizers(&["source"], &["sink"], &["sanitize"]);
    let mut hits: Vec<_> = find_flows(&cpg, &summaries, &spec)
        .into_iter()
        .map(|finding| (finding.method, finding.sink_line))
        .collect();
    hits.sort();
    hits.dedup();
    assert_eq!(
        hits,
        [
            ("do_hit".to_string(), Some(11)),
            ("for_hit".to_string(), Some(16)),
            ("while_hit".to_string(), Some(7))
        ]
    );
}
