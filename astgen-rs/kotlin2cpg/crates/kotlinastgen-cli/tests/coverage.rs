use kotlinastgen_core::{collect_kind_counts, parse_file};
use std::path::Path;

#[test]
fn fixture_exercises_core_kotlin_kinds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/kotlin-corpus");
    let sample = root.join("language_features").join("Sample.kt");
    let document = parse_file(&root, &sample).unwrap();
    let counts = collect_kind_counts(&document);

    for kind in [
        "source_file",
        "package_header",
        "import_header",
        "class_declaration",
        "primary_constructor",
        "property_declaration",
        "function_declaration",
        "if_expression",
        "when_expression",
        "lambda_literal",
        "call_expression",
    ] {
        assert!(counts.contains_key(kind), "missing kind {kind}");
    }
}
