//! Supplied-header macro expansion, compared against complete isolated Joern graphs.
use std::path::Path;
use std::process::Command;

struct Case {
    name: &'static str,
    sources: &'static [(&'static str, &'static str)],
    expected: &'static str,
}

macro_rules! fixture {
    ($name:literal, [$($path:literal),+]) => {
        Case {
            name: $name,
            sources: &[$(($path, include_str!(concat!("fixtures/header-macros/cases/", $name, "/input/", $path)))),+],
            expected: include_str!(concat!("fixtures/header-macros/cases/", $name, "/expected.txt")),
        }
    };
}

const CASES: &[Case] = &[
    fixture!("caller_object_define", ["defs.h", "main.c"]),
    fixture!("cast_spaced", ["defs.h", "main.c"]),
    fixture!("cast_unspaced", ["defs.h", "main.c"]),
    fixture!("definition_after_function", ["defs.h", "main.c"]),
    fixture!("header_object", ["defs.h", "main.c"]),
    fixture!("identity_add_spaced", ["defs.h", "main.c"]),
    fixture!("identity_add_unspaced", ["defs.h", "main.c"]),
    fixture!("identity_call_spaced", ["defs.h", "main.c"]),
    fixture!("identity_call_unspaced", ["defs.h", "main.c"]),
    fixture!("ignored_argument", ["defs.h", "main.c"]),
    fixture!("include_guard_twice", ["defs.h", "main.c"]),
    fixture!("indirect_recursive_object", ["defs.h", "main.c"]),
    fixture!("initializer_macro", ["defs.h", "main.c"]),
    fixture!("missing_include", ["main.c"]),
    fixture!("nested_call_wrapper", ["defs.h", "main.c"]),
    fixture!("nested_macros", ["defs.h", "main.c"]),
    fixture!(
        "nested_relative_headers",
        ["main.c", "sub/inner.h", "sub/outer.h"]
    ),
    fixture!("pragma_once_reinclude", ["defs.h", "main.c"]),
    fixture!("redefinition", ["defs.h", "main.c"]),
    fixture!("reinclude_without_pragma", ["defs.h", "main.c"]),
    fixture!("relative_subdirectory", ["main.c", "sub/defs.h"]),
    fixture!("return_macro", ["defs.h", "main.c"]),
    fixture!("second_argument_only", ["defs.h", "main.c"]),
    fixture!("self_recursive_object", ["defs.h", "main.c"]),
    fixture!("statement_macro", ["defs.h", "main.c"]),
    fixture!("two_translation_units", ["a.c", "b.c", "defs.h"]),
    fixture!("undefinition", ["defs.h", "main.c"]),
];

#[test]
fn supplied_header_macros_match_complete_isolated_live_graphs() {
    for case in CASES {
        let mut project = cpg_incremental::Project::new(
            || Box::new(cpg_lang_c::CFrontend::new()),
            cpg_analysis::standard_pipeline(),
        );
        project.build(case.sources);
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&project.cpg),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn parity_cli_preserves_relative_header_paths() {
    let unrelated_directory = tempfile::tempdir().unwrap();
    for name in ["relative_subdirectory", "nested_relative_headers"] {
        let case = CASES.iter().find(|case| case.name == name).unwrap();
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/header-macros/cases")
            .join(name)
            .join("input");
        for absolute_paths in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_joern-parity"));
            if absolute_paths {
                command.current_dir(unrelated_directory.path());
                for (path, _) in case.sources {
                    command.arg(input.join(path));
                }
            } else {
                command.current_dir(&input);
                command.args(case.sources.iter().map(|(path, _)| path));
            }
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{name}, absolute={absolute_paths}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8(result.stdout).unwrap(),
                case.expected,
                "{name}, absolute={absolute_paths}"
            );
        }
    }
}
