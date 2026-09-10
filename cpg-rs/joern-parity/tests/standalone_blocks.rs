//! Standalone compound statements, compared without filtering to Joern 4.0.555.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build() -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(
        "blocks.c",
        include_str!("fixtures/standalone-blocks/blocks.c"),
    )]);
    project.cpg
}

#[test]
fn standalone_blocks_match_complete_live_graph() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build()),
        include_str!("fixtures/standalone-blocks/expected.txt"),
    );
}

#[test]
fn standalone_block_is_a_cfg_node_after_its_statements() {
    let cpg = build();
    let method = cpg.method_named("bare_entry")[0];
    let body = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::Block)
        .unwrap();
    let standalone = cpg
        .out_kind(body, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::Block)
        .unwrap();
    let sink = cpg.out_kind(standalone, EdgeKind::Ast).next().unwrap();
    let returned = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
        .unwrap();
    assert_eq!(
        cpg.out_kind(sink, EdgeKind::Cfg).collect::<Vec<_>>(),
        [standalone]
    );
    assert_eq!(
        cpg.out_kind(standalone, EdgeKind::Cfg).collect::<Vec<_>>(),
        [returned]
    );
    assert_eq!(cpg.out_kind(body, EdgeKind::Cfg).count(), 0);
}

#[test]
fn standalone_scope_restores_parameter_references_and_pointer_dispatch() {
    let cpg = build();
    let method = cpg.method_named("block_local_scope")[0];
    let parameter = cpg.parameters_of(method)[0];
    let descendants = cpg_analysis::pass::ast_descendants(&cpg, method);
    let local = descendants
        .iter()
        .copied()
        .find(|&node| cpg.kind_of(node) == NodeKind::Local)
        .unwrap();
    let returned = descendants
        .iter()
        .copied()
        .find(|&node| cpg.kind_of(node) == NodeKind::Return)
        .unwrap();
    let identifier = cpg.out_kind(returned, EdgeKind::Ast).next().unwrap();
    assert_eq!(
        cpg.out_kind(identifier, EdgeKind::Ref).collect::<Vec<_>>(),
        [parameter]
    );
    assert!(descendants.iter().any(|&node| {
        cpg.kind_of(node) == NodeKind::Identifier
            && cpg
                .out_kind(node, EdgeKind::Ref)
                .any(|target| target == local)
    }));

    let method = cpg.method_named("block_pointer_scope")[0];
    let calls: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .filter(|&node| {
            cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("callable(value)")
        })
        .collect();
    assert_eq!(calls.len(), 2);
    let returned = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .find(|&node| cpg.kind_of(node) == NodeKind::Return)
        .unwrap();
    let returned_call = cpg.out_kind(returned, EdgeKind::Ast).next().unwrap();
    assert_eq!(cpg.name_of(returned_call), Some("<operator>.pointerCall"));
    assert!(calls
        .iter()
        .any(|&call| cpg.name_of(call) == Some("callable")));
}
