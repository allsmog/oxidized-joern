use javaastgen_core::{collect_kind_counts, parse_source};
use std::path::Path;

#[test]
fn broad_java_fixture_exposes_constructs_needed_by_javasrc2cpg() {
    let source = r#"
package demo.coverage;

import java.util.List;
import java.util.function.Function;
import module java.base;

@Sample.Marker("class")
public class Sample<T extends Number> implements Runnable {
  private final int[] values = new int[] { 1, 2, 3 };

  public Sample() {
    this(1);
  }

  public Sample(int seed) {
    for (int i = 0; i < values.length; i++) {
      values[i] += seed;
    }
  }

  @Override
  public void run() {
    try {
      List<String> names = List.of(" a ");
      Function<String, String> trim = name -> name.trim();
      names.stream().map(trim).forEach(System.out::println);
    } catch (RuntimeException ex) {
      throw ex;
    } finally {
      synchronized (this) {
        assert values.length > 0;
      }
    }
  }

  public int choose(int value) {
    switch (value) {
      case 0:
        return values[0];
      default:
        return value;
    }
  }

  public record Pair(int left, int right) {}
  public enum Mode { A, B }
  public @interface Marker { String value(); }
}
"#;

    let document = parse_source(Path::new("."), Path::new("Sample.java"), source).unwrap();
    let counts = collect_kind_counts(&document);

    for kind in [
        "program",
        "package_declaration",
        "import_declaration",
        "class_declaration",
        "modifiers",
        "annotation",
        "type_parameters",
        "field_declaration",
        "array_type",
        "array_creation_expression",
        "constructor_declaration",
        "method_declaration",
        "for_statement",
        "assignment_expression",
        "try_statement",
        "catch_clause",
        "finally_clause",
        "synchronized_statement",
        "assert_statement",
        "lambda_expression",
        "method_reference",
        "switch_expression",
        "record_declaration",
        "enum_declaration",
        "annotation_type_declaration",
    ] {
        assert!(counts.contains_key(kind), "missing tree-sitter kind {kind}");
    }

    assert!(
        !document.ast.has_error,
        "coverage source parsed with tree-sitter errors"
    );
}
