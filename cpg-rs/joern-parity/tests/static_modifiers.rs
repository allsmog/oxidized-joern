//! Static declaration modifiers and their effect on method child ordering.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build() -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[
        (
            "static.c",
            include_str!("fixtures/static-modifiers/static.c"),
        ),
        (
            "storage.c",
            include_str!("fixtures/static-modifiers/storage.c"),
        ),
        (
            "spliced.c",
            include_str!("fixtures/static-modifiers/spliced.c"),
        ),
    ]);
    project.cpg
}

#[test]
fn static_modifiers_match_the_complete_live_graph() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build()),
        include_str!("fixtures/static-modifiers/expected.txt"),
    );
}

#[test]
fn only_a_leading_static_token_adds_a_modifier_and_shifts_the_return() {
    let cpg = build();
    let positive = [
        "local_static",
        "local_inline",
        "static_prototype",
        "static_inline_prototype",
        "later_static",
        "inactive_static",
        "zero_params",
        "before_const",
        "static_newline",
        "static_comment",
        "leading_comment",
        "spliced",
        "spliced_prototype",
    ];
    let negative = [
        "plain",
        "external",
        "inline_function",
        "inline_local",
        "external_inline",
        "inherited_static",
        "after_type",
        "after_const",
        "after_type_prototype",
        "after_inline_prototype",
        "prefixed_type",
        "dollar_type",
    ];
    for (names, expected_modifiers, return_order) in [(&positive[..], 1, 4), (&negative[..], 0, 3)]
    {
        for name in names {
            let methods = cpg.method_named(name);
            assert_eq!(methods.len(), 1, "{name}");
            let children: Vec<_> = cpg.out_kind(methods[0], EdgeKind::Ast).collect();
            let modifiers: Vec<_> = children
                .iter()
                .filter(|&&node| cpg.kind_of(node) == NodeKind::Modifier)
                .collect();
            assert_eq!(modifiers.len(), expected_modifiers, "{name}");
            if let Some(&&modifier) = modifiers.first() {
                assert_eq!(cpg.order_of(modifier), 3, "{name}");
            }
            let returned = children
                .into_iter()
                .find(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
                .unwrap();
            assert_eq!(cpg.order_of(returned), return_order, "{name}");
        }
    }
}
