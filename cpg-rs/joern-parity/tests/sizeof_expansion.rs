//! Complete pinned graphs for macro-expanded sizeof spelling.
use cpg_core::{NodeKind, Query};
fn build(source: &str) -> cpg_core::Cpg {
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
                "fixtures/sizeof-expansion/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/sizeof-expansion/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}
#[test]
fn expanded_sizeof_matches_complete_live_graphs() {
    for (name, source, expected) in [
        fixture!("initial_argument_value"),
        fixture!("initial_bare_value"),
        fixture!("initial_comment_value"),
        fixture!("initial_double_parentheses"),
        fixture!("initial_nested_function"),
        fixture!("initial_object_type"),
        fixture!("initial_object_value"),
        fixture!("initial_ordinary"),
        fixture!("initial_parenthesized_value"),
        fixture!("initial_pointer_type"),
        fixture!("initial_string_literal"),
        fixture!("initial_unary_value"),
        fixture!("review_call_operand"),
        fixture!("review_comma_operand"),
        fixture!("review_compound_literal"),
        fixture!("review_conditional_operand"),
        fixture!("review_deref_operand"),
        fixture!("review_nested_comments"),
        fixture!("review_nested_parenthesized_call"),
        fixture!("review_nested_sizeof"),
        fixture!("review_postincrement"),
        fixture!("review_string_contents"),
        fixture!("review_subscript_operand"),
    ] {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}"
        );
    }
}
#[test]
fn malformed_sizeof_parentheses_retain_all_recovered_source_children() {
    // Full Joern outputs remain nonexact diagnostics. These cases pin the
    // repaired source-content loss without claiming whole-graph equality.
    for (source, expected) in [
        (
            include_str!(
                "fixtures/sizeof-expansion/diagnostics/malformed_adjacent_identifiers/main.c"
            ),
            "sizeof ((value other))",
        ),
        (
            include_str!(
                "fixtures/sizeof-expansion/diagnostics/malformed_semicolon_sibling/main.c"
            ),
            "sizeof ((value; other))",
        ),
    ] {
        let cpg = build(source);
        let method = cpg.method_named("read")[0];
        let codes: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
            .into_iter()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call
                    && cpg.name_of(node) == Some("<operator>.sizeOf")
            })
            .map(|node| cpg.code_of(node).unwrap_or(""))
            .collect();
        assert_eq!(codes, [expected]);
    }
}
