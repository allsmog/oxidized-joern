//! Production-graph regressions for external declarations and callable names.
//! Prototype shapes follow the pinned Joern v4.0.555 oracle fixtures; lexical
//! shadowing tests protect call resolution and REF bindings across block exits.
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};

fn build(files: &[(&str, &str)]) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(files);
    project.cpg
}

fn method(cpg: &Cpg, name: &str) -> NodeId {
    let methods = cpg.method_named(name);
    assert_eq!(methods.len(), 1, "method {name}: {methods:?}");
    methods[0]
}

fn calls_in(cpg: &Cpg, method_name: &str, code: &str) -> Vec<NodeId> {
    let mut calls: Vec<_> = cpg_analysis::pass::ast_descendants(cpg, method(cpg, method_name))
        .into_iter()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some(code))
        .collect();
    calls.sort_by_key(|&node| (cpg.line_of(node), node));
    calls
}

fn method_return(cpg: &Cpg, method: NodeId) -> NodeId {
    let returns: Vec<_> = cpg
        .out_kind(method, EdgeKind::Ast)
        .filter(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
        .collect();
    assert_eq!(returns.len(), 1);
    returns[0]
}

fn assert_direct(cpg: &Cpg, call: NodeId, name: &str) {
    assert_eq!(cpg.name_of(call), Some(name), "{:?}", cpg.code_of(call));
    assert_eq!(cpg.full_name_of(call), Some(name));
    assert_eq!(cpg.call_targets(call), vec![method(cpg, name)]);
}

fn assert_pointer(cpg: &Cpg, call: NodeId) {
    assert_eq!(
        cpg.name_of(call),
        Some("<operator>.pointerCall"),
        "{:?}",
        cpg.code_of(call)
    );
    assert_eq!(cpg.full_name_of(call), Some("<operator>.pointerCall"));
    assert!(cpg.call_targets(call).is_empty());
    let args = cpg.arguments_of(call);
    assert_eq!(args.len(), 1, "callee must not become an ordinary argument");
    assert_eq!(cpg.argument_index_of(args[0]), 1);
    assert_eq!(cpg.code_of(args[0]), Some("value"));
}

#[test]
fn late_prototypes_preserve_duplicate_static_definition_identity() {
    let cpg = build(&[
        ("a.c", "static int helper(int value) { return value; }\nstatic int helper(int value);\nint entry_a(int value) { return helper(value); }"),
        ("b.c", "static int helper(int value) { return value + 1; }\nint entry_b(int value) { return helper(value); }"),
    ]);
    let mut names: Vec<_> = cpg
        .method_named("helper")
        .into_iter()
        .map(|node| cpg.full_name_of(node).unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.c:helper", "b.c:helper"]);
    for (entry, target, file) in [
        ("entry_a", "a.c:helper", "a.c"),
        ("entry_b", "b.c:helper", "b.c"),
    ] {
        let calls = calls_in(&cpg, entry, "helper(value)");
        assert_eq!(calls.len(), 1);
        let targets = cpg.call_targets(calls[0]);
        assert_eq!(targets.len(), 1);
        assert_eq!(cpg.full_name_of(targets[0]), Some(target));
        assert_eq!(cpg.path_of(cpg.file_of(targets[0])), Some(file));
    }
}

#[test]
fn unused_multiple_void_unnamed_and_variadic_prototypes_keep_typed_structure() {
    let cpg = build(&[(
        "prototypes.c",
        r#"
int named(int value);
int named(int value);
int unnamed(int);
void nothing(void);
int empty();
int first(int value), second(char *text);
int variadic(const char *format, ...);
char *pointer(const char *text, int count);
"#,
    )]);
    for (name, signature, ret, params) in [
        (
            "named",
            "int(int)",
            "int",
            vec![("value", "int value", "int")],
        ),
        ("unnamed", "int(int)", "int", vec![("", "int", "int")]),
        ("nothing", "void(void)", "void", vec![("", "void", "void")]),
        ("empty", "int()", "int", vec![]),
        (
            "first",
            "int(int)",
            "int",
            vec![("value", "int value", "int")],
        ),
        (
            "second",
            "int(char*)",
            "int",
            vec![("text", "char *text", "char*")],
        ),
        (
            "variadic",
            "int(char*,...)",
            "int",
            vec![
                ("format", "const char *format", "char*"),
                ("<param>2", "<param>2...", "char*"),
            ],
        ),
        (
            "pointer",
            "char*(char*,int)",
            "char*",
            vec![
                ("text", "const char *text", "char*"),
                ("count", "int count", "int"),
            ],
        ),
    ] {
        let node = method(&cpg, name);
        assert_eq!(cpg.signature_of(node), Some(signature), "{name}");
        assert_eq!(
            cpg.type_full_name_of(method_return(&cpg, node)),
            Some(ret),
            "{name}"
        );
        assert_eq!(cpg.path_of(cpg.file_of(node)), Some("prototypes.c"));
        assert!(cpg.out_kind(node, EdgeKind::SourceFile).next().is_some());
        let inputs = cpg.parameters_of(node);
        assert_eq!(inputs.len(), params.len(), "{name}");
        let mut outputs: Vec<_> = cpg
            .out_kind(node, EdgeKind::Ast)
            .filter(|&n| cpg.kind_of(n) == NodeKind::MethodParameterOut)
            .collect();
        outputs.sort_by_key(|&n| cpg.order_of(n));
        assert_eq!(outputs.len(), inputs.len(), "{name}");
        for (index, ((param_name, code, ty), (input, output))) in params
            .into_iter()
            .zip(inputs.into_iter().zip(outputs))
            .enumerate()
        {
            for parameter in [input, output] {
                assert_eq!(cpg.name_of(parameter), Some(param_name), "{name} #{index}");
                assert_eq!(cpg.code_of(parameter), Some(code), "{name} #{index}");
                assert_eq!(
                    cpg.type_full_name_of(parameter),
                    Some(ty),
                    "{name} #{index}"
                );
                assert_eq!(cpg.order_of(parameter), (index + 1) as i32);
            }
        }
    }
}

#[test]
fn direct_callees_have_no_phantom_locals_and_keep_declared_return_types() {
    let cpg = build(&[(
        "calls.c",
        r#"
char *declared(char *value);
char *entry(char *value) { unknown(value); zero(); return declared(value); }
"#,
    )]);
    let entry = method(&cpg, "entry");
    for node in cpg_analysis::pass::ast_descendants(&cpg, entry) {
        if cpg.kind_of(node) == NodeKind::Local {
            assert!(!matches!(
                cpg.name_of(node),
                Some("declared" | "unknown" | "zero")
            ));
        }
    }
    let declared = calls_in(&cpg, "entry", "declared(value)")[0];
    assert_direct(&cpg, declared, "declared");
    assert_eq!(cpg.type_full_name_of(declared), Some("char*"));
    let zero = method(&cpg, "zero");
    let params = cpg.parameters_of(zero);
    assert_eq!(params.len(), 1);
    assert_eq!(cpg.name_of(params[0]), Some("p0"));
    assert_eq!(cpg.order_of(params[0]), 0);
    assert!(cpg
        .arguments_of(calls_in(&cpg, "entry", "zero()")[0])
        .is_empty());
}

#[test]
fn initialized_and_uninitialized_local_and_global_function_pointers_dispatch_dynamically() {
    let cpg = build(&[(
        "pointers.c",
        r#"
int target(int value) { return value; }
int (*global_initialized)(int) = target;
int (*global_uninitialized)(int);
int initialized(int value) { int (*callback)(int) = target; return callback(value); }
int uninitialized(int value) { int (*callback)(int); return callback(value); }
int global_one(int value) { return global_initialized(value); }
int global_two(int value) { return global_uninitialized(value); }
"#,
    )]);
    for (entry, code) in [
        ("initialized", "callback(value)"),
        ("uninitialized", "callback(value)"),
        ("global_one", "global_initialized(value)"),
        ("global_two", "global_uninitialized(value)"),
    ] {
        let calls = calls_in(&cpg, entry, code);
        assert_eq!(calls.len(), 1);
        assert_pointer(&cpg, calls[0]);
        assert_eq!(cpg.type_full_name_of(calls[0]), Some("int"));
    }
    let references: Vec<_> = cpg_analysis::pass::ast_descendants(&cpg, method(&cpg, "initialized"))
        .into_iter()
        .filter(|&node| {
            cpg.kind_of(node) == NodeKind::MethodRef && cpg.code_of(node) == Some("target")
        })
        .collect();
    assert_eq!(references.len(), 1);
    assert_eq!(cpg.full_name_of(references[0]), Some("target"));
    assert_eq!(cpg.type_full_name_of(references[0]), Some("int"));
    assert_eq!(cpg.line_of(references[0]), Some(5));
}

#[test]
fn block_pointer_shadow_does_not_change_later_direct_function_call() {
    // Live Joern: inner call is DYNAMIC_DISPATCH, outer call STATIC_DISPATCH.
    let cpg = build(&[(
        "scope.c",
        r#"int function(int value) { return value; }
int entry(int (*callback)(int), int value) {
  if (value) { int (*function)(int) = callback; function(value); }
  return function(value);
}"#,
    )]);
    let calls = calls_in(&cpg, "entry", "function(value)");
    assert_eq!(calls.len(), 2);
    assert_pointer(&cpg, calls[0]);
    assert_direct(&cpg, calls[1], "function");
    assert_eq!(cpg.line_of(calls[0]), Some(3));
    assert_eq!(cpg.line_of(calls[1]), Some(4));
}

#[test]
fn leaving_a_block_restores_the_shadowed_parameter_ref_binding() {
    let cpg = build(&[(
        "ref_scope.c",
        r#"void consume(double value);
int entry(int value) {
  if (value) { double value = 1.5; consume(value); }
  return value;
}"#,
    )]);
    let entry = method(&cpg, "entry");
    let parameter = cpg.parameters_of(entry)[0];
    let returned = cpg_analysis::pass::ast_descendants(&cpg, entry)
        .into_iter()
        .find(|&node| cpg.kind_of(node) == NodeKind::Return)
        .unwrap();
    let value = cpg.arguments_of(returned)[0];
    assert_eq!(cpg.type_full_name_of(value), Some("int"));
    assert_eq!(
        cpg.out_kind(value, EdgeKind::Ref).collect::<Vec<_>>(),
        vec![parameter]
    );
    let consume = calls_in(&cpg, "entry", "consume(value)")[0];
    let inner = cpg.arguments_of(consume)[0];
    let inner_binding: Vec<_> = cpg.out_kind(inner, EdgeKind::Ref).collect();
    assert_eq!(inner_binding.len(), 1);
    assert_eq!(cpg.kind_of(inner_binding[0]), NodeKind::Local);
    assert_eq!(cpg.type_full_name_of(inner), Some("double"));
}

#[test]
fn block_prototype_shadows_a_pointer_parameter_only_within_that_block() {
    let cpg = build(&[(
        "prototype_scope.c",
        r#"int entry(int (*callable)(int), int value) {
  if (value) { extern int callable(int); callable(value); }
  return callable(value);
}"#,
    )]);
    let calls = calls_in(&cpg, "entry", "callable(value)");
    assert_eq!(calls.len(), 2);
    assert_direct(&cpg, calls[0], "callable");
    assert_pointer(&cpg, calls[1]);
    let parameter = cpg.parameters_of(method(&cpg, "entry"))[0];
    let receiver = cpg
        .out_kind(calls[1], EdgeKind::Ast)
        .find(|&node| {
            cpg.kind_of(node) == NodeKind::Identifier && cpg.name_of(node) == Some("callable")
        })
        .unwrap();
    assert_eq!(
        cpg.out_kind(receiver, EdgeKind::Ref).collect::<Vec<_>>(),
        vec![parameter]
    );
}

fn for_scope_graph() -> Cpg {
    build(&[(
        "for_scope.c",
        r#"int function(int value) { return value; }
int pointer_entry(int (*callback)(int), int value) {
  for (int (*function)(int) = callback; value; value = 0) { function(value); }
  return function(value);
}
void consume(double value);
int parameter_entry(int value) {
  for (double value = 0.0; value < 1.0; value++) { consume(value); }
  return value;
}"#,
    )])
}

#[test]
fn for_initializer_scope_restores_function_dispatch_and_parameter_binding() {
    let cpg = for_scope_graph();
    let calls = calls_in(&cpg, "pointer_entry", "function(value)");
    assert_eq!(calls.len(), 2);
    let outer = calls
        .iter()
        .copied()
        .find(|&call| {
            cpg.in_kind(call, EdgeKind::Ast)
                .any(|parent| cpg.kind_of(parent) == NodeKind::Return)
        })
        .unwrap();
    let inner = calls.into_iter().find(|&call| call != outer).unwrap();
    assert_pointer(&cpg, inner);
    assert_direct(&cpg, outer, "function");
    let entry = method(&cpg, "parameter_entry");
    let parameter = cpg.parameters_of(entry)[0];
    let returned = cpg_analysis::pass::ast_descendants(&cpg, entry)
        .into_iter()
        .find(|&node| cpg.kind_of(node) == NodeKind::Return)
        .unwrap();
    let value = cpg.arguments_of(returned)[0];
    assert_eq!(cpg.type_full_name_of(value), Some("int"));
    assert_eq!(
        cpg.out_kind(value, EdgeKind::Ref).collect::<Vec<_>>(),
        vec![parameter]
    );
    let consume = calls_in(&cpg, "parameter_entry", "consume(value)")[0];
    assert_eq!(
        cpg.type_full_name_of(cpg.arguments_of(consume)[0]),
        Some("double")
    );
}

#[test]
fn synthetic_for_condition_does_not_shift_real_call_locations() {
    let cpg = for_scope_graph();
    let calls = calls_in(&cpg, "pointer_entry", "function(value)");
    assert_eq!(calls.len(), 2);
    for call in calls {
        let returned = cpg
            .in_kind(call, EdgeKind::Ast)
            .any(|parent| cpg.kind_of(parent) == NodeKind::Return);
        assert_eq!(
            cpg.line_of(call),
            Some(if returned { 4 } else { 3 }),
            "synthetic condition must not consume update/body tokens: {:?}",
            cpg.name_of(call)
        );
    }
}
