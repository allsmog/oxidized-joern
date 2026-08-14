//! Cross-language conformance suite — roadmap item #2.
//!
//! The idea from the architecture review: turn "twelve independently-correct
//! frontends" into "one specification, twelve conforming implementations". A
//! [`ConformanceCase`] asserts a property of the *language-independent schema*
//! (a method has two parameters; a call resolves to its target). Each language
//! supplies its own source text for the case via a [`LangFixture`]; the
//! assertion is identical across languages.
//!
//! Adding a new frontend then means: register a `LangFixture` with that
//! language's source for each case. If it passes the suite, its graph shape is
//! compatible with every shared pass and query — which is exactly what
//! de-risks consolidating frontend logic into the shared layer.

use cpg_core::{Cpg, Query};
use cpg_frontend::Frontend;
use std::collections::HashMap;

/// A schema-level assertion that must hold regardless of source language.
pub struct ConformanceCase {
    pub name: &'static str,
    pub assert: fn(&Cpg) -> Result<(), String>,
}

/// One language's source for each case, plus its frontend.
pub struct LangFixture {
    pub language: &'static str,
    pub frontend: Box<dyn Frontend>,
    pub sources: HashMap<&'static str, &'static str>,
}

impl LangFixture {
    pub fn new(language: &'static str, frontend: Box<dyn Frontend>) -> Self {
        LangFixture {
            language,
            frontend,
            sources: HashMap::new(),
        }
    }

    pub fn with_source(mut self, case: &'static str, src: &'static str) -> Self {
        self.sources.insert(case, src);
        self
    }
}

#[derive(Debug)]
pub struct CaseResult {
    pub language: String,
    pub case: String,
    pub outcome: Result<(), String>,
}

/// Run every case against one language fixture, building the CPG and running
/// the standard pipeline before each assertion.
pub fn run_suite(fixture: &mut LangFixture, cases: &[ConformanceCase]) -> Vec<CaseResult> {
    let mut results = Vec::new();
    for case in cases {
        let Some(src) = fixture.sources.get(case.name) else {
            results.push(CaseResult {
                language: fixture.language.to_string(),
                case: case.name.to_string(),
                outcome: Err("no source provided for case".into()),
            });
            continue;
        };
        let mut cpg = Cpg::new();
        let path = format!("case_{}.src", case.name);
        let file = cpg.file_id(&path);
        fixture.frontend.build_file(&mut cpg, &path, src);
        cpg_analysis::standard_pipeline().run_all(
            &mut cpg,
            &[file],
            &cpg_analysis::PassContext::empty(),
        );
        results.push(CaseResult {
            language: fixture.language.to_string(),
            case: case.name.to_string(),
            outcome: (case.assert)(&cpg),
        });
    }
    results
}

