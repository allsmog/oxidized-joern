//! Complete Joern-derived graphs for source-position typedef cast context.
use cpg_core::Query;

fn build(sources: &[(&str, &str)]) -> cpg_core::Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(sources);
    project.cpg
}

macro_rules! fixture {
    ($name:literal, [$($file:literal),+]) => {
        ($name, &[$(($file, include_str!(concat!("fixtures/macro-cast-context/cases/", $name, "/", $file)))),+][..],
        include_str!(concat!("fixtures/macro-cast-context/cases/", $name, "/expected.txt")))
    };
}

#[test]
fn known_typedef_cast_context_matches_complete_live_graphs() {
    let cases = [
        fixture!("block_shadow", ["main.c"]),
        fixture!("comma_cast", ["main.c"]),
        fixture!("function_alias", ["main.c"]),
        fixture!("header_condition", ["main.c", "types.h"]),
        fixture!("header_isolation", ["a.c", "b.c"]),
        fixture!("header_late", ["main.c", "types.h"]),
        fixture!("header_missing", ["main.c"]),
        fixture!("header_typedef", ["main.c", "types.h"]),
        fixture!("late_shadow", ["main.c"]),
        fixture!("local_macro_typedef", ["main.c"]),
        fixture!("local_typedef", ["main.c"]),
        fixture!("lua_intop", ["main.c"]),
        fixture!("macro_bare", ["main.c"]),
        fixture!("macro_named", ["main.c"]),
        fixture!("nested_macro", ["main.c"]),
        fixture!("ordinary_call_control", ["main.c"]),
        fixture!("ordinary_named", ["main.c"]),
        fixture!("ordinary_qualified", ["main.c"]),
        fixture!("parameter_shadow", ["main.c"]),
        fixture!("pointer_alias", ["main.c"]),
        fixture!("unknown_control", ["main.c"]),
        fixture!("compound_typedef", ["main.c"]),
        fixture!("direct_type_argument", ["main.c"]),
        fixture!("for_shadow", ["main.c"]),
        fixture!("local_preprocessor", ["main.c"]),
        fixture!("local_same_global", ["main.c"]),
        fixture!("pointer_typedefs", ["main.c"]),
        fixture!("primitive_typedefs", ["main.c"]),
        fixture!("prototype_shadow", ["main.c"]),
        fixture!("qualified_typedef", ["main.c"]),
        fixture!("repeated_local", ["main.c"]),
        fixture!("cast_comment", ["main.c"]),
    ];
    for (name, sources, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(sources)),
            expected,
            "{name}"
        );
    }
}

#[test]
fn sizeof_typedef_context_distinguishes_parameter_shadowing() {
    // Full graphs retain separately owned expansion-spelling diagnostics. These
    // production assertions pin type-vs-value identity without weakening them.
    for (source, expected_type) in [
        (
            include_str!("fixtures/macro-cast-context/diagnostics/sizeof_typedef/main.c"),
            "T",
        ),
        (
            include_str!("fixtures/macro-cast-context/diagnostics/sizeof_shadow/main.c"),
            "int",
        ),
    ] {
        let cpg = build(&[("main.c", source)]);
        let method = cpg.method_named("value")[0];
        let nodes = cpg_analysis::pass::ast_descendants(&cpg, method);
        let sizeof_calls: Vec<_> = nodes
            .into_iter()
            .filter(|&node| cpg.name_of(node) == Some("<operator>.sizeOf"))
            .collect();
        assert_eq!(sizeof_calls.len(), 2);
        for call in sizeof_calls {
            let operands = cpg.arguments_of(call);
            assert_eq!(operands.len(), 1);
            assert_eq!(cpg.kind_of(operands[0]), cpg_core::NodeKind::Identifier);
            assert_eq!(cpg.type_full_name_of(operands[0]), Some(expected_type));
            assert_eq!(cpg.name_of(operands[0]), Some("T"));
        }
    }
}
