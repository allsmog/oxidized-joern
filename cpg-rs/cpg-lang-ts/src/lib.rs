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
    pub fn scala() -> Self {
        Self::new(spec::scala())
    }
    pub fn cpp() -> Self {
        Self::new(spec::cpp())
    }
    pub fn typescript() -> Self {
        Self::new(spec::typescript())
    }
}

impl Frontend for TsFrontend {
    fn language(&self) -> &dyn Language {
        &self.lang
    }

    fn build_file(&mut self, cpg: &mut Cpg, path: &str, source: &str) -> BuildResult {
        // Language shims (e.g. C++/CLI -> standard C++) are line/column
        // preserving, so building from the shimmed text keeps every CPG
        // location valid for the original file.
        let shimmed = self.spec.preprocess.and_then(|f| f(source));
        let source = shimmed.as_deref().unwrap_or(source);
        // Dialect grammars are keyed by extension (TypeScript vs TSX): switch
        // the parser for the file, restore the base grammar afterwards.
        let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        let dialect = self.spec.dialects.iter().find(|(e, _)| *e == ext);
        if let Some((_, lang)) = dialect {
            self.parser.set_language(&lang()).expect("load dialect grammar");
        }
        let tree = self.parser.parse(source, None).expect("parse");
        if dialect.is_some() {
            self.parser.set_language(&self.spec.language).expect("restore grammar");
        }
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
    fn cpp_basics() {
        let cpg = build(
            TsFrontend::cpp(),
            "m.cpp",
            "int add(int a, char* b) { return a; }\nvoid f() { puts(add(1, x)); }\n",
        );
        assert_eq!(cpg.method_named("add").len(), 1, "free function by declarator name");
        assert_eq!(cpg.parameters_of(cpg.method_named("add")[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
        assert_eq!(cpg.calls_named("puts").len(), 1);
    }

    #[test]
    fn cpp_qualified_and_class_methods() {
        let cpg = build(
            TsFrontend::cpp(),
            "q.cpp",
            "namespace ns {\nclass Foo {\n public:\n  int inline_m(int x) { return x; }\n};\n}\nvoid ns::Foo::out_of_line(char* s) { use(s); }\nvoid caller(Foo* f) { f->inline_m(1); ns::Foo::out_of_line(p); std::getenv(\"X\"); }\n",
        );
        // Out-of-line definition: name from the qualified declarator, scope
        // becomes the receiver type.
        let ool = cpg.method_named("out_of_line");
        assert_eq!(ool.len(), 1);
        assert_eq!(cpg.full_name_of(ool[0]), Some("Foo::out_of_line"));
        // Inline class method: enclosing container is the receiver type.
        let im = cpg.method_named("inline_m");
        assert_eq!(im.len(), 1);
        assert_eq!(cpg.full_name_of(im[0]), Some("Foo::inline_m"));
        // Calls: member call, qualified call, and external qualified call all
        // carry their simple name.
        assert_eq!(cpg.calls_named("inline_m").len(), 1);
        assert_eq!(cpg.calls_named("out_of_line").len(), 1);
        assert_eq!(cpg.calls_named("getenv").len(), 1);
    }

    #[test]
    fn tsx_dialect_parses_jsx_and_lowers_markup() {
        // The plain TypeScript grammar cannot parse JSX at all — without the
        // dialect switch this whole component body drops from the graph.
        let cpg = build(
            TsFrontend::typescript(),
            "C.tsx",
            "function Comp(props) { const h = props.html; return <div className=\"x\" dangerouslySetInnerHTML={{__html: h}}>{h}</div>; }",
        );
        assert_eq!(cpg.method_named("Comp").len(), 1);
        assert_eq!(cpg.calls_named("dangerouslySetInnerHTML").len(), 1, "jsx attribute is a named call");
        assert_eq!(cpg.calls_named("div").len(), 1, "jsx element is a call named after the tag");
    }

    #[test]
    fn plain_ts_keeps_the_base_grammar() {
        // `<T>` is a generic parameter list here; the TSX grammar would
        // mis-parse it as JSX, so .ts files must keep the base grammar.
        let cpg = build(
            TsFrontend::typescript(),
            "m.ts",
            "function add<T>(a: T, b: T): T { return a; }\nfunction f(){ puts(add(1,2)); }",
        );
        assert_eq!(cpg.method_named("add").len(), 1);
        assert_eq!(cpg.calls_named("add").len(), 1);
    }

    #[test]
    fn template_string_substitution_is_concat_shaped() {
        // `query(`select ${u}`)` — the template must not be swallowed as a
        // constant literal; its substitution carries the argument's value.
        let cpg = build(
            TsFrontend::javascript(),
            "m.js",
            "function f(u){ query(`select ${u}`); }",
        );
        assert_eq!(cpg.calls_named("query").len(), 1);
        assert_eq!(cpg.calls_named("+").len(), 1, "substitution lowers to a concat-shaped call");
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
