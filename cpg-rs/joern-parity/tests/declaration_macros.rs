//! Complete pinned Joern graphs for declaration macro and supplied-header types.
use cpg_core::Query;
use std::path::Path;

fn build_case(name: &str) -> cpg_core::Cpg {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/declaration-macros/cases")
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
fn declaration_macros_match_complete_isolated_live_graphs() {
    let cases = [
        "alias_return",
        "empty_prefix",
        "header_caller_state",
        "header_include_order",
        "header_macro",
        "header_missing",
        "header_plain",
        "noret_attribute",
        "order",
        "pointer_alias",
        "prototype_return",
        "unknown_type",
        "alias_chain",
        "array_param",
        "attribute_prefix",
        "empty_macro",
        "function_macro_decl",
        "header_relative",
        "header_two_callers",
        "header_undef",
        "pointer_params",
        "qualifiers",
        "quoted_attribute",
        "self_recursive",
        "ucn_negative",
        "unicode_negative",
        "commented_prototype",
        "function_macro_missing",
        "ordinary_comment",
    ];
    for name in cases {
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/declaration-macros/cases")
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
fn header_call_types_follow_each_importing_translation_unit() {
    let cpg = build_case("header_two_callers");
    for (name, expected) in [("a", "longint"), ("b", "int")] {
        let method = cpg.method_named(name)[0];
        let call = cpg_analysis::pass::ast_descendants(&cpg, method)
            .into_iter()
            .find(|&node| cpg.name_of(node) == Some("f"))
            .unwrap();
        assert_eq!(cpg.type_full_name_of(call), Some(expected), "{name}");
        assert!(cpg.line_of(call).is_some());
    }
}

#[test]
fn empty_declaration_prefix_retains_method_and_call_locations() {
    let cpg = build_case("empty_macro");
    let method = cpg.method_named("g")[0];
    assert_eq!(cpg.line_of(method), Some(3));
    let call = cpg_analysis::pass::ast_descendants(&cpg, method)
        .into_iter()
        .find(|&node| cpg.name_of(node) == Some("f"))
        .unwrap();
    assert_eq!(cpg.line_of(call), Some(3));
    assert_eq!(cpg.line_of(cpg.arguments_of(call)[0]), Some(3));
}

#[test]
fn multiline_empty_prefix_and_conditional_duplicates_keep_separate_source_ranges() {
    let source = "#define API\nAPI\nint same(int x) {\n  return x;\n}\n#if 0\nAPI\nint same(int x) {\n  return x + 1;\n}\n#else\nAPI\nint same(int x) {\n  return x + 2;\n}\n#endif\n";
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("locations.c", source)]);
    let cpg = project.cpg;
    for (full, method_line, parameter_line, return_line) in [
        ("same", 3, 3, Some(4)),
        // Preserved baseline raw-prefix anchoring: Joern reports line 8 for
        // this inactive method/exit. This assertion is not location parity.
        ("same<duplicate>0", 7, 8, None),
        ("same<duplicate>1", 13, 13, Some(14)),
    ] {
        let method = cpg
            .method_named("same")
            .into_iter()
            .find(|&method| cpg.full_name_of(method) == Some(full))
            .unwrap();
        assert_eq!(cpg.line_of(method), Some(method_line), "{full}");
        let nodes = cpg_analysis::pass::ast_descendants(&cpg, method);
        let parameter = *nodes
            .iter()
            .find(|&&node| cpg.kind_of(node) == cpg_core::NodeKind::MethodParameterIn)
            .unwrap();
        assert_eq!(cpg.line_of(parameter), Some(parameter_line), "{full}");
        let returned = nodes
            .iter()
            .find(|&&node| cpg.kind_of(node) == cpg_core::NodeKind::Return)
            .copied();
        assert_eq!(
            returned.and_then(|node| cpg.line_of(node)),
            return_line,
            "{full}"
        );
        let method_return = *nodes
            .iter()
            .find(|&&node| cpg.kind_of(node) == cpg_core::NodeKind::MethodReturn)
            .unwrap();
        assert_eq!(cpg.line_of(method_return), Some(method_line), "{full}");
    }
}
