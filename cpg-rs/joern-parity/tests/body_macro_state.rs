//! Complete live graphs for source-order body macros and synthetic method metadata.
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/body-macro-state/cases")
        .join(name);
    fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, files);
            } else if path.extension().is_some_and(|ext| ext == "c" || ext == "h") {
                files.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read_to_string(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    let borrowed: Vec<_> = files
        .iter()
        .map(|(file, source)| (file.as_str(), source.as_str()))
        .collect();
    project.build(&borrowed);
    project.cpg
}

fn expected_case(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/body-macro-state/cases")
            .join(name)
            .join("expected.txt"),
    )
    .unwrap()
}

#[test]
fn body_macro_state_matches_complete_isolated_live_graphs() {
    for name in [
        "body_state_then_include",
        "conditional_local_typedef",
        "cross_function_state",
        "elif_definition",
        "header_then_local_undef",
        "inactive_effects",
        "inactive_function_effects",
        "local_definition",
        "local_function_macro",
        "local_redefinition",
        "local_type_macro",
        "local_undef",
        "nested_block_persistence",
        "no_backwards_leak",
        "runtime_if_directive",
        "selected_arm_definition",
        "block_macro_typedef_separation",
        "branch_condition_entry_state",
        "header_after_body_undef",
        "nested_copied_arguments",
        "recovery_after_redefinition",
        "recovery_entry_macros",
        "runtime_arms_lexical_order",
        "body_macro_later_prototype",
        "header_entry_before_body",
        "include_multiple_effects",
        "local_conditional_prototype",
        "local_macro_prototype",
        "spaced_inactive_typedef_shadow",
        "conditional_before_redefine",
        "defined_before_redefine",
        "earlier_function_condition",
        "intervening_nonmatching",
        "ordinary_references_redefined",
        "file_level_condition",
        "header_redefined",
        "inactive_and_shortcircuit",
        "outer_nested_body_argument",
        "global_before_function",
        "object_to_function",
        "header_event_tie_after",
        "header_event_tie_before",
        "mixed_global_method_order",
        "deferred_full_name_literal",
        "deferred_full_name_spaces",
        "character_literal",
        "prefixed_quoted",
        "quoted_body",
        "quoted_global",
        "function_wrap_redefinition",
        "ordinary_control",
        "sizeof_array_plain",
        "local_inactive_prototype",
        "local_inactive_prototype_header",
    ] {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build_case(name)),
            expected_case(name),
            "{name}"
        );
    }
}

#[test]
fn type_position_argument_events_preserve_complete_macro_methods() {
    // These complete projects retain broader lowering diagnostics. Assert the
    // complete method restored by this repair without narrowing their references.
    for name in ["sizeof_array", "disabled_function_tail"] {
        let expected = expected_case(name);
        let actual = cpg_lang_c::import::canonical_dump(&build_case(name));
        let method = |dump: &str| {
            dump.split("\n\n")
                .find(|block| block.starts_with("METHOD NAME=ARG "))
                .unwrap()
                .to_owned()
        };
        assert_eq!(method(&actual), method(&expected), "{name}");
    }
}
