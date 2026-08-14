use cpg_core::{NodeKind, Query};
use cpg_frontend::Frontend;
use cpg_lang_c::CFrontend;

fn graph(source: &str) -> cpg_core::Cpg {
    CFrontend::new()
        .build_project(&[("preprocessor.c", source)])
        .expect("project CPG")
}

#[test]
fn evaluates_if_elif_defined_and_function_macros() {
    let cpg = graph(
        r#"
#define LEVEL 3
#define ENABLED 1
#define VERSION_AT_LEAST(x, y) (((x) * 10 + (y)) >= 32)

int selected(void) {
#if LEVEL >= 3 && defined(ENABLED)
    live_primary();
#elif LEVEL == 2
    dead_elif();
#else
    dead_else();
#endif
#if VERSION_AT_LEAST(3, 2)
    live_function_macro();
#endif
#if UNDEFINED_NAME
    dead_undefined();
#endif
#if !defined(MISSING)
    live_not_defined();
#endif
    return 0;
}
"#,
    );

    for live in ["live_primary", "live_function_macro", "live_not_defined"] {
        assert_eq!(cpg.calls_named(live).len(), 1, "missing {live}");
    }
    for dead in ["dead_elif", "dead_else", "dead_undefined"] {
        assert!(
            cpg.calls_named(dead).is_empty(),
            "inactive call leaked: {dead}"
        );
    }
}

#[test]
fn inactive_symbols_and_top_level_declarations_do_not_leak() {
    let cpg = graph(
        r#"
#define BUILD_LIVE 1
#if BUILD_LIVE
int live_function(void) { return 1; }
#else
int dead_function(void) { return missing_global; }
#endif

#undef BUILD_LIVE
#if BUILD_LIVE
int dead_after_undef(void) { return 2; }
#else
int live_after_undef(void) { return 3; }
#endif
"#,
    );

    assert_eq!(cpg.method_named("live_function").len(), 1);
    assert_eq!(cpg.method_named("live_after_undef").len(), 1);
    assert!(cpg.method_named("dead_function").is_empty());
    assert!(cpg.method_named("dead_after_undef").is_empty());
    assert!(cpg
        .nodes_of_kind(NodeKind::Identifier)
        .into_iter()
        .all(|node| cpg.name_of(node) != Some("missing_global")));
}

#[test]
fn expands_nested_variadic_stringized_and_pasted_macros() {
    let cpg = graph(
        r#"
#define INNER(x) ((x) + 1)
#define OUTER(x) INNER(x)
#define STRINGIZE(x) #x
#define CONCAT(left, right) left ## right
#define FIRST(first, ...) first

int macro_advanced(int input) {
    int token_value = 3;
    const char *text = STRINGIZE(input);
    return OUTER(input) + CONCAT(token_, value) + FIRST(input, 11, 12);
}
"#,
    );

    for invocation in ["OUTER", "STRINGIZE", "CONCAT", "FIRST"] {
        assert_eq!(cpg.calls_named(invocation).len(), 1, "missing {invocation}");
    }
    assert!(
        cpg.calls_named("INNER").is_empty(),
        "nested replacement is expanded inside OUTER, not emitted as another invocation"
    );
    assert!(cpg
        .nodes_of_kind(NodeKind::Literal)
        .into_iter()
        .any(|node| cpg.code_of(node) == Some("\"input\"")));
    assert!(cpg
        .nodes_of_kind(NodeKind::Identifier)
        .into_iter()
        .any(|node| cpg.name_of(node) == Some("token_value")));
    for consumed in ["token_", "value"] {
        assert!(cpg
            .nodes_of_kind(NodeKind::Local)
            .into_iter()
            .all(|node| cpg.name_of(node) != Some(consumed)));
    }
}

#[test]
fn recursive_and_incomplete_macro_expansions_are_bounded() {
    let cpg = graph(
        r#"
#define FIRST(value) SECOND(value)
#define SECOND(value) FIRST(value)
#define INCOMPLETE(value) ((value) ? (value))

int recursive_macro(int value) {
    return FIRST(value);
}

int incomplete_macro(int value) {
    return INCOMPLETE(value);
}
"#,
    );

    assert_eq!(cpg.method_named("recursive_macro").len(), 1);
    assert_eq!(cpg.method_named("incomplete_macro").len(), 1);
    assert_eq!(cpg.calls_named("FIRST").len(), 2);
    assert_eq!(cpg.calls_named("INCOMPLETE").len(), 1);
}
