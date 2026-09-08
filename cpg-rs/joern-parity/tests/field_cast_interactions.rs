//! Complete live-derived graphs for combined field macro and nested cast behavior.
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
            include_str!(concat!(
                "fixtures/field-cast-interactions/cases/",
                $name,
                "/main.c"
            )),
            include_str!(concat!(
                "fixtures/field-cast-interactions/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn field_cast_interactions_match_complete_isolated_live_graphs() {
    let cases = [
        fixture!("cast_index"),
        fixture!("outer_function"),
        fixture!("sibling_cast_fields"),
    ];
    for (name, source, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}",
        );
    }
}
