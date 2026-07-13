//! Robustness: richer, realistic snippets per language, exercising constructs
//! the minimal conformance cases don't — classes and member/method calls, loops,
//! receivers, macros, arrow functions, multiple parameters. The point is that
//! the SAME generic engine survives all of them and extracts a sensible graph
//! (methods discovered, member-call names resolved to the trailing identifier).

use cpg_core::{Cpg, Query};
use cpg_frontend::Frontend;
use cpg_lang_ts::TsFrontend;

fn build(mut fe: TsFrontend, path: &str, code: &str) -> Cpg {
    let mut cpg = Cpg::new();
    fe.build_file(&mut cpg, path, code);
    cpg
}

/// Assert a method's parameters are named exactly `expected` (in order). This
/// guards the subtle bug where a typed parameter's *type* (itself a
/// `*_identifier` in some grammars) gets picked up as the name.
fn assert_param_names(cpg: &Cpg, method: &str, expected: &[&str], lang: &str) {
    let m = cpg.method_named(method);
    assert_eq!(m.len(), 1, "{lang}: method `{method}` not found");
    let got: Vec<&str> = cpg
        .parameters_of(m[0])
        .iter()
        .filter_map(|&p| cpg.name_of(p))
        .collect();
    assert_eq!(got, expected, "{lang}: `{method}` parameter names");
}

/// Helper: assert a method and a call (often a member call) are present.
fn assert_has(cpg: &Cpg, methods: &[&str], calls: &[&str], lang: &str) {
    for m in methods {
        assert!(
            !cpg.method_named(m).is_empty(),
            "{lang}: expected method `{m}`, methods = {:?}",
            cpg.methods().iter().filter_map(|&x| cpg.name_of(x)).collect::<Vec<_>>()
        );
    }
    for c in calls {
        assert!(
            !cpg.calls_named(c).is_empty(),
            "{lang}: expected call `{c}`, calls = {:?}",
            cpg.calls().iter().filter_map(|&x| cpg.name_of(x)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn java_rich() {
    let cpg = build(
        TsFrontend::java(),
        "C.java",
        r#"
        class Service {
            private Repo repo;
            int total(java.util.List<Integer> xs) {
                int sum = 0;
                for (int x : xs) { sum = add(sum, x); }
                this.repo.save(sum);
                return sum;
            }
            int add(int a, int b) { return a + b; }
        }
        "#,
    );
    // `save` is a member call; `add`/`total` are methods.
    assert_has(&cpg, &["total", "add"], &["add", "save"], "Java");
    // `total(List<Integer> xs)` — the param is `xs`, not the type `List`.
    assert_param_names(&cpg, "total", &["xs"], "Java");
    assert_param_names(&cpg, "add", &["a", "b"], "Java");
}

#[test]
fn go_rich() {
    let cpg = build(
        TsFrontend::go(),
        "m.go",
        r#"
        package m
        func (s *Server) Handle(name string, n int) (int, error) {
            logger.Printf("handling %s", name)
            v := compute(n)
            return v, nil
        }
        func compute(n int) int { return n * 2 }
        "#,
    );
    // method with a receiver + a member call (Printf) + a free function.
    assert_has(&cpg, &["Handle", "compute"], &["compute", "Printf"], "Go");
    // Typed params must keep their names, not their types (string/int).
    assert_param_names(&cpg, "Handle", &["name", "n"], "Go");
}

#[test]
fn javascript_rich() {
    let cpg = build(
        TsFrontend::javascript(),
        "m.js",
        r#"
        class Api {
            constructor(client){ this.client = client; }
            fetchUser(id){ return this.client.get(wrap(id)); }
        }
        const handler = (req) => sink(req.body);
        function wrap(x){ return x; }
        "#,
    );
    // class method, arrow bound to `handler`, member calls get + member access.
    assert_has(&cpg, &["fetchUser", "handler", "wrap"], &["wrap", "get", "sink"], "JavaScript");
}

#[test]
fn ruby_rich() {
    let cpg = build(
        TsFrontend::ruby(),
        "m.rb",
        r#"
        class Worker
          def perform(id, opts)
            data = fetch(id)
            logger.info("done")
            transform(data)
          end
          def transform(d)
            d.upcase
          end
        end
        "#,
    );
    // command call (logger.info), member call, methods with params.
    assert_has(&cpg, &["perform", "transform"], &["fetch", "info", "transform"], "Ruby");
    assert_eq!(cpg.parameters_of(cpg.method_named("perform")[0]).len(), 2);
}

#[test]
fn rust_rich() {
    let cpg = build(
        TsFrontend::rust(),
        "m.rs",
        r#"
        struct S { n: i32 }
        impl S {
            fn run(&self, input: String) -> String {
                let cleaned = sanitize(input);
                println!("running {}", cleaned);
                self.helper.process(cleaned)
            }
        }
        fn sanitize(s: String) -> String { s }
        "#,
    );
    // impl method, macro call (println! args via token_tree), member call,
    // tail-expression method call, free fn.
    assert_has(&cpg, &["run", "sanitize"], &["sanitize", "println", "process"], "Rust");
    // `sanitize(s: String)` — the param is `s`, not the type `String`.
    assert_param_names(&cpg, "sanitize", &["s"], "Rust");
    assert_param_names(&cpg, "run", &["input"], "Rust");
    // The macro's argument `cleaned` must be captured from the token_tree.
    let println = cpg.calls_named("println");
    assert!(
        !cpg.arguments_of(println[0]).is_empty(),
        "Rust: println! args (token_tree) not captured"
    );
}

#[test]
fn python_rich() {
    let cpg = build(
        TsFrontend::python(),
        "m.py",
        r#"
class Handler:
    def process(self, request, config):
        data = parse(request)
        self.logger.info("ok")
        return transform(data)

    def transform(self, d):
        return d.upper()
"#,
    );
    assert_has(&cpg, &["process", "transform"], &["parse", "info", "transform"], "Python");
    // `self` is dropped, so process has 2 real params (request, config).
    assert_eq!(cpg.parameters_of(cpg.method_named("process")[0]).len(), 2);
}
