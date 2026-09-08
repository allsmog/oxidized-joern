use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_frontend::Frontend;

#[test]
fn conditional_methods_keep_source_locations_and_active_scanner_findings() {
    let source = "char *source(void);\nvoid sink(char *value);\n#if 1\nvoid active_entry(void) { sink(source()); }\n#else\nvoid inactive_entry(void) { sink(source()); }\n#endif\nvoid plain_entry(void) { sink(source()); }\n";
    let mut cpg = cpg_lang_c::CFrontend::new()
        .build_project(&[("conditional_scan.c", source)])
        .expect("canonical C graph");
    let methods = cpg_analysis::pass::method_name_index(&cpg);
    let context = cpg_analysis::PassContext {
        methods_by_name: Some(&methods),
    };
    let files = cpg.files();
    standard_pipeline().run_all(&mut cpg, &files, &context);
    for (name, line) in [
        ("active_entry", 4),
        ("inactive_entry", 6),
        ("plain_entry", 8),
    ] {
        let method = cpg
            .nodes()
            .find(|&node| {
                cpg.kind_of(node) == cpg_core::NodeKind::Method && cpg.name_of(node) == Some(name)
            })
            .unwrap();
        assert_eq!(cpg.line_of(method), Some(line), "{name}");
    }
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
    let spec = TaintSpec::with_sanitizers(&["source"], &["sink"], &[]);
    let mut methods: Vec<_> = find_flows(&cpg, &summaries, &spec)
        .into_iter()
        .map(|finding| finding.method)
        .collect();
    methods.sort();
    methods.dedup();
    assert_eq!(methods, ["active_entry", "plain_entry"]);
}
