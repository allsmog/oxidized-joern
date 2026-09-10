//! Complete live Joern graphs for default C macros and explicit overrides.
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
        ($name, &[$(($file, include_str!(concat!("fixtures/predefined-c-macros/cases/", $name, "/", $file)))),+][..],
        include_str!(concat!("fixtures/predefined-c-macros/cases/", $name, "/expected.txt")))
    };
}

#[test]
fn default_c_macros_match_complete_live_graphs() {
    let cases = [
        fixture!("conditions_and_headers__c_language", ["main.c"]),
        fixture!("conditions_and_headers__cplusplus", ["main.c"]),
        fixture!(
            "conditions_and_headers__explicit_stdc_header",
            ["api.h", "config.h", "main.c"]
        ),
        fixture!("conditions_and_headers__gcc", ["main.c"]),
        fixture!(
            "conditions_and_headers__gcc_derived_header",
            ["api.h", "config.h", "main.c"]
        ),
        fixture!("conditions_and_headers__stdc", ["main.c"]),
        fixture!(
            "conditions_and_headers__stdc_derived_header",
            ["api.h", "config.h", "main.c"]
        ),
        fixture!("conditions_and_headers__stdc_version", ["main.c"]),
        fixture!(
            "conditions_and_headers__undefined_stdc_header",
            ["api.h", "config.h", "main.c"]
        ),
        fixture!("conditions_and_headers__version_c99", ["main.c"]),
        fixture!("overrides__stdc_redefined_two", ["main.c"]),
        fixture!("overrides__stdc_undef", ["main.c"]),
        fixture!("overrides__stdc_value_one", ["main.c"]),
        fixture!("overrides__stdc_zero", ["main.c"]),
        fixture!("direct_values__hosted", ["main.c"]),
        fixture!("direct_values__hosted_redefined", ["main.c"]),
        fixture!("direct_values__hosted_undef", ["main.c"]),
        fixture!("direct_values__stdc", ["main.c"]),
        fixture!("direct_values__version", ["main.c"]),
        fixture!("direct_values__version_redefined", ["main.c"]),
        fixture!("direct_values__version_undef", ["main.c"]),
        fixture!("body_combinations__combined", ["main.c"]),
        fixture!("body_combinations__redefined_combined", ["main.c"]),
        fixture!("body_combinations__undefined_direct", ["main.c"]),
        fixture!("body_combinations__user_macro", ["main.c"]),
    ];
    for (name, sources, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(sources)),
            expected,
            "{name}"
        );
    }
}
