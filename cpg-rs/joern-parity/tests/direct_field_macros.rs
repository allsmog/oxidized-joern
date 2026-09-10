//! Complete graph and source-location pins for direct field-token expansions.
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
                "fixtures/direct-field-macros/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/direct-field-macros/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn direct_field_macros_match_complete_isolated_live_graphs() {
    let cases = [
        fixture!("array_receiver"),
        fixture!("arrow"),
        fixture!("binary_replacement"),
        fixture!("cycle"),
        fixture!("call_index"),
        fixture!("recursive_index"),
        fixture!("indirect_recursive_index"),
        fixture!("sibling_owners"),
        fixture!("dot"),
        fixture!("expression_index"),
        fixture!("field_subscript"),
        fixture!("field_then_index"),
        fixture!("function_name"),
        fixture!("global_index"),
        fixture!("multiline"),
        fixture!("multiple_literals"),
        fixture!("object_chain"),
        fixture!("object_macro_index"),
        fixture!("parameter_index"),
        fixture!("repeated"),
    ];
    for (name, source, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}",
        );
    }
}

#[test]
fn repeated_field_expansions_keep_their_own_access_and_following_call_lines() {
    let cpg = build(include_str!(
        "fixtures/direct-field-macros/cases/multiline/main.c"
    ));
    let method = cpg.method_named("read")[0];
    let nodes = cpg_analysis::pass::ast_descendants(&cpg, method);
    for (name, expected) in [("Len", vec![5, 7]), ("use", vec![6, 8])] {
        let mut lines: Vec<_> = nodes
            .iter()
            .filter(|&&node| cpg.name_of(node) == Some(name))
            .map(|&node| cpg.line_of(node).expect("located source expression"))
            .collect();
        lines.sort();
        assert_eq!(lines, expected, "{name}");
    }
}
