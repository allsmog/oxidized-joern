use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind};
use cpg_frontend::Frontend;

fn graph() -> Cpg {
    cpg_lang_c::CFrontend::new()
        .build_project(&[(
            "control_truth_loops.c",
            include_str!("../../joern-parity/corpus/control_truth_loops.c"),
        )])
        .unwrap()
}

fn method_nodes(cpg: &Cpg, name: &str) -> Vec<NodeId> {
    let root = cpg
        .nodes()
        .find(|&node| cpg.kind_of(node) == NodeKind::Method && cpg.name_of(node) == Some(name))
        .unwrap();
    let mut nodes = vec![root];
    let mut next = 0;
    while next < nodes.len() {
        nodes.extend(cpg.out_kind(nodes[next], EdgeKind::Ast));
        next += 1;
    }
    nodes
}

#[test]
fn identifier_truth_tests_use_the_oracle_integer_and_pointer_zero() {
    let cpg = graph();
    for (method, expected, zero, zero_type) in [
        ("scalar_if", "x != 0", "0", "int"),
        ("scalar_while", "x != 0", "0", "int"),
        ("scalar_do", "x != 0", "0", "int"),
        ("scalar_for", "x != 0", "0", "int"),
        ("pointer_if", "p != NULL", "NULL", "ANY"),
        ("pointer_while", "p != NULL", "NULL", "ANY"),
        ("pointer_do", "p != NULL", "NULL", "ANY"),
        ("pointer_for", "p != NULL", "NULL", "ANY"),
        ("paren_if", "((x)) != 0", "0", "int"),
        ("boolean_if", "x != 0", "0", "int"),
        ("array_if", "p != 0", "0", "int"),
    ] {
        let nodes = method_nodes(&cpg, method);
        let condition = nodes
            .iter()
            .find_map(|&node| cpg.out_kind(node, EdgeKind::Condition).next())
            .unwrap();
        assert_eq!(
            cpg.name_of(condition),
            Some("<operator>.notEquals"),
            "{method}"
        );
        assert_eq!(cpg.code_of(condition), Some(expected));
        assert_eq!(cpg.type_full_name_of(condition), Some("int"));
        let literal = cpg
            .out_kind(condition, EdgeKind::Argument)
            .find(|&node| cpg.kind_of(node) == NodeKind::Literal)
            .unwrap();
        assert_eq!(cpg.code_of(literal), Some(zero));
        assert_eq!(cpg.type_full_name_of(literal), Some(zero_type));
    }
}

#[test]
fn explicit_and_non_identifier_conditions_keep_their_expression() {
    let cpg = graph();
    for (method, code) in [
        ("explicit_if", "x != 0"),
        ("negation_if", "!x"),
        ("pointer_negation_if", "!p"),
        ("arithmetic_if", "x + 1"),
        ("call_if", "control_tick(x)"),
        ("literal_if", "1"),
        ("dereference_if", "*p"),
        ("and_if", "x && y"),
        ("assignment_if", "x = control_tick(x)"),
    ] {
        let nodes = method_nodes(&cpg, method);
        let condition = nodes
            .iter()
            .find_map(|&node| cpg.out_kind(node, EdgeKind::Condition).next())
            .unwrap();
        assert_eq!(cpg.code_of(condition), Some(code), "{method}");
        assert!(!cpg
            .out_kind(condition, EdgeKind::Ast)
            .any(|child| cpg.name_of(child) == Some("<operator>.notEquals")));
    }
}

#[test]
fn braceless_loop_bodies_keep_direct_statements_and_cfg_edges() {
    let cpg = graph();
    for (method, role, kind) in [
        ("scalar_while", EdgeKind::TrueBody, NodeKind::Call),
        ("scalar_do", EdgeKind::DoBody, NodeKind::Call),
        ("scalar_for", EdgeKind::ForBody, NodeKind::Call),
        ("empty_for", EdgeKind::ForBody, NodeKind::ControlStructure),
        ("return_while", EdgeKind::TrueBody, NodeKind::Return),
        ("return_do", EdgeKind::DoBody, NodeKind::Return),
        ("return_for", EdgeKind::ForBody, NodeKind::Return),
    ] {
        let nodes = method_nodes(&cpg, method);
        let body = nodes
            .iter()
            .find_map(|&node| cpg.out_kind(node, role).next())
            .unwrap();
        assert_eq!(cpg.kind_of(body), kind, "{method}");
        assert!(
            cpg.out_kind(body, EdgeKind::Cfg).next().is_some(),
            "{method}"
        );
        assert!(cpg.line_of(body).is_some(), "{method}");
    }
}

#[test]
fn multiple_for_declarators_share_one_initializer_slot() {
    let cpg = graph();
    for (method, assignment_count) in [("multi_init_for", 2), ("multi_empty_for", 0)] {
        let nodes = method_nodes(&cpg, method);
        let initializer = nodes
            .iter()
            .find_map(|&node| cpg.out_kind(node, EdgeKind::ForInit).next())
            .unwrap();
        assert_eq!(cpg.kind_of(initializer), NodeKind::Block);
        assert_eq!(cpg.order_of(initializer), 3);
        let mut assignments: Vec<_> = cpg.out_kind(initializer, EdgeKind::Ast).collect();
        assignments.sort_by_key(|&node| cpg.order_of(node));
        assert_eq!(assignments.len(), assignment_count);
        for (index, assignment) in assignments.into_iter().enumerate() {
            assert_eq!(cpg.name_of(assignment), Some("<operator>.assignment"));
            assert_eq!(cpg.argument_index_of(assignment), index as i32 + 1);
        }
    }
}

#[test]
fn unconditional_do_return_keeps_unreachable_tail_out_of_reaching_defs() {
    let cpg = graph();
    let nodes = method_nodes(&cpg, "return_do");
    let returns: Vec<_> = nodes
        .into_iter()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Return)
        .collect();
    assert_eq!(returns.len(), 2);
    let reachable = *returns
        .iter()
        .find(|&&node| cpg.code_of(node) == Some("return x;"))
        .unwrap();
    let unreachable = *returns
        .iter()
        .find(|&&node| cpg.code_of(node) == Some("return 0;"))
        .unwrap();
    assert!(cpg
        .out_kind(reachable, EdgeKind::ReachingDef)
        .next()
        .is_some());
    assert!(cpg
        .out_kind(unreachable, EdgeKind::ReachingDef)
        .next()
        .is_none());
    assert!(cpg
        .in_kind(unreachable, EdgeKind::ReachingDef)
        .next()
        .is_none());
}
