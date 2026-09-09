//! Complete live graphs for primitive MEMBER spelling and retained type roles.
use std::path::Path;

fn assert_case(name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/primitive-members/cases")
        .join(name);
    let mut sources: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "c" || ext == "h"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(path).unwrap(),
            )
        })
        .collect();
    sources.sort();
    let borrowed: Vec<_> = sources
        .iter()
        .map(|(file, source)| (file.as_str(), source.as_str()))
        .collect();
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&borrowed);
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&project.cpg),
        std::fs::read_to_string(root.join("expected.txt")).unwrap(),
        "{name}"
    );
}

#[test]
fn primitive_members_match_complete_live_graphs() {
    for name in [
        "base_qualified_pointer",
        "const_unsigned_short",
        "mixed_declarator_shapes",
        "nonprimitive_scalar_control",
        "ordinary_numeric_control",
        "pointer_qualified_pointer",
        "short_int_unsigned",
        "short_unsigned",
        "signed_char",
        "signed_short",
        "unsigned_char",
        "unsigned_long_long_int",
        "unsigned_short",
        "unsigned_short_array",
        "unsigned_short_int",
        "unsigned_short_pointer",
        "volatile_unsigned_short",
    ] {
        assert_case(name);
    }
}

#[test]
fn retained_fixedtables_anchors_match_complete_live_graphs() {
    for name in ["tiny_fixedtables_include", "tiny_fixedtables_inline"] {
        assert_case(name);
    }
}
