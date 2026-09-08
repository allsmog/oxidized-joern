use cpg_core::Cpg;

fn build(sources: &[(&str, &str)]) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(sources);
    project.cpg
}

#[test]
fn static_modifiers_primitive_roles_and_numeric_literals_compose() {
    let cpg = build(&[(
        "mixed.c",
        include_str!("fixtures/combined-c-types/numeric-static/mixed.c"),
    )]);
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/combined-c-types/numeric-static/expected.txt"),
    );
}

#[test]
fn static_parenthesized_definitions_keep_translation_unit_identities() {
    let cpg = build(&[
        (
            "a.c",
            include_str!("fixtures/combined-c-types/static-parentheses/a.c"),
        ),
        (
            "b.c",
            include_str!("fixtures/combined-c-types/static-parentheses/b.c"),
        ),
    ]);
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&cpg),
        include_str!("fixtures/combined-c-types/static-parentheses/expected.txt"),
    );
}
