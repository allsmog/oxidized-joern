use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind};
use cpg_frontend::Frontend;

fn graph(files: &[(&str, &str)]) -> Cpg {
    cpg_lang_c::CFrontend::new().build_project(files).unwrap()
}

fn method(cpg: &Cpg, full: &str) -> NodeId {
    cpg.nodes()
        .find(|&node| cpg.kind_of(node) == NodeKind::Method && cpg.full_name_of(node) == Some(full))
        .unwrap_or_else(|| panic!("missing method {full}"))
}

#[test]
fn function_typedefs_do_not_erase_neighboring_ordinary_aliases() {
    let cpg = graph(&[(
        "typedef_shapes.c",
        include_str!("../../joern-parity/corpus/typedef_shapes.c"),
    )]);
    for absent in ["FunctionType", "CallbackFirst", "CallbackSecond"] {
        assert!(!cpg.nodes().any(|node| {
            cpg.kind_of(node) == NodeKind::TypeDecl && cpg.name_of(node) == Some(absent)
        }));
    }
    for (name, order) in [
        ("ScalarFirst", 1),
        ("ScalarSecond", 2),
        ("One", 3),
        ("Two", 4),
    ] {
        let node = cpg
            .nodes()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::TypeDecl && cpg.name_of(node) == Some(name)
            })
            .unwrap();
        assert_eq!(cpg.order_of(node), order, "{name}");
    }
}

#[test]
fn mixed_prototypes_keep_individual_identities_and_shared_return_type() {
    let cpg = graph(&[(
        "mixed_prototypes.c",
        include_str!("../../joern-parity/corpus/mixed_prototypes.c"),
    )]);
    for name in ["paren_first", "paren_second", "macro_paren"] {
        let prototype = method(&cpg, &format!("<unresolvedNamespace>.{name}"));
        assert_eq!(cpg.name_of(prototype), Some(name));
        assert_eq!(cpg.signature_of(prototype), Some("int(int)"));
        assert_eq!(cpg.name_of(method(&cpg, name)), Some(name));
    }
    for name in ["ordinary_first", "ordinary_second", "macro_ordinary"] {
        assert_eq!(cpg.signature_of(method(&cpg, name)), Some("int(int)"));
        assert!(!cpg.nodes().any(|node| {
            cpg.full_name_of(node) == Some(format!("<unresolvedNamespace>.{name}").as_str())
        }));
    }
    let code = "extern int extern int  ()(int) extern int (int);";
    assert_eq!(
        cpg.code_of(method(&cpg, "<unresolvedNamespace>.macro_paren")),
        Some(code)
    );
    assert_eq!(cpg.code_of(method(&cpg, "macro_ordinary")), Some(code));
}

#[test]
fn parenthesized_definition_keeps_its_body_and_separate_call_identity() {
    let cpg = graph(&[(
        "parenthesized_definitions.c",
        include_str!("../../joern-parity/corpus/parenthesized_definitions.c"),
    )]);
    let defined = method(&cpg, "<unresolvedNamespace>.paren_defined");
    let body = cpg
        .out_kind(defined, EdgeKind::Ast)
        .find(|&node| cpg.kind_of(node) == NodeKind::Block)
        .unwrap();
    assert!(cpg
        .out_kind(body, EdgeKind::Ast)
        .any(|node| cpg.kind_of(node) == NodeKind::Return));
    assert_eq!(
        cpg.name_of(method(&cpg, "paren_defined")),
        Some("paren_defined")
    );
    let projection = cpg_lang_c::import::canonical_dump(&cpg);
    assert!(projection.contains("TYPE_DECL NAME=paren_defined FULL_NAME=<unresolvedNamespace>.paren_defined CODE=<unresolvedNamespace>.paren_defined AST_PARENT_TYPE=TYPE_DECL"));
}

#[test]
fn quoted_header_macros_resolve_relative_to_each_supplied_source() {
    for prefix in ["", "/nonexistent/project/"] {
        let paths = [
            format!("{prefix}src/lua.h"),
            format!("{prefix}src/lualib.h"),
            format!("{prefix}other/lua.h"),
            format!("{prefix}other/lualib.h"),
        ];
        let cpg = graph(&[
            (&paths[0], "#define LUA_API extern\n"),
            (
                &paths[1],
                "#include \"./lua.h\"\nLUA_API int (relative_lua)(int value);\n",
            ),
            (&paths[2], "#define LUA_API static\n"),
            (
                &paths[3],
                "#include \"sub/../lua.h\"\nLUA_API int (relative_other)(int value);\n",
            ),
        ]);
        assert_eq!(
            cpg.code_of(method(&cpg, "<unresolvedNamespace>.relative_lua")),
            Some("extern int extern int  ()(int);")
        );
        assert_eq!(
            cpg.code_of(method(&cpg, "<unresolvedNamespace>.relative_other")),
            Some("static int static int  ()(int);")
        );
        assert!(!cpg
            .nodes()
            .any(|node| cpg.kind_of(node) == NodeKind::Unknown));
    }
}

#[test]
fn headers_apply_undef_and_redefinitions_in_order_without_basename_fallback() {
    let cpg = graph(&[
        ("src/change.h", "#undef DROP_API\n#define KEEP_API extern\n#define TEMP_API extern\n#undef TEMP_API\n#define TEMP_API static\n"),
        ("src/caller.c", "#define DROP_API extern\n#include \"change.h\"\nDROP_API int (dropped)(int value);\nKEEP_API int (kept)(int value);\nTEMP_API int (redefined)(int value);\n"),
        ("elsewhere/missing.h", "#define MISSING_API extern\n"),
        ("src/missing.c", "#include \"missing.h\"\nMISSING_API int (missing)(int value);\n"),
    ]);
    for (name, code) in [
        ("dropped", "int (dropped)(int value);"),
        ("kept", "extern int extern int  ()(int);"),
        ("redefined", "static int static int  ()(int);"),
        ("missing", "int (missing)(int value);"),
    ] {
        assert_eq!(
            cpg.code_of(method(&cpg, &format!("<unresolvedNamespace>.{name}"))),
            Some(code)
        );
    }
    for unknown in ["DROP_API", "MISSING_API"] {
        assert!(cpg.nodes().any(
            |node| cpg.kind_of(node) == NodeKind::Unknown && cpg.code_of(node) == Some(unknown)
        ));
    }
}

#[test]
fn duplicate_parenthesized_definitions_keep_calls_to_the_shared_plain_stub() {
    for storage in ["", "static "] {
        let a = format!("{storage}int (helper)(int value) {{ return value; }}\nint use_a(int value) {{ return helper(value); }}\n");
        let b = format!("{storage}int (helper)(int value) {{ return value; }}\nint use_b(int value) {{ return helper(value); }}\n");
        let cpg = graph(&[("a.c", &a), ("b.c", &b)]);
        method(&cpg, "<unresolvedNamespace>.helper");
        method(&cpg, "<unresolvedNamespace>.helper<duplicate>0");
        let stub = method(&cpg, "helper");
        let calls: Vec<_> = cpg
            .nodes()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some("helper")
            })
            .collect();
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert_eq!(
                cpg.out_kind(call, EdgeKind::Call).collect::<Vec<_>>(),
                vec![stub]
            );
        }
    }
}
