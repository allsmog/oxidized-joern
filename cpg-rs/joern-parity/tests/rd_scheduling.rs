//! Complete graph comparisons for Joern's reaching-definition scheduling.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build(name: &str, source: &str) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(name, source)]);
    project.cpg
}

#[test]
fn scheduling_matches_all_sections_of_the_live_graph() {
    let cpg = build(
        "scheduling.c",
        include_str!("fixtures/rd-scheduling/scheduling.c"),
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/rd-scheduling/expected.txt"),
    );
}

#[test]
fn ordered_control_composition_matches_the_complete_live_graph() {
    let cpg = build(
        "ordering.c",
        include_str!("fixtures/rd-scheduling/ordering/ordering.c"),
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/rd-scheduling/ordering/expected.txt"),
    );
}

#[test]
fn broader_macro_ordering_diagnostic_is_now_exact() {
    let cpg = build(
        "ordering.c",
        include_str!("fixtures/rd-scheduling/macro-diagnostic/ordering.c"),
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/rd-scheduling/macro-diagnostic/expected-joern.txt"),
    );
}

#[test]
fn former_standalone_block_loop_tail_differences_are_exact() {
    let cpg = build(
        "blocks.c",
        include_str!("fixtures/standalone-blocks/loop-tail-diagnostic/blocks.c"),
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/standalone-blocks/loop-tail-diagnostic/expected-joern.txt"),
    );
}

#[test]
fn loop_tail_without_standalone_blocks_is_also_exact() {
    let cpg = build(
        "preexisting.c",
        include_str!("fixtures/standalone-blocks/loop-tail-diagnostic/preexisting.c"),
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/standalone-blocks/loop-tail-diagnostic/without-blocks-joern.txt"),
    );
}

#[test]
fn an_unscheduled_call_can_define_exit_without_receiving_call_site_edges() {
    let cpg = build(
        "scheduling.c",
        include_str!("fixtures/rd-scheduling/scheduling.c"),
    );
    let method = cpg.method_named("rd_dead_continue")[0];
    let descendants = cpg_analysis::pass::ast_descendants(&cpg, method);
    let dead_call = descendants
        .iter()
        .copied()
        .find(|&node| {
            cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("sink(value)")
        })
        .unwrap();
    let dead_argument = cpg.out_kind(dead_call, EdgeKind::Argument).next().unwrap();
    let exit = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
        .unwrap();
    assert!(cpg
        .out_kind(dead_call, EdgeKind::ReachingDef)
        .any(|to| to == exit));
    assert!(cpg
        .out_kind(dead_argument, EdgeKind::ReachingDef)
        .any(|to| to == exit));
    assert_eq!(cpg.in_kind(dead_call, EdgeKind::ReachingDef).count(), 0);
    let decrement = descendants
        .into_iter()
        .find(|&node| cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("count--"))
        .unwrap();
    assert!(!cpg
        .out_kind(decrement, EdgeKind::ReachingDef)
        .any(|to| to == exit));
}
