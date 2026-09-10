//! Complete cross-feature references for declaration, typedef and sizeof context.
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
        ($name, &[$(($file, include_str!(concat!("fixtures/sixth-context-interactions/cases/", $name, "/", $file)))),+][..],
        include_str!(concat!("fixtures/sixth-context-interactions/cases/", $name, "/expected.txt")))
    };
}

#[test]
fn declaration_typedef_and_sizeof_contexts_match_complete_live_graphs() {
    let cases = [
        fixture!("block_macro_parameter_shadow", ["api.h", "main.c"]),
        fixture!("header_cast_size_return", ["api.h", "main.c"]),
        fixture!("header_isolated_cast", ["a.c", "api.h", "b.c"]),
        fixture!("header_order_cast", ["api.h", "main.c"]),
        fixture!("macro_parameter_shadow", ["api.h", "main.c"]),
        fixture!("adjacent_methods", ["main.c"]),
        fixture!("before_after_typedef", ["main.c"]),
        fixture!("compound_macro_restoration", ["main.c"]),
        fixture!("compound_macro_typedef", ["main.c"]),
        fixture!("sizeof_typedef", ["main.c"]),
        fixture!("sizeof_shadow", ["main.c"]),
        fixture!("sizeof_minus", ["main.c"]),
        fixture!("sizeof_grouping", ["main.c"]),
    ];
    for (name, sources, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(sources)),
            expected,
            "{name}"
        );
    }
}
