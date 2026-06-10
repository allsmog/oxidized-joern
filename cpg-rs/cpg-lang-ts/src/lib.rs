//! Generic tree-sitter frontend.
//!
//! A single [`TsFrontend`] maps any grammar described by a [`spec::TsLangSpec`]
//! onto the shared CPG builders. The six constructors below — Java, Go,
//! JavaScript, Ruby, Rust, Python — differ only in data. This is the
//! consolidation argument taken to its logical end: adding a language is
//! writing a struct literal, not a frontend.

mod engine;
pub mod spec;

use cpg_core::{Cpg, CpgBuilder};
use cpg_frontend::{BuildResult, Frontend, Language, LanguageTraits};
use spec::TsLangSpec;
use tree_sitter::Parser;

/// A `Language` whose metadata is borrowed from the spec.
pub struct TsLanguage {
    name: &'static str,
    delim: &'static str,
    traits: LanguageTraits,
    extensions: &'static [&'static str],
}

impl Language for TsLanguage {
    fn name(&self) -> &'static str {
        self.name
    }
    fn namespace_delimiter(&self) -> &'static str {
        self.delim
    }
    fn traits(&self) -> LanguageTraits {
        self.traits
    }
    fn file_extensions(&self) -> &'static [&'static str] {
        self.extensions
    }
}

pub struct TsFrontend {
    spec: TsLangSpec,
    lang: TsLanguage,
    parser: Parser,
}

impl TsFrontend {
    pub fn new(spec: TsLangSpec) -> Self {
        let lang = TsLanguage {
            name: spec.name,
            delim: spec.namespace_delim,
            traits: spec.traits,
            extensions: spec.extensions,
        };
        let mut parser = Parser::new();
        parser.set_language(&spec.language).expect("load grammar");
        TsFrontend { spec, lang, parser }
    }

    pub fn java() -> Self {
        Self::new(spec::java())
    }
    pub fn go() -> Self {
        Self::new(spec::go())
    }
    pub fn javascript() -> Self {
        Self::new(spec::javascript())
    }
    pub fn ruby() -> Self {
        Self::new(spec::ruby())
    }
    pub fn rust() -> Self {
        Self::new(spec::rust())
    }
    pub fn python() -> Self {
        Self::new(spec::python())
    }
}

impl Frontend for TsFrontend {
    fn language(&self) -> &dyn Language {
        &self.lang
    }

    fn build_file(&mut self, cpg: &mut Cpg, path: &str, source: &str) -> BuildResult {
        let tree = self.parser.parse(source, None).expect("parse");
        let file = cpg.file_id(path);
        let mut b = CpgBuilder::new(cpg, file);
        let methods = engine::build(&self.spec, &mut b, tree.root_node(), source.as_bytes(), path);
        BuildResult { file, methods_built: methods }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::Query;

    fn build(mut fe: TsFrontend, path: &str, code: &str) -> Cpg {
        let mut cpg = Cpg::new();
        fe.build_file(&mut cpg, path, code);
        cpg
    }

    #[test]
    fn java_basics() {
        let cpg = build(
            TsFrontend::java(),
            "C.java",
            "class C { int add(int a, int b){ return a; } void f(){ puts(add(1,2)); } }",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
        assert_eq!(cpg.calls_named("puts").len(), 1);
    }

    #[test]
    fn go_basics() {
        let cpg = build(
            TsFrontend::go(),
            "m.go",
            "package m\nfunc add(a int, b int) int { return a }\nfunc f(){ puts(add(1,2)) }",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
    }

    #[test]
    fn rust_basics() {
        let cpg = build(
            TsFrontend::rust(),
            "m.rs",
            "fn add(a: i32, b: i32) -> i32 { a } fn f(){ puts(add(1,2)); }",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
    }

    #[test]
    fn ruby_basics() {
        let cpg = build(
            TsFrontend::ruby(),
            "m.rb",
            "def add(a, b)\n  a\nend\ndef f\n  puts(add(1,2))\nend",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
    }

    #[test]
    fn javascript_basics() {
        let cpg = build(
            TsFrontend::javascript(),
            "m.js",
            "function add(a, b){ return a; } function f(){ puts(add(1,2)); }",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
    }
}
