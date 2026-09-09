//! Complete live graphs for macro names versus compound array dimensions.
#[test]
fn array_dimension_macros_match_complete_live_graphs() {
    for (name, source, expected) in [
        (
            "bare_macro",
            include_str!("fixtures/array-dimension-macros/cases/bare_macro/main.c"),
            include_str!("fixtures/array-dimension-macros/cases/bare_macro/expected.txt"),
        ),
        (
            "binary_macro",
            include_str!("fixtures/array-dimension-macros/cases/binary_macro/main.c"),
            include_str!("fixtures/array-dimension-macros/cases/binary_macro/expected.txt"),
        ),
        (
            "parenthesized_macro",
            include_str!("fixtures/array-dimension-macros/cases/parenthesized_macro/main.c"),
            include_str!("fixtures/array-dimension-macros/cases/parenthesized_macro/expected.txt"),
        ),
        (
            "unary_macro",
            include_str!("fixtures/array-dimension-macros/cases/unary_macro/main.c"),
            include_str!("fixtures/array-dimension-macros/cases/unary_macro/expected.txt"),
        ),
    ] {
        let mut project = cpg_incremental::Project::new(
            || Box::new(cpg_lang_c::CFrontend::new()),
            cpg_analysis::standard_pipeline(),
        );
        project.build(&[("main.c", source)]);
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&project.cpg),
            expected,
            "{name}"
        );
    }
}
