//! Complete live graphs for selected function-body preprocessor branches.
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/body-preprocessor/cases")
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

#[test]
fn active_body_branches_match_complete_live_graphs() {
    for name in [
        "plain_call_control",
        "if_true",
        "if_else",
        "if_elif",
        "snprintf_defined",
        "ifndef_control",
        "inactive_phantoms",
        "definition_positions",
        "inactive_call_only",
        "builtin_body_condition",
        "header_positions",
        "ifdef_elif",
        "inactive_local_shadow",
        "inactive_typedef_shadow",
        "nested_conditionals",
        "header_spaced_undef",
        "plain_undef",
        "spaced_undef",
        "tab_undef",
        "define_after_undef",
        "inactive_undef",
        "undef_source_order",
        "unknown_undef",
        "inactive_header_directives",
        "inactive_header_source_order",
        "inactive_macro_prefix",
        "inactive_macro_return",
        "conditional_duplicates",
        "trailing_comment",
        "unknown_directive",
    ] {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/body-preprocessor/cases")
                .join(name)
                .join("expected.txt"),
        )
        .unwrap();
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build_case(name)),
            expected,
            "{name}"
        );
    }
}

#[test]
fn inactive_branches_and_directive_conditions_create_no_phantom_locals() {
    for name in ["inactive_phantoms", "snprintf_defined"] {
        let dump = cpg_lang_c::import::canonical_dump(&build_case(name));
        for inactive in ["discarded", "NO_snprintf", "NO_vsnprintf"] {
            assert!(
                !dump.contains(&format!("LOCAL NAME={inactive} ")),
                "{name}: {inactive}"
            );
        }
    }
    let dump = cpg_lang_c::import::canonical_dump(&build_case("inactive_phantoms"));
    assert!(dump.contains("LOCAL NAME=selected CODE=<unknown> selected"));
    let dump = cpg_lang_c::import::canonical_dump(&build_case("snprintf_defined"));
    assert!(dump.contains("CALL NAME=snprintf CODE=snprintf(p, n, \"%d\", n)"));
    assert!(!dump.contains("CALL NAME=sprintf "));
}
