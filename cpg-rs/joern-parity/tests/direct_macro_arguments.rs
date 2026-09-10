//! Raw preprocessing-token argument slots and complete live-derived graphs.
use cpg_core::Query;

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
                "fixtures/direct-macro-arguments/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/direct-macro-arguments/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn token_arguments_match_complete_isolated_live_graphs() {
    let cases = [
        fixture!("comment_operator"),
        fixture!("continued_comment"),
        fixture!("continued_comment_boundary"),
        fixture!("first_type"),
        fixture!("nested_comma"),
        fixture!("nested_operator"),
        fixture!("operator_slots"),
        fixture!("operators"),
        fixture!("quoted_tokens"),
        fixture!("quoted_whitespace"),
        fixture!("shifts"),
        fixture!("unknown_operand"),
    ];
    for (name, source, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}"
        );
    }
}

#[test]
fn typedef_intop_preserves_operands_and_original_macro_argument_slots() {
    // The full reference remains a diagnostic: typedef casts still lower as
    // pointer calls. This assertion pins the independently repaired operand loss.
    let cpg = build(include_str!(
        "fixtures/direct-macro-arguments/diagnostics/lua_intop/main.c"
    ));
    for (method_name, expected) in [
        ("add", ["v1", "v2"]),
        ("left", ["x", "y"]),
        ("right", ["x", "-y"]),
    ] {
        let method = cpg.method_named(method_name)[0];
        let nodes = cpg_analysis::pass::ast_descendants(&cpg, method);
        let wrapper = *nodes
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("intop"))
            .unwrap();
        let copied: Vec<_> = cpg
            .arguments_of(wrapper)
            .into_iter()
            .filter(|&node| cpg.kind_of(node) != cpg_core::NodeKind::Block)
            .map(|node| (cpg.argument_index_of(node), cpg.code_of(node).unwrap_or("")))
            .collect();
        assert_eq!(
            copied,
            [(2, expected[0]), (3, expected[1])],
            "{method_name}"
        );
        assert!(
            nodes.iter().all(|&node| cpg.name_of(node) != Some("")),
            "no missing operand in {method_name}"
        );
    }
}
