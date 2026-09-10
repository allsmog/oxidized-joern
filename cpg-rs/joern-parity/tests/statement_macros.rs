//! Complete pinned graphs and production findings for statement macro expansion.
use cpg_core::{Cpg, Query};

fn build(source: &str) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("main.c", source)]);
    project.cpg
}

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                "fixtures/statement-macros/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/statement-macros/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}
const CASES: &[(&str, &str, &str)] = &[
    fixture!("array_declaration"),
    fixture!("brace_assign"),
    fixture!("brace_declarations"),
    fixture!("brace_local"),
    fixture!("brace_multi"),
    fixture!("braceless_do"),
    fixture!("do_assign"),
    fixture!("do_local"),
    fixture!("do_nested"),
    fixture!("empty_block"),
    fixture!("flow_brace"),
    fixture!("for_empty"),
    fixture!("for_expression"),
    fixture!("for_statement"),
    fixture!("if_statement"),
    fixture!("lua_nested"),
    fixture!("lua_struct"),
    fixture!("macro_flows"),
    fixture!("nested_blocks"),
    fixture!("nested_macro"),
    fixture!("object_compound"),
    fixture!("pointer_qualifier"),
    fixture!("return_statement"),
    fixture!("shadow_after"),
    fixture!("source_lines"),
    fixture!("while_assign"),
];

#[test]
fn statement_macros_match_complete_isolated_live_graphs() {
    for &(name, source, expected) in CASES {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}"
        );
    }
}

#[test]
fn macro_comments_do_not_create_file_scope_declarations() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(include_str!(
            "fixtures/statement-macros/diagnostics/block_comments/main.c"
        ))),
        include_str!("fixtures/statement-macros/diagnostics/block_comments/expected.txt"),
    );
}

#[test]
fn statement_macro_findings_match_all_live_positive_and_negative_outcomes() {
    let cpg = build(include_str!(
        "fixtures/statement-macros/cases/macro_flows/main.c"
    ));
    let mut summaries = cpg_analysis::SummaryStore::new();
    summaries.compute_all(&cpg);
    let findings = cpg_analysis::find_flows(
        &cpg,
        &summaries,
        &cpg_analysis::TaintSpec::new(&["source"], &["sink"]),
    );
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
                findings.iter().any(|finding| finding.method == entry),
            )
        })
        .collect();
    assert_eq!(
        actual,
        include_str!("fixtures/statement-macros/flows/outcomes.txt")
    );
}

#[test]
fn generated_statement_nodes_remain_at_their_own_invocation() {
    let cpg = build(include_str!(
        "fixtures/statement-macros/cases/source_lines/main.c"
    ));
    for (name, invocation_line, following_line) in [("z_first", 4, 5), ("a_second", 8, 9)] {
        let method = cpg.method_named(name)[0];
        let descendants = cpg_analysis::pass::ast_descendants(&cpg, method);
        let invocation = *descendants
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("SET"))
            .unwrap();
        assert_eq!(cpg.line_of(invocation), Some(invocation_line), "{name}");
        for node in cpg_analysis::pass::ast_descendants(&cpg, invocation) {
            assert_eq!(
                cpg.line_of(node),
                Some(invocation_line),
                "{name}: {:?} {:?}",
                cpg.name_of(node),
                cpg.code_of(node)
            );
        }
        let following = *descendants
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("sink"))
            .unwrap();
        assert_eq!(cpg.line_of(following), Some(following_line), "{name}");
    }
}
