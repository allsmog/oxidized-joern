//! Complete live-derived graphs for object macros in expanded field positions.
fn build(source: &str) -> cpg_core::Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("main.c", source)]);
    project.cpg
}

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("fixtures/field-macros/cases/", $name, "/main.c")),
            include_str!(concat!(
                "fixtures/field-macros/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn field_macros_match_complete_isolated_live_graphs() {
    let cases = [
        fixture!("array_receiver"),
        fixture!("cycle"),
        fixture!("field_alias"),
        fixture!("field_subscript"),
        fixture!("function_macro"),
        fixture!("function_name"),
        fixture!("nonfield"),
        fixture!("object_chain"),
        fixture!("redefinition"),
        fixture!("statement"),
    ];
    for (name, source, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}",
        );
    }
}
