//! Complete live graphs for included declarations and caller/header ownership.
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/body-includes/cases")
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
            .join("tests/fixtures/body-includes/cases")
            .join(name)
            .join("expected.txt"),
    )
    .unwrap()
}

#[test]
fn body_includes_match_complete_live_graphs() {
    for name in [
        "body_include_macro_only",
        "guarded_nested_relative_include",
        "inactive_body_include",
        "nested_scope_typedef_macro",
        "primitive_static_array_include",
        "primitive_static_array_inline",
        "used_scalar_include",
        "used_scalar_inline",
        "included_call_origin",
        "inline_call_origin",
    ] {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build_case(name)),
            expected_case(name),
            "{name}"
        );
    }
}

fn method_block<'a>(dump: &'a str, name: &str) -> &'a str {
    dump.split("\n\n")
        .find(|block| block.starts_with(&format!("METHOD NAME={name} ")))
        .unwrap()
}

#[test]
fn included_arrays_preserve_complete_supported_method_subtrees() {
    // Every complete project remains intact. The tiny projects retain the same
    // unrelated primitive member spelling gap, and the repeated project retains
    // its no-eligible-event wrapper and standalone-header phantom diagnostics.
    for (case, method) in [
        ("tiny_fixedtables_include", "fixedtables"),
        ("tiny_fixedtables_inline", "fixedtables"),
        ("repeated_include_context", "first"),
    ] {
        let actual = cpg_lang_c::import::canonical_dump(&build_case(case));
        let expected = expected_case(case);
        assert_eq!(
            method_block(&actual, method),
            method_block(&expected, method),
            "{case}"
        );
    }
}

#[test]
fn generated_header_method_registration_preserves_both_macro_owners() {
    let actual = cpg_lang_c::import::canonical_dump(&build_case("repeated_include_context"));
    let expected = expected_case("repeated_include_context");
    for (full, owner) in [
        ("main.c:N:int(0)", "main.c"),
        ("nested/table.h:N:int(0)", "nested/table.h"),
    ] {
        let matching = |dump: &str| {
            dump.split("\n\n")
                .find(|block| {
                    block.starts_with("METHOD NAME=N ")
                        && block
                            .lines()
                            .next()
                            .unwrap()
                            .contains(&format!(" FULL_NAME={full} "))
                })
                .unwrap()
                .to_owned()
        };
        assert_eq!(matching(&actual), matching(&expected), "{full}");
        for record in [
            format!("EDGES|SOURCE_FILE {full}#0 -> F:{owner}"),
            format!("EDGES|CONTAINS D:{owner}:<global> -> {full}#0"),
        ] {
            assert!(
                expected.split('\n').any(|line| line == record),
                "reference {record}"
            );
            assert!(
                actual.split('\n').any(|line| line == record),
                "candidate {record}"
            );
        }
    }
}
