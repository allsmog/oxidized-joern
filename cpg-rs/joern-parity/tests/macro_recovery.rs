//! Complete live-derived graphs for CDT recovery after invalid macro-expanded call arguments.
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
            include_str!(concat!("fixtures/macro-recovery/cases/", $name, "/main.c")),
            include_str!(concat!(
                "fixtures/macro-recovery/cases/",
                $name,
                "/expected.txt"
            )),
        )
    };
}

#[test]
fn macro_recovery_matches_complete_isolated_live_graphs() {
    let cases = [
        fixture!("bad_condition"),
        fixture!("bad_conditional"),
        fixture!("bad_expression"),
        fixture!("bad_format"),
        fixture!("bad_while"),
        fixture!("callable_later_callee"),
        fixture!("callable_later_macro_callee"),
        fixture!("callable_later_method_ref"),
        fixture!("global_argument"),
        fixture!("good_format"),
        fixture!("multiple_declarators"),
        fixture!("nested_bad_format"),
        fixture!("order_extra_global_arg_later"),
        fixture!("order_extra_global_arg_only"),
        fixture!("order_extra_global_reverse"),
        fixture!("order_extra_recovery_repeat_helper"),
        fixture!("order_ordinary_ab"),
        fixture!("order_ordinary_aba"),
        fixture!("order_ordinary_ba"),
        fixture!("order_ordinary_call"),
        fixture!("order_recovery_forward"),
        fixture!("order_recovery_renamed"),
        fixture!("order_recovery_repeat"),
        fixture!("order_recovery_reverse"),
        fixture!("order_recovery_unknown"),
        fixture!("pointer_call"),
        fixture!("return_bad_format"),
        fixture!("review_bare_function_sibling"),
        fixture!("review_defined_function_argument"),
        fixture!("review_function_argument"),
        fixture!("review_ordinary_after_recovery"),
        fixture!("review_scoped_callback"),
        fixture!("shadowing"),
        fixture!("sibling_shadow"),
        fixture!("sizeof_array"),
        fixture!("sizeof_basic"),
        fixture!("sizeof_function_pointer"),
        fixture!("sizeof_pointer"),
        fixture!("sizeof_qualified_pointer"),
        fixture!("sizeof_repeated_basics"),
        fixture!("sizeof_review_postfix_qualifier"),
        fixture!("sizeof_tagged_pointer"),
        fixture!("sizeof_unsigned_pointer"),
        fixture!("special_null_interleaved"),
        fixture!("special_sizeof_interleaved"),
        fixture!("special_sizeof_types"),
        fixture!("unknown_name"),
        fixture!("valid_then_recovery"),
    ];
    for (name, source, expected) in cases {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "{name}",
        );
    }
}
