//! Source-position header return bindings, with complete pinned graph comparisons.
use cpg_core::{NodeKind, Query};
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/header-return-bindings/cases")
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
fn supplied_header_bindings_match_complete_live_graphs() {
    for name in [
        "header_alias_parenthesized",
        "header_parenthesized",
        "header_parenthesized_pointer",
        "header_plain_macro_alias",
        "parenthesized_same_file",
        "alias_chain",
        "primitive_alias",
        "struct_alias",
        "unknown_return",
        "caller_alias",
        "cycle_alias",
        "parenthesized_unknown",
        "prefix_primitive",
        "prefix_unknown",
        "review_anonymous_alias",
        "review_late_alias",
        "review_named_alias",
    ] {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/header-return-bindings/cases")
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

// These target assertions include retained nonconformant scaffold diagnostics;
// complete equality is claimed only by the separate test above.
#[test]
fn header_call_bindings_resolve_known_aliases_and_keep_unknown_targets_unknown() {
    for (case, expected) in [
        ("unknown_alias", "ANY"),
        ("unknown_return", "ANY"),
        ("unknown_pointer_alias", "ANY"),
        ("unknown_macro_alias", "ANY"),
        ("primitive_alias", "longint"),
        ("alias_chain", "longint"),
        ("macro_alias", "longint"),
        ("struct_alias", "Tag*"),
        ("struct_same", "I*"),
        ("forward_tag", "Tag*"),
        ("caller_alias", "longint"),
        ("late_alias", "ANY"),
        ("cycle_alias", "ANY"),
        ("pointer_alias", "longint*"),
    ] {
        let cpg = build_case(case);
        let method = cpg.method_named("g")[0];
        let calls: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method)
            .into_iter()
            .filter(|&node| cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some("f"))
            .collect();
        assert_eq!(calls.len(), 1, "{case}");
        assert_eq!(cpg.type_full_name_of(calls[0]), Some(expected), "{case}");
    }
}

#[test]
fn expanded_header_recovery_cannot_invent_a_callable_named_after_its_return_type() {
    for case in ["prefix_unknown", "parenthesized_unknown"] {
        let cpg = build_case(case);
        let method = cpg.method_named("g")[0];
        let nodes = cpg_analysis::pass::ast_descendants(&cpg, method);
        assert!(
            nodes
                .iter()
                .any(|&node| cpg.kind_of(node) == NodeKind::Local
                    && cpg.name_of(node) == Some("Unknown")),
            "{case}"
        );
        assert!(
            !nodes
                .iter()
                .any(|&node| cpg.kind_of(node) == NodeKind::MethodRef
                    && cpg.code_of(node) == Some("Unknown")),
            "{case}"
        );
        let call = nodes
            .into_iter()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some("declared")
            })
            .unwrap();
        assert_eq!(cpg.type_full_name_of(call), Some("ANY"), "{case}");
    }
}

// Full diagnostic graphs remain retained: these assertions pin the binding
// recovery behavior independently of their unrelated declaration scaffolding.
#[test]
fn invalid_redeclarations_do_not_erase_valid_recovered_typedef_bindings() {
    for (case, method_name, call_name, expected) in [
        (
            "review_invalid_redeclaration",
            "read_value",
            "get_value",
            "int",
        ),
        ("recovery_valid_then_error", "g", "f", "int"),
        ("recovery_error_then_valid", "g", "f", "longint"),
    ] {
        let cpg = build_case(case);
        let method = cpg.method_named(method_name)[0];
        let call = cpg_analysis::pass::ast_descendants(&cpg, method)
            .into_iter()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some(call_name)
            })
            .unwrap();
        assert_eq!(cpg.type_full_name_of(call), Some(expected), "{case}");
    }
}
