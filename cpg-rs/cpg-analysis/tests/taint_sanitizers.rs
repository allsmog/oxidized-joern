use cpg_analysis::{find_flows, SummaryStore, TaintSpec};
use cpg_core::{Cpg, CpgBuilder};

fn source_clean_sink_graph() -> Cpg {
    let mut cpg = Cpg::new();
    let file_id = cpg.file_id("v.c");
    {
        let mut b = CpgBuilder::new(&mut cpg, file_id);
        let file = b.file_node("v.c");
        let method = b.method("run", "run", "void(void)", Some(1));
        b.contains(file, method);

        let sink = b.call("sink", "sink(clean(source()))", Some(2));
        let clean = b.call("clean", "clean(source())", Some(2));
        let source = b.call("source", "source()", Some(2));
        b.ast_child(method, sink);
        b.add_argument(sink, clean, 1);
        b.add_argument(clean, source, 1);
    }
    cpg
}

fn summaries_with_clean_passthrough() -> SummaryStore {
    let mut summaries = SummaryStore::new();
    summaries
        .load_external_json(
            r#"[{"functionDeclaration":{"language":"C","methodName":"clean"},
                 "dataFlows":[{"from":"param0","to":"return"}]}]"#,
        )
        .expect("external clean summary loads");
    summaries
}

#[test]
fn explicit_sanitizer_blocks_source_to_sink() {
    let cpg = source_clean_sink_graph();
    let summaries = summaries_with_clean_passthrough();

    let unsanitized = TaintSpec::new(&["source"], &["sink"]);
    assert_eq!(find_flows(&cpg, &summaries, &unsanitized).len(), 1);

    let sanitized = TaintSpec::with_sanitizers(&["source"], &["sink"], &["clean"]);
    assert!(find_flows(&cpg, &summaries, &sanitized).is_empty());
}
