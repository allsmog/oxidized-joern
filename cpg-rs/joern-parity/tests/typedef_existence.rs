//! Typedef-name existence after declaration macro expansion, with complete live references.
use cpg_core::Query;

fn build(sources: &[(&str, &str)]) -> cpg_core::Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(sources);
    project.cpg
}

#[test]
fn typedef_existence_preserves_complete_exact_controls() {
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/known_outer_unknown_inner/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/known_outer_unknown_inner/expected.txt"),
        "known_outer_unknown_inner"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/no_typedef_nested/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/no_typedef_nested/expected.txt"),
        "no_typedef_nested"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/plain_type_contexts/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/plain_type_contexts/expected.txt"),
        "plain_type_contexts"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/function_alias/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/function_alias/expected.txt"),
        "function_alias"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/long_double/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/long_double/expected.txt"),
        "long_double"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/unsigned_char/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/unsigned_char/expected.txt"),
        "unsigned_char"
    );
    assert_eq!(
        cpg_lang_c::import::canonical_dump(&build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/unsigned_int/main.c")
        )])),
        include_str!("fixtures/typedef-existence/cases/unsigned_int/expected.txt"),
        "unsigned_int"
    );
}

#[test]
fn expanded_typedef_validity_preserves_cast_and_pointer_call_context() {
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/known_outer_unknown_inner/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(T)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.pointerCall".to_string(), "(U)(x)".to_string()),
                ("<operator>.pointerCall".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[
            (
                "lapi.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lapi.h"),
            ),
            (
                "lauxlib.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lauxlib.h"),
            ),
            (
                "lcode.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lcode.h"),
            ),
            (
                "lctype.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lctype.h"),
            ),
            (
                "ldebug.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/ldebug.h"),
            ),
            (
                "ldo.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/ldo.h"),
            ),
            (
                "lfunc.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lfunc.h"),
            ),
            (
                "lgc.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lgc.h"),
            ),
            (
                "ljumptab.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/ljumptab.h"),
            ),
            (
                "llex.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/llex.h"),
            ),
            (
                "llimits.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/llimits.h"),
            ),
            (
                "lmem.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lmem.h"),
            ),
            (
                "lobject.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lobject.h"),
            ),
            (
                "lopcodes.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lopcodes.h"),
            ),
            (
                "lopnames.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lopnames.h"),
            ),
            (
                "lparser.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lparser.h"),
            ),
            (
                "lprefix.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lprefix.h"),
            ),
            (
                "lstate.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lstate.h"),
            ),
            (
                "lstring.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lstring.h"),
            ),
            (
                "ltable.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/ltable.h"),
            ),
            (
                "ltm.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/ltm.h"),
            ),
            (
                "lua.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lua.h"),
            ),
            (
                "luaconf.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/luaconf.h"),
            ),
            (
                "lualib.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lualib.h"),
            ),
            (
                "lundump.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lundump.h"),
            ),
            (
                "lvm.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lvm.h"),
            ),
            (
                "lzio.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/lzio.h"),
            ),
            (
                "main.c",
                include_str!("fixtures/typedef-existence/diagnostics/lua_all_headers/main.c"),
            ),
        ]);
        assert_classification(
            &cpg,
            "direct",
            &[(
                "<operator>.pointerCall".to_string(),
                "(lua_Unsigned)(i)".to_string(),
            )],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(lua_Integer)(((lua_Unsigned)(i)) + ((lua_Unsigned)(j)))".to_string(),
                ),
                (
                    "<operator>.pointerCall".to_string(),
                    "(lua_Unsigned)(i)".to_string(),
                ),
                (
                    "<operator>.pointerCall".to_string(),
                    "(lua_Unsigned)(j)".to_string(),
                ),
            ],
        );
    }
    {
        let cpg = build(&[
            (
                "llimits.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_type_headers/llimits.h"),
            ),
            (
                "lprefix.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_type_headers/lprefix.h"),
            ),
            (
                "lua.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_type_headers/lua.h"),
            ),
            (
                "luaconf.h",
                include_str!("fixtures/typedef-existence/diagnostics/lua_type_headers/luaconf.h"),
            ),
            (
                "main.c",
                include_str!("fixtures/typedef-existence/diagnostics/lua_type_headers/main.c"),
            ),
        ]);
        assert_classification(
            &cpg,
            "direct",
            &[(
                "<operator>.pointerCall".to_string(),
                "(lua_Unsigned)(i)".to_string(),
            )],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(lua_Integer)(((lua_Unsigned)(i)) + ((lua_Unsigned)(j)))".to_string(),
                ),
                (
                    "<operator>.pointerCall".to_string(),
                    "(lua_Unsigned)(i)".to_string(),
                ),
                (
                    "<operator>.pointerCall".to_string(),
                    "(lua_Unsigned)(j)".to_string(),
                ),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/macro_underlying/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(U)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.cast".to_string(), "(U)(x)".to_string()),
                ("<operator>.cast".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/no_typedef_nested/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.pointerCall".to_string(),
                    "(U)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.pointerCall".to_string(), "(U)(x)".to_string()),
                ("<operator>.pointerCall".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/plain_type_contexts/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(U)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.cast".to_string(), "(U)(x)".to_string()),
                ("<operator>.cast".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/undefined_underlying/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.cast".to_string(),
                    "(U)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.cast".to_string(), "(U)(x)".to_string()),
                ("<operator>.cast".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/unsigned_undefined/main.c"),
        )]);
        assert_classification(
            &cpg,
            "direct",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
        assert_classification(
            &cpg,
            "add",
            &[
                (
                    "<operator>.pointerCall".to_string(),
                    "(U)(((U)(x)) + ((U)(y)))".to_string(),
                ),
                ("<operator>.pointerCall".to_string(), "(U)(x)".to_string()),
                ("<operator>.pointerCall".to_string(), "(U)(y)".to_string()),
            ],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/const_unknown/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/function_alias/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/local_invalid/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/long_double/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/long_unknown/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/signed_unknown/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/unsigned_char/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/cases/unsigned_int/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.cast".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/unsigned_int128/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
    }
    {
        let cpg = build(&[(
            "main.c",
            include_str!("fixtures/typedef-existence/diagnostics/unsigned_known_alias/main.c"),
        )]);
        assert_classification(
            &cpg,
            "check",
            &[("<operator>.pointerCall".to_string(), "(U)(x)".to_string())],
        );
    }
}

fn assert_classification(cpg: &cpg_core::Cpg, method_name: &str, expected: &[(String, String)]) {
    let methods = cpg.method_named(method_name);
    assert_eq!(methods.len(), 1, "{method_name}");
    let mut actual: Vec<_> = cpg_analysis::pass::ast_descendants(cpg, methods[0])
        .into_iter()
        .filter_map(|node| {
            let name = cpg.name_of(node)?;
            matches!(name, "<operator>.cast" | "<operator>.pointerCall").then(|| {
                (
                    name.to_string(),
                    cpg.code_of(node).unwrap_or_default().to_string(),
                )
            })
        })
        .collect();
    actual.sort();
    assert_eq!(actual, expected, "{method_name}");
}
