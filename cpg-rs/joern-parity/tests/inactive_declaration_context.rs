//! Pinned graphs for retained inactive declarations and active callable lookup.
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inactive-declaration-context/cases")
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
fn retained_inactive_declarations_match_complete_live_graphs() {
    for name in [
        "inactive_prototype",
        "active_then_inactive",
        "elif_context",
        "inactive_header_prototype",
        "initialized_scalar",
        "late_type_macro",
        "nested_inactive",
        "header_switch",
        "inactive_header_override",
        "inactive_aggregate",
        "inactive_typedef_binding",
    ] {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/inactive-declaration-context/cases")
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

// Array extent spelling remains a complete retained diagnostic. These targeted
// assertions check macro state without claiming whole-graph equality for it.
#[test]
fn inactive_directives_cannot_mutate_retained_array_macro_expansions() {
    for name in [
        "builtin_branch",
        "literal_inactive",
        "inactive_define",
        "inactive_include",
    ] {
        let dump = cpg_lang_c::import::canonical_dump(&build_case(name));
        assert!(
            dump.contains("CALL NAME=N CODE=N TYPE_FULL_NAME=int"),
            "{name}"
        );
        assert!(dump.contains("METHOD NAME=N CODE=#define N 3"), "{name}");
        assert!(!dump.contains("METHOD NAME=N CODE=#define N 4"), "{name}");
    }
    let dump = cpg_lang_c::import::canonical_dump(&build_case("later_define"));
    assert!(dump.contains("IDENTIFIER NAME=N CODE=<unknown> N TYPE_FULL_NAME=ANY"));
    assert!(dump.contains("CALL NAME=N CODE=N TYPE_FULL_NAME=int"));
}
