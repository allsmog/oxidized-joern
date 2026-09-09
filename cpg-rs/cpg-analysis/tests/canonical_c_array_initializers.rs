use cpg_analysis::{find_flows, standard_pipeline, SummaryStore, TaintSpec};
use cpg_core::Query;
use cpg_frontend::Frontend;

#[test]
fn array_initializer_findings_match_live_joern_outcomes() {
    let mut cpg = cpg_lang_c::CFrontend::new()
        .build_project(&[(
            "arrays.c",
            include_str!(
                "../../joern-parity/tests/fixtures/array-initializers/cases/array_flows/arrays.c"
            ),
        )])
        .unwrap();
    let files = cpg.files();
    standard_pipeline().run_all(&mut cpg, &files, &Default::default());
    let mut summaries = SummaryStore::new();
    summaries.compute_all(&cpg);
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
            "../../joern-parity/tests/fixtures/array-initializers/cases/array_flows/outcomes.txt"
        )
    );
}
