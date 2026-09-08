use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_frontend::Frontend;

#[test]
fn canonical_c_braceless_if_preserves_findings_and_sanitizers() {
    let source = r#"
char *source(void);
char *clean(void);
char *sanitize(char *value);
void sink(char *value);

void conditional_call(int condition) {
    if (condition)
        sink(source());
}

void cleaned_conditional_call(int condition) {
    if (condition)
        sink(sanitize(source()));
}

char *conditional_return(int condition) {
    if (condition)
        return source();
    return clean();
}

void returned(int condition) { sink(conditional_return(condition)); }
"#;
    let mut frontend = cpg_lang_c::CFrontend::new();
    let mut cpg = frontend
        .build_project(&[("braceless_if.c", source)])
        .expect("C uses the canonical project frontend");
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
        .map(|finding| finding.method)
        .collect();
    hits.sort();
    hits.dedup();
    assert_eq!(hits, ["conditional_call", "returned"]);
}
