//! Adjacent string expressions and macro expansion spellings from Joern 4.0.555.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build() -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(
        "strings.c",
        include_str!("fixtures/concatenated-strings/strings.c"),
    )]);
    project.cpg
}

#[test]
fn concatenated_strings_match_complete_live_graph() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build()),
        include_str!("fixtures/concatenated-strings/expected.txt")
    );
}

#[test]
fn lua_ternary_keeps_all_operands_and_cfg_branches() {
    let cpg = build();
    let method = cpg.method_named("lua_literal")[0];
    let conditional = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .find(|&node| cpg.name_of(node) == Some("<operator>.conditional"))
        .unwrap();
    let args = cpg.arguments_of(conditional);
    assert_eq!(args.len(), 3);
    assert_eq!(cpg.code_of(args[1]), Some("\"0x%\" WIDTH \"x\""));
    assert_eq!(cpg.kind_of(args[1]), NodeKind::Literal);
    assert_eq!(cpg.type_full_name_of(args[1]), Some("char*"));
    assert_eq!(cpg.argument_index_of(args[1]), 2);
    let branches: Vec<_> = cpg.out_kind(args[0], EdgeKind::Cfg).collect();
    assert!(branches.contains(&args[1]));
    assert!(branches.contains(&args[2]));
    assert!(cpg
        .out_kind(args[1], EdgeKind::Cfg)
        .any(|node| node == conditional));
}

#[test]
fn source_literals_and_following_calls_keep_their_lines() {
    let cpg = build();
    let method = cpg.method_named("multiline")[0];
    let mut calls: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some("consume"))
        .collect();
    calls.sort_by_key(|&node| cpg.line_of(node));
    assert_eq!(calls.len(), 2);
    assert_eq!(cpg.line_of(calls[0]), Some(28));
    assert_eq!(cpg.line_of(cpg.arguments_of(calls[0])[0]), Some(28));
    assert_eq!(cpg.line_of(calls[1]), Some(30));
    assert_eq!(cpg.line_of(cpg.arguments_of(calls[1])[0]), Some(30));
    let lua = cpg.method_named("lua_literal")[0];
    let literal = cpg_analysis::pass::ast_descendants(&cpg, lua)
        .into_iter()
        .find(|&node| cpg.code_of(node) == Some("\"0x%\" WIDTH \"x\""))
        .unwrap();
    assert_eq!(cpg.line_of(literal), Some(9));
}
