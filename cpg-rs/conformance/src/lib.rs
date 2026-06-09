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
        cpg_analysis::standard_pipeline().run_all(&mut cpg, &[file], &cpg_analysis::PassContext::empty());
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
                    return Err(format!("expected 1 method `two_params`, found {}", ms.len()));
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
                    Err(format!("expected 1 `guarded` call inside branch, found {}", cs.len()))
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
                    Err(format!("expected methods alpha(×1) and beta(×1), found {a} and {b}"))
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
            .with_source(
                "call_with_two_args",
                "void f() { callee(1, 2); }",
            )
            .with_source(
                "intraprocedural_call_resolves",
                "int target(int x){ return x; } void f(){ target(3); }",
            )
            .with_source("nested_call_is_argument", "void f(){ outer(inner(1)); }")
            .with_source(
                "call_inside_branch",
                "void f(int c){ if (c) { guarded(c); } }",
            )
            .with_source(
                "two_methods",
                "void alpha(){} void beta(){}",
            )
    }

    #[test]
    fn c_conforms_to_standard_cases() {
        let cases = standard_cases();
        let mut fx = c_fixture();
        let results = run_suite(&mut fx, &cases);
        let failures: Vec<&CaseResult> =
            results.iter().filter(|r| r.outcome.is_err()).collect();
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
            .with_source("two_methods", "def alpha():\n    pass\n\ndef beta():\n    pass\n")
    }

    #[test]
    fn python_conforms_to_standard_cases() {
        let cases = standard_cases();
        let mut fx = python_fixture();
        let results = run_suite(&mut fx, &cases);
        let failures: Vec<&CaseResult> =
            results.iter().filter(|r| r.outcome.is_err()).collect();
        assert!(
            failures.is_empty(),
            "Python frontend failed conformance: {:#?}",
            failures
        );
    }
}
