//! Typedef aggregate bodies and initializer methods from pinned Joern v4.0.555.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

struct Case {
    name: &'static str,
    sources: &'static [(&'static str, &'static str)],
    expected: &'static str,
}

macro_rules! fixture {
    ($name:literal, [$($path:literal),+]) => {
        Case {
            name: $name,
            sources: &[$(($path, include_str!(concat!("fixtures/typedef-aggregates/cases/", $name, "/input/", $path)))),+],
            expected: include_str!(concat!("fixtures/typedef-aggregates/cases/", $name, "/expected.txt")),
        }
    };
}

fn build(case: &Case) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(case.sources);
    project.cpg
}

#[test]
fn typedef_aggregates_match_complete_isolated_live_graphs() {
    for case in [
        fixture!("alias_existing", ["main.c"]),
        fixture!("anonymous_array", ["main.c"]),
        fixture!("anonymous_macro", ["main.c"]),
        fixture!("anonymous_plain", ["main.c"]),
        fixture!("anonymous_union", ["main.c"]),
        fixture!("macro_source_order", ["main.c"]),
        fixture!("multiple_aliases", ["main.c"]),
        fixture!("pointer_alias", ["main.c"]),
        fixture!("tagged_array", ["main.c"]),
        fixture!("dimension_parenthesized", ["main.c"]),
        fixture!("header_size", ["defs.h", "main.c"]),
        fixture!("macro_expression", ["main.c"]),
        fixture!("macro_parenthesized", ["main.c"]),
        fixture!("multidimensional", ["main.c"]),
        fixture!("named_macro", ["main.c"]),
        fixture!("pointer_first", ["main.c"]),
        fixture!("conditional_aggregate", ["main.c"]),
        fixture!("function_macro_size", ["main.c"]),
        fixture!("lua_rn", ["main.c"]),
        fixture!("plain_alias", ["main.c"]),
        fixture!("qualified_pointer_member", ["main.c"]),
        fixture!("same_tag_alias", ["main.c"]),
        fixture!("array_alias", ["main.c"]),
        fixture!("named_multiple", ["main.c"]),
        fixture!("named_pointer", ["main.c"]),
        fixture!("ordinary_multidimensional", ["main.c"]),
    ] {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(&case)),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn lua_typedef_body_and_initializer_survive_production_import() {
    let cpg = build(&fixture!("lua_rn", ["main.c"]));
    let clinit = cpg
        .methods()
        .into_iter()
        .filter(|&node| cpg.full_name_of(node) == Some("RN.<clinit>:RN()"))
        .collect::<Vec<_>>();
    assert_eq!(
        clinit.len(),
        1,
        "the nested and standalone dumps represent one method"
    );
    let global = cpg
        .methods()
        .into_iter()
        .find(|&node| cpg.full_name_of(node) == Some("main.c:<global>"))
        .unwrap();
    let aggregate = cpg
        .out_kind(global, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::TypeDecl && cpg.name_of(node) == Some("RN"))
        .unwrap();
    let members = cpg
        .out_kind(aggregate, EdgeKind::Ast)
        .filter(|&node| cpg.kind_of(node) == NodeKind::Member)
        .collect::<Vec<_>>();
    assert_eq!(
        members
            .iter()
            .map(|&node| cpg.name_of(node).unwrap())
            .collect::<Vec<_>>(),
        ["f", "c", "n", "buff"]
    );
    assert_eq!(
        cpg.type_full_name_of(members[3]),
        Some("char[L_MAXLENNUM+1]")
    );
    assert!(cpg
        .out_kind(aggregate, EdgeKind::Ast)
        .any(|node| node == clinit[0]));
    assert!(cpg
        .out_kind(global, EdgeKind::Ast)
        .any(|node| cpg.kind_of(node) == NodeKind::Local && cpg.name_of(node) == Some("RN")));
    let body = cpg_analysis::pass::ast_descendants(&cpg, clinit[0]);
    assert!(body
        .iter()
        .any(|&node| cpg.name_of(node) == Some("<operator>.arrayInitializer")));
    assert!(body
        .iter()
        .any(|&node| cpg.kind_of(node) == NodeKind::Literal && cpg.code_of(node) == Some("200")));
}

#[test]
fn typedef_tags_aliases_and_source_order_remain_distinct() {
    let cpg = build(&fixture!("same_tag_alias", ["main.c"]));
    let global = cpg
        .methods()
        .into_iter()
        .find(|&node| cpg.full_name_of(node) == Some("main.c:<global>"))
        .unwrap();
    let declarations = cpg
        .out_kind(global, EdgeKind::Ast)
        .filter(|&node| cpg.kind_of(node) == NodeKind::TypeDecl)
        .map(|node| cpg.full_name_of(node).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(declarations, ["RN", "RN<duplicate>0"]);

    let cpg = build(&fixture!("macro_source_order", ["main.c"]));
    let global = cpg
        .methods()
        .into_iter()
        .find(|&node| cpg.full_name_of(node) == Some("main.c:<global>"))
        .unwrap();
    let descendants = cpg_analysis::pass::ast_descendants(&cpg, global);
    for (name, ty) in [("first", "char[2]"), ("second", "char[5]")] {
        let member = descendants
            .iter()
            .copied()
            .find(|&node| cpg.kind_of(node) == NodeKind::Member && cpg.name_of(node) == Some(name))
            .unwrap();
        assert_eq!(cpg.type_full_name_of(member), Some(ty));
    }
}

#[test]
fn every_member_dimension_reaches_the_initializer() {
    let cpg = build(&fixture!("multidimensional", ["main.c"]));
    let call = cpg
        .calls()
        .into_iter()
        .find(|&node| cpg.name_of(node) == Some("<operator>.arrayInitializer"))
        .unwrap();
    let dimensions = cpg
        .arguments_of(call)
        .into_iter()
        .map(|node| cpg.code_of(node).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(dimensions, ["2", "3 + 1"]);
}

#[test]
fn aggregate_pointer_and_array_aliases_keep_underlying_types_and_initializers() {
    let cpg = build(&fixture!("named_multiple", ["main.c"]));
    let types = cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Type)
        .filter_map(|node| cpg.full_name_of(node))
        .collect::<Vec<_>>();
    assert!(types.contains(&"Tag*"));
    assert!(types.contains(&"typedefTag[2]"));
    assert!(!types.contains(&"*Ptr"));
    assert!(!types.contains(&"unionTag"));
    let initialization = cpg
        .calls()
        .into_iter()
        .find(|&node| cpg.code_of(node) == Some("Values[2]"))
        .unwrap();
    assert_eq!(
        cpg.name_of(initialization),
        Some("<operator>.arrayInitializer")
    );
    let arguments = cpg.arguments_of(initialization);
    assert_eq!(arguments.len(), 1);
    assert_eq!(cpg.code_of(arguments[0]), Some("2"));
}