/// The standard, language-independent case set.
pub fn standard_cases() -> Vec<ConformanceCase> {
    vec![
        ConformanceCase {
            name: "method_with_two_params",
            assert: |cpg| {
                let ms = cpg.method_named("two_params");
                if ms.len() != 1 {
                    return Err(format!(
                        "expected 1 method `two_params`, found {}",
                        ms.len()
                    ));
                }
                let n = cpg.parameters_of(ms[0]).len();
                if n != 2 {
                    return Err(format!("expected 2 params, found {n}"));
                }
                Ok(())
            },
        },
        ConformanceCase {
            name: "call_with_two_args",
            assert: |cpg| {
                let cs = cpg.calls_named("callee");
                if cs.len() != 1 {
                    return Err(format!("expected 1 call `callee`, found {}", cs.len()));
                }
                let n = cpg.arguments_of(cs[0]).len();
                if n != 2 {
                    return Err(format!("expected 2 args, found {n}"));
                }
                Ok(())
            },
        },
        ConformanceCase {
            name: "intraprocedural_call_resolves",
            assert: |cpg| {
                let cs = cpg.calls_named("target");
                if cs.len() != 1 {
                    return Err(format!("expected 1 call `target`, found {}", cs.len()));
                }
                match cpg.call_target(cs[0]) {
                    Some(t) if cpg.name_of(t) == Some("target") => Ok(()),
                    _ => Err("call `target` did not resolve to its definition".into()),
                }
            },
        },
        ConformanceCase {
            // `outer(inner(x))`: the inner call must be an argument of the outer
            // call, so both exist and `inner` sits in `outer`'s argument subtree.
            name: "nested_call_is_argument",
            assert: |cpg| {
                let outer = cpg.calls_named("outer");
                let inner = cpg.calls_named("inner");
                if outer.len() != 1 || inner.len() != 1 {
                    return Err(format!(
                        "expected one `outer` and one `inner` call, found {} and {}",
                        outer.len(),
                        inner.len()
                    ));
                }
                let args = cpg.arguments_of(outer[0]);
                if args.contains(&inner[0]) {
                    Ok(())
                } else {
                    Err("`inner` is not an argument of `outer`".into())
                }
            },
        },
        ConformanceCase {
            // A call nested inside a control structure must still be discovered
            // (branches don't hide calls from the graph).
            name: "call_inside_branch",
            assert: |cpg| {
                let cs = cpg.calls_named("guarded");
                if cs.len() == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected 1 `guarded` call inside branch, found {}",
                        cs.len()
                    ))
                }
            },
        },
        ConformanceCase {
            // Two top-level methods, both present and distinct.
            name: "two_methods",
            assert: |cpg| {
                let a = cpg.method_named("alpha").len();
                let b = cpg.method_named("beta").len();
                if a == 1 && b == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "expected methods alpha(×1) and beta(×1), found {a} and {b}"
                    ))
                }
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_lang_c::CFrontend;

    fn c_fixture() -> LangFixture {
        LangFixture::new("C", Box::new(CFrontend::new()))
            .with_source(
                "method_with_two_params",
                "int two_params(int a, int b) { return a; }",
            )
            .with_source("call_with_two_args", "void f() { callee(1, 2); }")
            .with_source(
                "intraprocedural_call_resolves",
                "int target(int x){ return x; } void f(){ target(3); }",
            )
            .with_source("nested_call_is_argument", "void f(){ outer(inner(1)); }")
            .with_source(
                "call_inside_branch",
                "void f(int c){ if (c) { guarded(c); } }",
            )
            .with_source("two_methods", "void alpha(){} void beta(){}")
    }

    #[test]
    fn c_conforms_to_standard_cases() {
        let cases = standard_cases();
        let mut fx = c_fixture();
        let results = run_suite(&mut fx, &cases);
        let failures: Vec<&CaseResult> = results.iter().filter(|r| r.outcome.is_err()).collect();
        assert!(
            failures.is_empty(),
            "C frontend failed conformance: {:#?}",
            failures
        );
        assert_eq!(results.len(), standard_cases().len());
    }

    // The second frontend: same assertions, different language. This is the
    // consolidation contract in action — Python passes the identical suite
    // with zero changes to the cases or the shared passes.
    fn python_fixture() -> LangFixture {
        use cpg_lang_python::PythonFrontend;
        LangFixture::new("Python", Box::new(PythonFrontend::new()))
            .with_source(
                "method_with_two_params",
                "def two_params(a, b):\n    return a\n",
            )
            .with_source("call_with_two_args", "def f():\n    callee(1, 2)\n")
            .with_source(
                "intraprocedural_call_resolves",
                "def target(x):\n    return x\n\ndef f():\n    target(3)\n",
            )
            .with_source("nested_call_is_argument", "def f():\n    outer(inner(1))\n")
            .with_source(
                "call_inside_branch",
                "def f(c):\n    if c:\n        guarded(c)\n",
            )
            .with_source(
                "two_methods",
                "def alpha():\n    pass\n\ndef beta():\n    pass\n",
            )
    }

    #[test]
    fn python_conforms_to_standard_cases() {
        let cases = standard_cases();
        let mut fx = python_fixture();
        let results = run_suite(&mut fx, &cases);
        let failures: Vec<&CaseResult> = results.iter().filter(|r| r.outcome.is_err()).collect();
        assert!(
            failures.is_empty(),
            "Python frontend failed conformance: {:#?}",
            failures
        );
    }

    // --- the six-language stress test ---
    //
    // Java, Go, JavaScript, Ruby and Rust are all served by the SAME generic
    // tree-sitter engine (cpg-lang-ts), differing only by a declarative spec.
    // Each supplies its own source for the identical case set. If they all pass,
    // the language contract holds under maximum stress: one engine, one schema,
    // one set of shared passes, six very different grammars.

    use cpg_lang_ts::TsFrontend;

    fn java_fixture() -> LangFixture {
        LangFixture::new("Java", Box::new(TsFrontend::java()))
            .with_source(
                "method_with_two_params",
                "class C { int two_params(int a, int b){ return a; } }",
            )
            .with_source(
                "call_with_two_args",
                "class C { void f(){ callee(1, 2); } }",
            )
            .with_source(
                "intraprocedural_call_resolves",
                "class C { int target(int x){ return x; } void f(){ target(3); } }",
            )
            .with_source(
                "nested_call_is_argument",
                "class C { void f(){ outer(inner(1)); } }",
            )
            .with_source(
                "call_inside_branch",
                "class C { void f(int c){ if (c > 0) { guarded(c); } } }",
            )
            .with_source("two_methods", "class C { void alpha(){} void beta(){} }")
    }

    fn go_fixture() -> LangFixture {
        LangFixture::new("Go", Box::new(TsFrontend::go()))
            .with_source(
                "method_with_two_params",
                "package m\nfunc two_params(a int, b int) int { return a }",
            )
            .with_source("call_with_two_args", "package m\nfunc f(){ callee(1, 2) }")
            .with_source(
                "intraprocedural_call_resolves",
                "package m\nfunc target(x int) int { return x }\nfunc f(){ target(3) }",
            )
            .with_source(
                "nested_call_is_argument",
                "package m\nfunc f(){ outer(inner(1)) }",
            )
            .with_source(
                "call_inside_branch",
                "package m\nfunc f(c int){ if c > 0 { guarded(c) } }",
            )
            .with_source("two_methods", "package m\nfunc alpha(){}\nfunc beta(){}")
    }

    fn javascript_fixture() -> LangFixture {
        LangFixture::new("JavaScript", Box::new(TsFrontend::javascript()))
            .with_source(
                "method_with_two_params",
                "function two_params(a, b){ return a; }",
            )
            .with_source("call_with_two_args", "function f(){ callee(1, 2); }")
            .with_source(
                "intraprocedural_call_resolves",
                "function target(x){ return x; } function f(){ target(3); }",
            )
            .with_source(
                "nested_call_is_argument",
                "function f(){ outer(inner(1)); }",
            )
            .with_source(
                "call_inside_branch",
                "function f(c){ if (c) { guarded(c); } }",
            )
            .with_source("two_methods", "function alpha(){} function beta(){}")
    }

    fn typescript_fixture() -> LangFixture {
        LangFixture::new("TypeScript", Box::new(TsFrontend::typescript()))
            .with_source(
                "method_with_two_params",
                "function two_params(a: number, b: number): number { return a; }",
            )
            .with_source(
                "call_with_two_args",
                "function f(): void { callee(1, 2); }",
            )
            .with_source(
                "intraprocedural_call_resolves",
                "function target(x: number): number { return x; } function f(): void { target(3); }",
            )
            .with_source(
                "nested_call_is_argument",
                "function f(): void { outer(inner(1)); }",
            )
            .with_source(
                "call_inside_branch",
                "function f(c: boolean): void { if (c) { guarded(c); } }",
            )
            .with_source(
                "two_methods",
                "function alpha(): void {} function beta(): void {}",
            )
    }

    fn cpp_fixture() -> LangFixture {
        LangFixture::new("C++", Box::new(TsFrontend::cpp()))
            .with_source(
                "method_with_two_params",
                "int two_params(int a, int b) { return a; }",
            )
            .with_source("call_with_two_args", "void f() { callee(1, 2); }")
            .with_source(
                "intraprocedural_call_resolves",
                "int target(int x){ return x; } void f(){ target(3); }",
            )
            .with_source("nested_call_is_argument", "void f(){ outer(inner(1)); }")
            .with_source(
                "call_inside_branch",
                "void f(bool c){ if (c) { guarded(c); } }",
            )
            .with_source("two_methods", "void alpha(){} void beta(){}")
    }

    fn scala_fixture() -> LangFixture {
        LangFixture::new("Scala", Box::new(TsFrontend::scala()))
            .with_source(
                "method_with_two_params",
                "object C { def two_params(a: Int, b: Int): Int = a }",
            )
            .with_source(
                "call_with_two_args",
                "object C { def f(): Unit = { callee(1, 2) } }",
            )
            .with_source(
                "intraprocedural_call_resolves",
                "object C { def target(x: Int): Int = x; def f(): Unit = { target(3) } }",
            )
            .with_source(
                "nested_call_is_argument",
                "object C { def f(): Unit = { outer(inner(1)) } }",
            )
            .with_source(
                "call_inside_branch",
                "object C { def f(c: Boolean): Unit = { if (c) guarded(c) } }",
            )
            .with_source(
                "two_methods",
                "object C { def alpha(): Unit = {}; def beta(): Unit = {} }",
            )
    }

    fn ruby_fixture() -> LangFixture {
        LangFixture::new("Ruby", Box::new(TsFrontend::ruby()))
            .with_source("method_with_two_params", "def two_params(a, b)\n  a\nend")
            .with_source("call_with_two_args", "def f\n  callee(1, 2)\nend")
            .with_source(
                "intraprocedural_call_resolves",
                "def target(x)\n  x\nend\ndef f\n  target(3)\nend",
            )
            .with_source("nested_call_is_argument", "def f\n  outer(inner(1))\nend")
            .with_source(
                "call_inside_branch",
                "def f(c)\n  if c\n    guarded(c)\n  end\nend",
            )
            .with_source("two_methods", "def alpha\nend\ndef beta\nend")
    }

    fn rust_fixture() -> LangFixture {
        LangFixture::new("Rust", Box::new(TsFrontend::rust()))
            .with_source(
                "method_with_two_params",
                "fn two_params(a: i32, b: i32) -> i32 { a }",
            )
            .with_source("call_with_two_args", "fn f(){ callee(1, 2); }")
            .with_source(
                "intraprocedural_call_resolves",
                "fn target(x: i32) -> i32 { x } fn f(){ target(3); }",
            )
            .with_source("nested_call_is_argument", "fn f(){ outer(inner(1)); }")
            .with_source(
                "call_inside_branch",
                "fn f(c: bool){ if c { guarded(c); } }",
            )
            .with_source("two_methods", "fn alpha(){} fn beta(){}")
    }

    /// Every language must pass every case. One assert covers all six.
    #[test]
    fn all_languages_conform() {
        let cases = standard_cases();
        let mut fixtures = vec![
            c_fixture(),
            python_fixture(),
            java_fixture(),
            go_fixture(),
            javascript_fixture(),
            typescript_fixture(),
            ruby_fixture(),
            rust_fixture(),
            cpp_fixture(),
            scala_fixture(),
        ];
        let mut all_failures: Vec<CaseResult> = Vec::new();
        for fx in &mut fixtures {
            for r in run_suite(fx, &cases) {
                if r.outcome.is_err() {
                    all_failures.push(r);
                }
            }
        }
        assert!(
            all_failures.is_empty(),
            "language contract failures:\n{:#?}",
            all_failures
        );
    }
}
