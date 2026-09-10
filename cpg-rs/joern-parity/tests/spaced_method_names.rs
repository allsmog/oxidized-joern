//! Complete live graph regression coverage for method full names containing spaces.
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spaced-method-names/cases")
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
fn complete_method_names_preserve_cfg_and_reaching_definitions() {
    for name in [
        "function_ulong",
        "function_ulonglong",
        "function_unsigned",
        "object_ulong",
        "object_ulonglong",
        "object_unsigned",
        "function_unsigned_arg",
        "function_unsigned_long_long_mixed",
        "negative_unsigned_control",
        "object_long_double_control",
        "object_unsigned_long_lower",
        "object_unsigned_lower",
        "ordinary_unsigned_returns",
        "spaced_header",
        "lowercase_marker_control",
        "prefix_collision",
        "spaced_global_capture",
    ] {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/spaced-method-names/cases")
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
fn method_prefix_collision_cannot_steal_header_or_macro_entry_edges() {
    let dump = cpg_lang_c::import::canonical_dump(&build_case("prefix_collision"));
    assert!(dump.contains("EDGES|CFG a b.h:<global>#0 -> a b.h:<global>#2"));
    assert!(dump.contains("EDGES|CFG a b.h:VALUE:ANY(1)#0 -> a b.h:VALUE:ANY(1)#4"));
    assert!(dump.contains("FLOWS|REACHING_DEF[] a b.h:VALUE:ANY(1)#0 -> a b.h:VALUE:ANY(1)#1"));
    assert!(!dump.contains("EDGES|CFG a#0 -> a b.h:"));
    assert!(!dump.contains("FLOWS|REACHING_DEF[] a#0 -> a b.h:"));
}

// CODE still has a separate, retained property-marker transport difference.
// Preserve every live method-origin CFG/RD fact without claiming that the
// complete graphs of these four diagnostic cases are exact.
#[test]
fn code_property_markers_preserve_method_origin_edges() {
    for name in [
        "fullname_property_in_code",
        "self_comment",
        "self_literal",
        "self_order_comment",
    ] {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/spaced-method-names/cases")
                .join(name)
                .join("expected.txt"),
        )
        .unwrap();
        let dump = cpg_lang_c::import::canonical_dump(&build_case(name));
        let mut checked = 0;
        for line in expected.lines().filter(|line| {
            line.starts_with("EDGES|CFG invoke#0 ->")
                || line.starts_with("FLOWS|REACHING_DEF[] invoke#0 ->")
        }) {
            assert!(dump.lines().any(|actual| actual == line), "{name}: {line}");
            checked += 1;
        }
        assert!(checked >= 3, "{name}");
    }
}
