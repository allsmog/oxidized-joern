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
        cpg_analysis::standard_pipeline().run_all(&mut cpg, &[file]);
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
        assert_eq!(results.len(), 3);
    }

    // When a second frontend lands, the test is literally:
    //
    //   let mut fx = LangFixture::new("Java", Box::new(JavaFrontend::new()))
    //       .with_source("method_with_two_params", "int two_params(int a,int b){return a;}")
    //       ...;
    //   assert all run_suite(&mut fx, &standard_cases()) pass;
    //
    // Same assertions, new language. That is the consolidation contract.
}
