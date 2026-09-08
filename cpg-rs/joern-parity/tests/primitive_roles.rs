//! Primitive spellings differ by declaration, definition and expression role.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build(file: &str, source: &str) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(file, source)]);
    project.cpg
}

macro_rules! fixture {
    ($case:literal, $file:literal) => {
        (
            $case,
            $file,
            include_str!(concat!("fixtures/primitive-roles/", $case, "/", $file)),
            include_str!(concat!("fixtures/primitive-roles/", $case, "/expected.txt")),
        )
    };
}

const CASES: &[(&str, &str, &str, &str)] = &[
    fixture!("type_00", "types.c"),
    fixture!("type_01", "types.c"),
    fixture!("type_02", "types.c"),
    fixture!("type_03", "types.c"),
    fixture!("type_04", "types.c"),
    fixture!("type_05", "types.c"),
    fixture!("type_06", "types.c"),
    fixture!("type_07", "types.c"),
    fixture!("type_08", "types.c"),
    fixture!("type_09", "types.c"),
    fixture!("type_10", "types.c"),
    fixture!("type_11", "types.c"),
    fixture!("type_12", "types.c"),
    fixture!("type_13", "types.c"),
    fixture!("type_14", "types.c"),
    fixture!("type_15", "types.c"),
    fixture!("type_16", "types.c"),
    fixture!("type_17", "types.c"),
    fixture!("type_18", "types.c"),
    fixture!("type_19", "types.c"),
    fixture!("type_20", "types.c"),
    fixture!("type_21", "types.c"),
    fixture!("type_22", "types.c"),
    fixture!("type_23", "types.c"),
    fixture!("type_24", "types.c"),
    fixture!("type_25", "types.c"),
    fixture!("type_26", "types.c"),
    fixture!("type_27", "types.c"),
    fixture!("type_28", "types.c"),
    fixture!("type_29", "types.c"),
    fixture!("type_30", "types.c"),
    fixture!("type_31", "types.c"),
    fixture!("type_32", "types.c"),
    fixture!("type_33", "types.c"),
    fixture!("type_34", "types.c"),
    fixture!("pointers", "roles.c"),
    fixture!("paren_calls", "paren_calls.c"),
    fixture!("callback_parameters", "callback_parameters.c"),
    fixture!("for_callbacks", "for_callbacks.c"),
];

#[test]
fn primitive_roles_match_each_complete_isolated_live_graph() {
    for &(case, file, source, expected) in CASES {
        // A fresh CPG for every spelling prevents an unrelated declaration
        // from hiding a missing or extra TYPE registration.
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(file, source)),
            expected,
            "isolated fixture {case}",
        );
    }
}

#[test]
fn definition_prototype_and_call_result_types_stay_distinct() {
    for (index, defined, declared, call) in [
        (19, "unsigned long", "longunsigned", "unsigned longint"),
        (
            21,
            "unsigned longint",
            "long unsigned int",
            "unsigned longint",
        ),
        (32, "bool", "_Bool", "_Bool"),
        (
            34,
            "volatile unsigned long",
            "volatile longunsigned",
            "volatile unsigned longint",
        ),
    ] {
        let (_, file, source, _) = CASES[index];
        let cpg = build(file, source);
        for (name, expected) in [("identity", defined), ("declared", declared)] {
            let method = cpg.method_named(name);
            assert_eq!(method.len(), 1);
            let returned = cpg
                .out_kind(method[0], EdgeKind::Ast)
                .find(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
                .unwrap();
            assert_eq!(
                cpg.type_full_name_of(returned),
                Some(expected),
                "{index}: {name}"
            );
            let parameters = cpg.parameters_of(method[0]);
            assert_eq!(parameters.len(), 1);
            assert_eq!(cpg.type_full_name_of(parameters[0]), Some(declared));
        }
        for name in ["identity", "declared"] {
            let calls: Vec<_> = cpg
                .calls()
                .into_iter()
                .filter(|&node| cpg.name_of(node) == Some(name))
                .collect();
            assert_eq!(calls.len(), 1);
            assert_eq!(cpg.type_full_name_of(calls[0]), Some(call));
            assert!(cpg.line_of(calls[0]).is_some(), "call location retained");
        }
    }
}

#[test]
fn function_pointer_parameters_keep_declaration_types_and_resolve_call_results() {
    let (_, file, source, _) = CASES[35];
    let cpg = build(file, source);
    let method = cpg.method_named("callback_parameter")[0];
    let parameter = cpg.parameters_of(method)[0];
    assert_eq!(cpg.type_full_name_of(parameter), Some("longunsigned"));
    let call = cpg
        .calls()
        .into_iter()
        .find(|&node| cpg.code_of(node) == Some("callback(value)"))
        .unwrap();
    assert_eq!(cpg.type_full_name_of(call), Some("unsigned longint"));
    let object = cpg
        .nodes()
        .find(|&node| {
            cpg.kind_of(node) == NodeKind::Local && cpg.name_of(node) == Some("local_callback")
        })
        .unwrap();
    assert_eq!(
        cpg.type_full_name_of(object),
        Some("unsigned longint(*)(unsignedlongint)")
    );
}
