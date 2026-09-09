//! Macro directive recovery and wrapper types measured against Joern v4.0.555.

struct Case {
    name: &'static str,
    sources: &'static [(&'static str, &'static str)],
    expected: &'static str,
}

macro_rules! fixture {
    ($name:literal, [$($path:literal),+]) => {
        Case {
            name: $name,
            sources: &[$(($path, include_str!(concat!("fixtures/macro-definitions/cases/", $name, "/input/", $path)))),+],
            expected: include_str!(concat!("fixtures/macro-definitions/cases/", $name, "/expected.txt")),
        }
    };
}

fn assert_complete_graph(case: &Case) {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(case.sources);
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&project.cpg),
        case.expected,
        "{}",
        case.name,
    );
}

#[test]
fn logical_macro_definitions_match_complete_isolated_live_graphs() {
    for case in [
        fixture!("lua_checkstackp_names", ["defs.h", "probe.c"]),
        fixture!("lua_fastgeti_names", ["defs.h", "probe.c"]),
        fixture!("multiline_comment", ["defs.h", "probe.c"]),
        fixture!("multiline_name", ["defs.h", "probe.c"]),
        fixture!("object_macro_cast", ["defs.h", "probe.c"]),
        fixture!("quoted_formal", ["defs.h", "probe.c"]),
    ] {
        assert_complete_graph(&case);
    }
}

#[test]
fn macro_wrapper_types_match_complete_live_graph() {
    assert_complete_graph(&fixture!("pi-types", ["types.c"]));
}
