//! Initializers register an additional type; declarations alone do not.
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};

fn build(source: &str) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("registration.c", source)]);
    project.cpg
}

macro_rules! fixture {
    ($case:literal) => {
        (
            $case,
            include_str!(concat!(
                "fixtures/declaration-registration/cases/",
                $case,
                "/registration.c"
            )),
            include_str!(concat!(
                "fixtures/declaration-registration/cases/",
                $case,
                "/expected.txt"
            )),
        )
    };
}

const CASES: &[(&str, &str, &str)] = &[
    fixture!("alias_pointer"),
    fixture!("alias_unsigned"),
    fixture!("float_initialized"),
    fixture!("float_pointer"),
    fixture!("for_initialized"),
    fixture!("for_uninitialized"),
    fixture!("global_array_string"),
    fixture!("global_array_callback"),
    fixture!("global_array_uninitialized"),
    fixture!("global_mixed_array"),
    fixture!("global_mixed_first"),
    fixture!("global_mixed_none"),
    fixture!("global_mixed_pointer"),
    fixture!("global_mixed_second"),
    fixture!("global_plain_initialized"),
    fixture!("global_plain_uninitialized"),
    fixture!("global_pointer_initialized"),
    fixture!("global_pointer_uninitialized"),
    fixture!("global_qualified"),
    fixture!("later_assignment"),
    fixture!("later_pointer_assignment"),
    fixture!("local_array_uninitialized"),
    fixture!("local_array_callback"),
    fixture!("local_mixed_array"),
    fixture!("local_mixed_first"),
    fixture!("local_mixed_none"),
    fixture!("local_mixed_pointer"),
    fixture!("local_mixed_second"),
    fixture!("local_plain_initialized"),
    fixture!("local_plain_uninitialized"),
    fixture!("local_pointer_initialized"),
    fixture!("local_pointer_uninitialized"),
    fixture!("mixed_callback_first"),
    fixture!("mixed_callback_initialized"),
    fixture!("mixed_global_callback"),
    fixture!("mixed_ordinary_initialized"),
    fixture!("mixed_ordinary_last"),
    fixture!("qualified_0_initialized"),
    fixture!("qualified_0_uninitialized"),
    fixture!("qualified_1_initialized"),
    fixture!("qualified_1_uninitialized"),
    fixture!("qualified_2_initialized"),
    fixture!("qualified_2_uninitialized"),
    fixture!("qualified_3_initialized"),
    fixture!("qualified_3_uninitialized"),
    fixture!("qualified_4_initialized"),
    fixture!("qualified_4_uninitialized"),
];

fn case_source(name: &str) -> &'static str {
    CASES.iter().find(|case| case.0 == name).unwrap().1
}

fn has_type(cpg: &Cpg, name: &str) -> bool {
    cpg.nodes()
        .any(|node| cpg.kind_of(node) == NodeKind::Type && cpg.full_name_of(node) == Some(name))
}

#[test]
fn registration_matches_each_complete_isolated_live_graph() {
    for &(case, source, expected) in CASES {
        // A fresh project prevents another initializer from hiding an extra
        // TYPE in the shared type pool. Compare every selected graph record.
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(source)),
            expected,
            "isolated fixture {case}",
        );
    }
}

#[test]
fn uninitialized_objects_keep_their_full_types_and_source_locations() {
    for (case, declared, absent) in [
        ("local_plain_uninitialized", "unsigned char", "unsigned"),
        ("global_pointer_uninitialized", "unsigned char*", "unsigned"),
        ("local_array_uninitialized", "unsigned char[2]", "unsigned"),
        ("global_array_uninitialized", "unsigned char[2]", "unsigned"),
        ("qualified_1_uninitialized", "short unsigned int*", "short"),
        (
            "qualified_2_uninitialized",
            "volatile long unsigned int*",
            "volatile",
        ),
        ("float_pointer", "longdouble*", "longdouble"),
    ] {
        let cpg = build(case_source(case));
        let local = cpg
            .nodes()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::Local && cpg.name_of(node) == Some("value")
            })
            .unwrap();
        assert_eq!(cpg.type_full_name_of(local), Some(declared), "{case}");
        assert_eq!(cpg.line_of(local), Some(1), "{case}");
        assert!(has_type(&cpg, declared), "{case}: full type retained");
        assert!(!has_type(&cpg, absent), "{case}: spurious prefix type");
        assert!(cpg
            .out_kind(local, EdgeKind::EvalType)
            .any(|node| cpg.full_name_of(node) == Some(declared)));
    }
}

#[test]
fn only_an_ordinary_objects_own_initializer_adds_the_prefix_type() {
    for (case, registered) in [
        ("local_plain_initialized", true),
        ("local_pointer_initialized", true),
        ("later_assignment", false),
        ("later_pointer_assignment", false),
        ("for_initialized", true),
        ("for_uninitialized", false),
        ("local_mixed_first", true),
        ("local_mixed_second", true),
        ("local_mixed_none", false),
        ("mixed_callback_initialized", false),
        ("mixed_callback_first", false),
        ("mixed_global_callback", false),
        ("mixed_ordinary_initialized", true),
        ("mixed_ordinary_last", true),
        ("local_array_callback", false),
        ("global_array_callback", false),
    ] {
        let cpg = build(case_source(case));
        assert_eq!(has_type(&cpg, "unsigned"), registered, "{case}");
        if matches!(
            case,
            "local_plain_initialized"
                | "local_pointer_initialized"
                | "later_assignment"
                | "later_pointer_assignment"
        ) {
            assert_eq!(
                cpg.calls()
                    .into_iter()
                    .filter(|&node| cpg.name_of(node) == Some("<operator>.assignment"))
                    .count(),
                1,
                "{case}: assignment retained regardless of registration",
            );
        }
    }
}

#[test]
fn tags_keep_their_independent_base_type_registration() {
    // These diagnostics retain unrelated preexisting scaffold differences;
    // their complete live references and diffs remain alongside the sources.
    // An initializer-only gate must not erase the independently required base
    // TYPE of a struct or enum pointer.
    for (source, base, pointer) in [
        (
            include_str!(
                "fixtures/declaration-registration/diagnostics/struct_pointer/registration.c"
            ),
            "Packet",
            "Packet*",
        ),
        (
            include_str!(
                "fixtures/declaration-registration/diagnostics/enum_pointer/registration.c"
            ),
            "Kind",
            "Kind*",
        ),
    ] {
        let cpg = build(source);
        assert!(has_type(&cpg, base), "{source}: tagged base retained");
        assert!(has_type(&cpg, pointer));
    }
}
