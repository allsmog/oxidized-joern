//! Complete graphs that become exact when the sixth context repairs compose.
//! Original standalone diagnostics and producer evidence remain unchanged.
macro_rules! fixture {
    ($path:literal) => {
        (
            $path,
            &[
                ("api.h", include_str!(concat!("fixtures/", $path, "/api.h"))),
                (
                    "main.c",
                    include_str!(concat!("fixtures/", $path, "/main.c")),
                ),
            ][..],
            include_str!(concat!("fixtures/", $path, "/expected.txt")),
        )
    };
}

#[test]
fn combined_type_binding_repairs_match_complete_live_graphs() {
    let cases = [
        fixture!("sixth-context-interactions/diagnostics/header_alias_return"),
        fixture!("header-return-bindings/cases/late_alias"),
        fixture!("header-return-bindings/cases/pointer_alias"),
    ];
    for (name, sources, expected) in cases {
        let mut project = cpg_incremental::Project::new(
            || Box::new(cpg_lang_c::CFrontend::new()),
            cpg_analysis::standard_pipeline(),
        );
        project.build(sources);
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&project.cpg),
            expected,
            "{name}"
        );
    }
}
