//! Complete isolated Joern graphs for C array initialization.
use cpg_core::{Cpg, Query};

fn build(source: &str) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("arrays.c", source)]);
    project.cpg
}

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                "fixtures/array-initializers/cases/",
                $name,
                "/arrays.c"
            )),
            include_str!(concat!(
                "fixtures/array-initializers/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

const CASES: &[(&str, &str, &str)] = &[
    fixture!("array_flows"),
    fixture!("array_parameter"),
    fixture!("callback_array"),
    fixture!("commented_brace"),
    fixture!("compound_literal"),
    fixture!("concat_string"),
    fixture!("for_multiple"),
    fixture!("for_unsized"),
    fixture!("global_char_brace"),
    fixture!("global_char_sized_string"),
    fixture!("global_char_unsized_string"),
    fixture!("global_const_brace"),
    fixture!("global_designated"),
    fixture!("global_double_brace"),
    fixture!("global_empty_brace"),
    fixture!("global_flat_multidim"),
    fixture!("global_int_sized_brace"),
    fixture!("global_int_unsized_brace"),
    fixture!("global_long_brace"),
    fixture!("global_nested_brace"),
    fixture!("global_nested_designated"),
    fixture!("global_pointer_elements"),
    fixture!("global_sized_noinit"),
    fixture!("global_string_rows"),
    fixture!("global_trailing_brace"),
    fixture!("global_uchar_brace"),
    fixture!("global_unsized_multidim"),
    fixture!("global_unsized_noinit"),
    fixture!("local_char_brace"),
    fixture!("local_char_sized_string"),
    fixture!("local_char_unsized_string"),
    fixture!("local_const_brace"),
    fixture!("local_designated"),
    fixture!("local_double_brace"),
    fixture!("local_empty_brace"),
    fixture!("local_flat_multidim"),
    fixture!("local_int_sized_brace"),
    fixture!("local_int_unsized_brace"),
    fixture!("local_long_brace"),
    fixture!("local_multiple"),
    fixture!("local_nested_brace"),
    fixture!("local_nested_designated"),
    fixture!("local_pointer_elements"),
    fixture!("local_pointer_to_array"),
    fixture!("local_pointer_to_array_init"),
    fixture!("local_range"),
    fixture!("local_size_expr"),
    fixture!("local_sized_noinit"),
    fixture!("local_static"),
    fixture!("local_string_rows"),
    fixture!("local_trailing_brace"),
    fixture!("local_uchar_brace"),
    fixture!("local_unsized_multidim"),
    fixture!("local_unsized_noinit"),
    fixture!("location_overlap"),
    fixture!("mixed_noinit"),
    fixture!("nested_array_dims"),
    fixture!("nested_pointer_array"),
    fixture!("parameter_types"),
    fixture!("parenthesized_pointer"),
    fixture!("parenthesized_scalar"),
    fixture!("pointer_array"),
    fixture!("pointer_multidim"),
    fixture!("pointer_to_array"),
    fixture!("pointer_to_array_init"),
    fixture!("range_designator"),
    fixture!("scalar_brace"),
    fixture!("source_lines"),
    fixture!("wide_string"),
];

#[test]
fn array_initializers_match_complete_isolated_live_graphs() {
    for &(name, source, expected) in CASES {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}"
        );
    }
}

#[test]
fn all_local_allocations_precede_explicit_initializers() {
    let cpg = build(include_str!(
        "fixtures/array-initializers/cases/local_multiple/arrays.c"
    ));
    let method = cpg.method_named("arrays")[0];
    let mut assignments: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .filter(|&node| cpg.name_of(node) == Some("<operator>.assignment"))
        .collect();
    assignments.sort_by_key(|&node| cpg.order_of(node));
    let shapes: Vec<_> = assignments
        .iter()
        .map(|&node| {
            let args = cpg.arguments_of(node);
            (
                cpg.order_of(node),
                cpg.code_of(args[0]).unwrap(),
                cpg.name_of(args[1]),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        [
            (4, "first", Some("<operator>.alloc")),
            (5, "second", Some("<operator>.alloc")),
            (6, "first", Some("<operator>.arrayInitializer")),
            (7, "other", None),
            (8, "second", Some("<operator>.arrayInitializer")),
        ]
    );
}

#[test]
fn array_initializers_preserve_source_lines_across_repeated_methods() {
    let cpg = build(include_str!(
        "fixtures/array-initializers/cases/source_lines/arrays.c"
    ));
    for (name, initializer_line, produce_line, consume_line) in
        [("z_first", 4, 5, 8), ("a_second", 11, 12, 15)]
    {
        let method = cpg.method_named(name)[0];
        let descendants = cpg_analysis::pass::ast_descendants(&cpg, method);
        let produce = *descendants
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("produce"))
            .unwrap();
        let consume = *descendants
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("consume"))
            .unwrap();
        let initializer = *descendants
            .iter()
            .find(|&&node| cpg.name_of(node) == Some("<operator>.arrayInitializer"))
            .unwrap();
        assert_eq!(cpg.line_of(produce), Some(produce_line), "{name}");
        assert_eq!(cpg.line_of(consume), Some(consume_line), "{name}");
        assert_eq!(cpg.line_of(initializer), Some(initializer_line), "{name}");
    }
}

#[test]
fn allocation_views_do_not_consume_declaration_or_following_statement_locations() {
    let cpg = build(include_str!(
        "fixtures/array-initializers/cases/location_overlap/arrays.c"
    ));
    let lines = |method: &str, code: &str| {
        let method = cpg.method_named(method)[0];
        let mut lines: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
            .into_iter()
            .filter(|&node| cpg.code_of(node) == Some(code))
            .map(|node| cpg.line_of(node).unwrap())
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(lines("sized_string", "\"x\""), [6]);
    assert_eq!(lines("sized_string", "use_char(value)"), [7]);
    assert_eq!(lines("mixed_array_first", "a[2] = {1}"), [10, 10, 10]);
    assert_eq!(lines("mixed_array_first", "b[2]"), [11, 11]);
    assert_eq!(lines("mixed_array_first", "use(a)"), [12]);
    assert_eq!(lines("mixed_scalar_first", "a=1"), [15]);
    assert_eq!(lines("mixed_scalar_first", "b[2]"), [16, 16]);
    assert_eq!(lines("mixed_scalar_first", "use(b)"), [17]);
    assert_eq!(lines("repeat_dimension", "size()"), [20, 21]);
    assert_eq!(
        lines("separate_scope", "a[2]={1}"),
        [24, 24, 24, 25, 25, 25]
    );
    assert_eq!(lines("separate_scope", "use(a)"), [26]);
}
