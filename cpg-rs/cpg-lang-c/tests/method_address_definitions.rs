use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind};
use cpg_frontend::Frontend;

fn graph() -> Cpg {
    cpg_lang_c::CFrontend::new()
        .build_project(&[(
            "method_address_definitions.c",
            include_str!("../../joern-parity/corpus/method_address_definitions.c"),
        )])
        .unwrap()
}

fn subtree(cpg: &Cpg, name: &str) -> Vec<NodeId> {
    let method = cpg
        .nodes()
        .find(|&node| cpg.kind_of(node) == NodeKind::Method && cpg.name_of(node) == Some(name))
        .unwrap();
    let mut nodes = vec![method];
    let mut cursor = 0;
    while cursor < nodes.len() {
        nodes.extend(cpg.out_kind(nodes[cursor], EdgeKind::Ast));
        cursor += 1;
    }
    nodes
}

fn exit(cpg: &Cpg, nodes: &[NodeId]) -> NodeId {
    *nodes
        .iter()
        .find(|&&node| cpg.kind_of(node) == NodeKind::MethodReturn)
        .unwrap()
}

#[test]
fn distinct_empty_code_targets_and_unnamed_parameter_reach_exit() {
    let cpg = graph();
    let nodes = subtree(&cpg, "address_distinct");
    let exit = exit(&cpg, &nodes);
    let targets: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Identifier && cpg.code_of(node) == Some(""))
        .collect();
    assert_eq!(targets.len(), 2);
    for target in targets {
        assert!(cpg
            .out_kind(target, EdgeKind::ReachingDef)
            .any(|node| node == exit));
    }
    let parameter = *nodes
        .iter()
        .find(|&&node| cpg.kind_of(node) == NodeKind::MethodParameterIn)
        .unwrap();
    assert_eq!(cpg.name_of(parameter), Some(""));
    assert!(cpg
        .out_kind(parameter, EdgeKind::ReachingDef)
        .any(|node| node == exit));
}

#[test]
fn reassignment_kills_the_prior_empty_code_definition_by_name() {
    let cpg = graph();
    for method in ["address_overwritten", "address_killed"] {
        let nodes = subtree(&cpg, method);
        let exit = exit(&cpg, &nodes);
        let target = *nodes
            .iter()
            .find(|&&node| {
                cpg.kind_of(node) == NodeKind::Identifier
                    && cpg.name_of(node) == Some("first")
                    && cpg.code_of(node) == Some("")
            })
            .unwrap();
        assert!(
            !cpg.out_kind(target, EdgeKind::ReachingDef)
                .any(|node| node == exit),
            "{method}"
        );
        let replacement = *nodes
            .iter()
            .find(|&&node| {
                cpg.kind_of(node) == NodeKind::Identifier
                    && cpg.name_of(node) == Some("first")
                    && cpg.code_of(node) == Some("first")
            })
            .unwrap();
        assert!(
            cpg.out_kind(replacement, EdgeKind::ReachingDef)
                .any(|node| node == exit),
            "{method}"
        );
    }
}

#[test]
fn explicit_function_address_does_not_kill_the_unnamed_parameter() {
    let cpg = graph();
    let nodes = subtree(&cpg, "address_explicit");
    let parameter = *nodes
        .iter()
        .find(|&&node| cpg.kind_of(node) == NodeKind::MethodParameterIn)
        .unwrap();
    let exit = exit(&cpg, &nodes);
    assert!(cpg
        .out_kind(parameter, EdgeKind::ReachingDef)
        .any(|node| node == exit));
}
