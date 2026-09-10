//! Global array dimensions and local allocation shapes, pinned to Joern 4.0.555.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build() -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(
        "arrays.c",
        include_str!("fixtures/array-declarations/arrays.c"),
    )]);
    project.cpg
}

#[test]
fn array_declarations_match_complete_live_graph() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build()),
        include_str!("fixtures/array-declarations/expected.txt"),
    );
}

#[test]
fn global_dimensions_do_not_expand_local_allocation_parameters() {
    let cpg = build();
    let global = cpg
        .methods()
        .into_iter()
        .find(|&method| cpg.full_name_of(method) == Some("arrays.c:<global>"))
        .unwrap();
    let block = cpg
        .out_kind(global, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::Block)
        .unwrap();
    let calls = cpg_analysis::pass::ast_descendants(&cpg, block);
    assert!(calls
        .iter()
        .all(|&node| cpg.name_of(node) != Some("<operator>.alloc")));
    let global_three = calls
        .into_iter()
        .find(|&node| cpg.code_of(node) == Some("global_three[2][3][4]"))
        .unwrap();
    assert_eq!(
        cpg.name_of(global_three),
        Some("<operator>.arrayInitializer")
    );
    let dimensions: Vec<_> = cpg
        .arguments_of(global_three)
        .into_iter()
        .map(|node| cpg.code_of(node).unwrap())
        .collect();
    assert_eq!(dimensions, ["2", "3", "4"]);

    let alloc = cpg.method_named("<operator>.alloc");
    assert_eq!(alloc.len(), 1);
    assert_eq!(cpg.parameters_of(alloc[0]).len(), 3);
    assert_eq!(
        cpg.out_kind(alloc[0], EdgeKind::Ast)
            .filter(|&node| cpg.kind_of(node) == NodeKind::MethodParameterOut)
            .count(),
        3
    );
    let local_two = cpg
        .calls()
        .into_iter()
        .find(|&node| {
            cpg.name_of(node) == Some("<operator>.alloc")
                && cpg.code_of(node) == Some("local_two[2][3]")
        })
        .unwrap();
    let args: Vec<_> = cpg
        .arguments_of(local_two)
        .into_iter()
        .map(|node| cpg.code_of(node).unwrap())
        .collect();
    assert_eq!(args, ["int[2][3]", "2", "3"]);
}
