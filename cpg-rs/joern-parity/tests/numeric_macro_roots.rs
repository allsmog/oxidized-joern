//! Complete live graphs for literal and unary numeric macro roots.
macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                "fixtures/numeric-macro-roots/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/numeric-macro-roots/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn numeric_macro_roots_match_complete_live_graphs() {
    let cases = [
        fixture!("function_float"),
        fixture!("function_hexfloat"),
        fixture!("function_hexint"),
        fixture!("function_long"),
        fixture!("function_longdouble"),
        fixture!("function_longlong"),
        fixture!("function_negative_float"),
        fixture!("function_negative_long"),
        fixture!("function_paren_float"),
        fixture!("function_paren_long"),
        fixture!("object_float"),
        fixture!("object_hexfloat"),
        fixture!("object_hexint"),
        fixture!("object_long"),
        fixture!("object_longdouble"),
        fixture!("object_longlong"),
        fixture!("object_negative_float"),
        fixture!("object_negative_long"),
        fixture!("object_paren_float"),
        fixture!("object_paren_long"),
    ];
    for (name, source, expected) in cases {
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
