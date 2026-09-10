//! Complete pinned graphs for preprocessing-token macro arguments and cast CODE.
fn assert_graph(file: &str, source: &str, expected: &str) {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(file, source)]);
    assert_eq!(cpg_lang_c::import::canonical_dump(&project.cpg), expected);
}
macro_rules! case {
    ($name:literal, $file:literal) => {
        assert_graph(
            $file,
            include_str!(concat!(
                "fixtures/nested-macro-casts/cases/",
                $name,
                "/",
                $file
            )),
            include_str!(concat!(
                "fixtures/nested-macro-casts/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}
#[test]
fn nested_type_arguments_and_rawtt_match_complete_live_graph() {
    case!("nested", "casts.c");
}
#[test]
fn expanded_cast_types_preserve_qualifiers_and_distinct_type_identity() {
    case!("types", "types.c");
}
#[test]
fn ordinary_calls_literals_recursion_and_nested_arguments_remain_exact() {
    case!("boundaries", "boundaries.c");
}
#[test]
fn a_known_macro_suffix_does_not_expand_inside_a_unicode_identifier() {
    case!("unicode", "probe.c");
}
#[test]
fn composite_descriptors_render_once_and_preserve_nested_value_casts() {
    case!("composite", "probe.c");
}
