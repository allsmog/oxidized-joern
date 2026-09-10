//! Numeric literal-node types measured against Joern v4.0.555.
use cpg_core::{Cpg, NodeKind, Query};

fn build() -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(
        "literals.c",
        include_str!("fixtures/numeric-literals/literals.c"),
    )]);
    project.cpg
}

#[test]
fn numeric_literals_match_complete_live_graph() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build()),
        include_str!("fixtures/numeric-literals/expected.txt"),
    );
}

#[test]
fn signed_children_keep_suffix_types_and_hex_digits_are_not_float_suffixes() {
    let cpg = build();
    for (method, operator, literal, ty) in [
        ("lit_35", "<operator>.minus", "1U", "unsigned int"),
        ("lit_59", "<operator>.minus", "1LL", "longlongint"),
        ("lit_60", "<operator>.plus", "1UL", "unsigned longint"),
        ("lit_61", "<operator>.minus", "1.0f", "float"),
        ("lit_62", "<operator>.plus", "0x1p2L", "longdouble"),
        ("lit_63", "<operator>.minus", "0xFFu", "unsigned int"),
    ] {
        let methods = cpg.method_named(method);
        assert_eq!(methods.len(), 1);
        let call = cpg_analysis::pass::ast_descendants(&cpg, methods[0])
            .into_iter()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some(operator)
            })
            .unwrap();
        let args = cpg.arguments_of(call);
        assert_eq!(args.len(), 1);
        assert_eq!(cpg.kind_of(args[0]), NodeKind::Literal);
        assert_eq!(cpg.code_of(args[0]), Some(literal));
        assert_eq!(cpg.type_full_name_of(args[0]), Some(ty));
    }
    for literal in ["0XFEED", "0xdeadBEEF", "18446744073709551615"] {
        let nodes: Vec<_> = cpg
            .nodes()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Literal && cpg.code_of(node) == Some(literal)
            })
            .collect();
        assert!(!nodes.is_empty(), "literal {literal}");
        assert!(
            nodes
                .into_iter()
                .all(|node| cpg.type_full_name_of(node) == Some("int")),
            "literal {literal}"
        );
    }
}
