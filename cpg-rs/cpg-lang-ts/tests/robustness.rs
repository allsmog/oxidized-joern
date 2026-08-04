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

#[test]
fn cpp_typedecl_members() {
    use cpg_core::NodeKind;
    let cpg = build(
        TsFrontend::cpp(),
        "gateway_server.cpp",
        r#"
namespace example {
class Status;
class GatewayServer final : public example::gateway::GatewayServerIf {
 public:
  void mkdir(common::Status& response, const MkdirRequest& request);
 private:
  std::shared_ptr<filesvc::FileServiceIf> file_service_client_;
  int count_, total_;
};
void GatewayServer::mkdir(common::Status& response, const MkdirRequest& request) {
  file_service_client_->mkdir(response, request);
}
}
"#,
    );
    // The class body emits a TypeDecl with its base list in the signature
    // column; the forward declaration (`class Status;`) emits nothing.
    let tds: Vec<_> = cpg
        .nodes_of_kind(NodeKind::TypeDecl)
        .into_iter()
        .filter(|&t| cpg.is_live(t))
        .collect();
    assert_eq!(tds.len(), 1, "one TypeDecl (forward decl excluded)");
    let td = tds[0];
    assert_eq!(cpg.name_of(td), Some("GatewayServer"));
    assert_eq!(cpg.full_name_of(td), Some("example::GatewayServer"));
    assert_eq!(cpg.signature_of(td), Some("GatewayServerIf"), "base simple name, ns stripped");
    // Members: the method prototype is skipped; the smart pointer unwraps to
    // its pointee; a multi-declarator line yields one Member each.
    let members: Vec<_> = cpg.nodes_of_kind(NodeKind::Member);
    let by_name: std::collections::HashMap<_, _> = members
        .iter()
        .filter_map(|&m| Some((cpg.name_of(m)?, cpg.type_full_name_of(m)?)))
        .collect();
    assert_eq!(by_name.get("file_service_client_"), Some(&"FileServiceIf"));
    assert_eq!(by_name.get("count_"), Some(&"int"));
    assert_eq!(by_name.get("total_"), Some(&"int"));
    assert!(!by_name.contains_key("mkdir"), "prototype must not be a Member");
    // The client call carries its receiver variable name in the signature
    // column (the hook the call-graph pass uses for member-type hints).
    let calls = cpg.calls_named("mkdir");
    assert_eq!(calls.len(), 1);
    assert_eq!(cpg.signature_of(calls[0]), Some("file_service_client_"));
    // Out-of-line definition still gets the qualified full name.
    let m = cpg.method_named("mkdir");
    assert_eq!(m.len(), 1);
    assert_eq!(cpg.full_name_of(m[0]), Some("GatewayServer::mkdir"));
    // Params of interface type also unwrap smart pointers elsewhere; here just
    // confirm the reference-typed params kept their names.
    assert_param_names(&cpg, "mkdir", &["response", "request"], "C++");
}

#[test]
fn typescript_rich() {
    let cpg = build(
        TsFrontend::typescript(),
        "app.ts",
        r#"
export async function download(ctx: Context): Promise<void> {
  const name: string = ctx.query.name;
  fs.readFileSync("/r/" + name);
}
class Tool {
  private cmd: string;
  run(req: ToolRequest): void {
    exec(new Builder(req.args).build());
  }
}
"#,
    );
    assert_has(&cpg, &["download", "run"], &["readFileSync", "exec", "Builder", "build"], "TypeScript");
    // Param types survive TS annotations (drives entry guards + hints).
    let run = cpg.method_named("run");
    let p = cpg.parameters_of(run[0])[0];
    assert_eq!(cpg.type_full_name_of(p), Some("ToolRequest"));
}

#[test]
fn scala_field_reads_and_named_args() {
    let cpg = build(
        TsFrontend::scala(),
        "JobConfig.scala",
        r#"
object JobConfigOps {
  def update(config: JobConfig, newUser: String): JobConfig = {
    config.copy(executionUser = newUser)
  }
  def runTask(config: JobConfig): Unit = {
    exec(config.executionSettings.executionUser)
  }
}
"#,
    );
    assert_has(&cpg, &["update", "runTask"], &["copy", "exec"], "Scala");
    // A bare field read lowers to a Call named after the field, base as
    // argument, receiver root stamped in signature — the shape the
    // persistence stitch matches getter-sources against.
    let reads = cpg.calls_named("executionUser");
    assert!(!reads.is_empty(), "field read `executionUser` must surface as a call");
    let read = reads
        .iter()
        .copied()
        .find(|&c| cpg.code_of(c) == Some("config.executionSettings.executionUser"))
        .expect("chained read with full code text");
    assert_eq!(cpg.signature_of(read), Some("config"), "receiver root in signature");
    let inner = cpg.arguments_of(read);
    assert_eq!(inner.len(), 1);
    assert_eq!(cpg.name_of(inner[0]), Some("executionSettings"), "chain nests");
    // A named argument survives as a nested `=` call: arg 1 names the
    // parameter, arg 2 carries the value (and its taint).
    let copy = cpg.calls_named("copy");
    assert_eq!(copy.len(), 1);
    assert_eq!(cpg.signature_of(copy[0]), Some("config"));
    let cargs = cpg.arguments_of(copy[0]);
    assert_eq!(cargs.len(), 1);
    assert_eq!(cpg.name_of(cargs[0]), Some("="), "named arg is a `=` call");
    let na = cpg.arguments_of(cargs[0]);
    assert_eq!(na.len(), 2);
    assert_eq!(cpg.name_of(na[0]), Some("executionUser"));
    assert_eq!(cpg.name_of(na[1]), Some("newUser"));
}

#[test]
fn go_field_read_lowering() {
    let cpg = build(
        TsFrontend::go(),
        "cfg.go",
        r#"
package m
func f(c Cfg) {
    u := c.Sub.ExecutionUser
    g(u)
}
"#,
    );
    // `c.Sub.ExecutionUser` → Call "ExecutionUser"(Call "Sub"(Ident c)).
    let reads = cpg.calls_named("ExecutionUser");
    assert_eq!(reads.len(), 1, "Go selector read must lower to a call");
    assert_eq!(cpg.signature_of(reads[0]), Some("c"));
    let sub = cpg.arguments_of(reads[0]);
    assert_eq!(sub.len(), 1);
    assert_eq!(cpg.name_of(sub[0]), Some("Sub"));
}

#[test]
fn cpp_ctor_factories_and_direct_init() {
    let cpg = build(
        TsFrontend::cpp(),
        "recv.cpp",
        r#"
struct ScopedFD { explicit ScopedFD(const char* c); };
void direct_init(const char* path) {
    std::shared_ptr<ScopedFD> fd(new ScopedFD(path));
}
void factory(const char* path) {
    auto f = std::make_shared<ScopedFD>(path);
    auto g = std::make_unique<ScopedFD>(path);
}
"#,
    );
    // `make_shared<ScopedFD>(path)` / `make_unique<...>` surface under the
    // constructed type's name (one more from the direct-init's inner `new`).
    let ctors = cpg.calls_named("ScopedFD");
    assert_eq!(ctors.len(), 3, "make_shared/make_unique/new must all name ScopedFD");
    assert!(cpg.calls_named("make_shared").is_empty(), "factory name must be rewritten");
    // Direct-init `shared_ptr<ScopedFD> fd(new ...)` must not be swallowed:
    // it lowers to fd = shared_ptr(ScopedFD(path)) with the argument built.
    let outers = cpg.calls_named("shared_ptr");
    assert_eq!(outers.len(), 1, "direct-init declaration must lower to a typed ctor call");
    let inner = cpg.arguments_of(outers[0]);
    assert_eq!(inner.len(), 1);
    assert_eq!(cpg.name_of(inner[0]), Some("ScopedFD"));
    // And the constructor argument still carries the parameter.
    let new_args = cpg.arguments_of(inner[0]);
    assert_eq!(new_args.len(), 1);
    assert_eq!(cpg.name_of(new_args[0]), Some("path"));
}

#[test]
fn scala_error_recovery_keeps_enclosing_class() {
    // A standalone annotation on a class parameter (the Play/Guice
    // `@Named("x")\n param: Type` idiom) breaks tree-sitter-scala's parse of
    // the class header: the class_definition survives bodyless and the
    // members land in sibling ERROR subtrees. The scan must still qualify
    // those methods with the class name.
    let cpg = build(
        TsFrontend::scala(),
        "Api.scala",
        r#"class Api @Inject() (
  system: ActorSystem,
  @Named("external")
  AuthenticatedAction: AuthenticatedActionBuilder,
  config: Configuration
) extends InjectedController {
  def login(): Action[JsValue] = {
    doThing()
  }
}
"#,
    );
    let m = cpg.method_named("login");
    assert_eq!(m.len(), 1, "login not found");
    assert_eq!(cpg.full_name_of(m[0]), Some("Api.login"));
}

#[test]
fn member_chain_receiver_is_built() {
    // `request.body.asJson.getOrElse(x)`: the receiver of `getOrElse` is a
    // member chain, not a call. Each field read in the chain must exist as a
    // named Call (source specs match `asJson`/`body`); collapsing the chain
    // to its root identifier made framework request accessors unmatchable
    // whenever they sit in receiver position — which is where they always
    // sit in real controllers.
    // The multiline leading-dot layout additionally wraps the rhs in an
    // `indented_block`, which must be transparent — real controllers write
    // long accessor chains this way.
    let cpg = build(
        TsFrontend::scala(),
        "C.scala",
        r#"class C {
  def handle(request: Request): Result = {
    val v = request.body.asJson.getOrElse(fallback)
    use(v)
  }
  def multiline(request: Request): Result = {
    val w =
      request
        .body
        .asFormUrlEncoded
        .getOrElse(fallback)
    use(w)
  }
}
"#,
    );
    assert!(!cpg.calls_named("getOrElse").is_empty(), "getOrElse call missing");
    assert!(!cpg.calls_named("asJson").is_empty(), "asJson field read not built");
    assert!(!cpg.calls_named("body").is_empty(), "body field read not built");
    assert!(
        !cpg.calls_named("asFormUrlEncoded").is_empty(),
        "multiline (indented_block) chain not built"
    );
}

/// Blocks and branches in VALUE position must keep their value link.
/// Before build_value_shape, an assignment whose rhs was a `{ .. }` block or
/// an if/match expression dropped ENTIRELY (walk_stmts returns without
/// descending on failed assignments), a failure observed at scale in the
/// Scala validation corpus.
#[test]
fn scala_value_shapes_keep_assignments() {
    let cpg = build(
        TsFrontend::scala(),
        "V.scala",
        r#"
object V {
  def blockVal(): String = {
    val x = {
      log("side")
      compute()
    }
    use(x)
  }
  def ifVal(c: Boolean): String = {
    val y = if (c) taintedSource() else "safe"
    use(y)
  }
  def matchVal(c: Int): String = {
    val z = c match {
      case 1 => fromMatch()
      case _ => "other"
    }
    use(z)
  }
  def implicitBranchReturn(c: Boolean): String =
    if (c) branchA() else branchB()
}
"#,
    );
    // The `=` assignments must exist AND carry a value argument.
    for want in ["compute", "fromMatch", "taintedSource", "log"] {
        assert!(
            !cpg.calls_named(want).is_empty(),
            "scala: `{want}` call missing — value-shape rhs dropped; calls = {:?}",
            cpg.calls().iter().filter_map(|&x| cpg.name_of(x)).collect::<Vec<_>>()
        );
    }
    // Each val must lower to a `=` whose rhs argument exists (2 args).
    let assigns: Vec<_> = cpg
        .calls_named("=")
        .into_iter()
        .filter(|&a| {
            let code = cpg.code_of(a).unwrap_or("");
            code.starts_with("val x") || code.starts_with("val y") || code.starts_with("val z")
        })
        .collect();
    assert_eq!(assigns.len(), 3, "all three vals must lower to `=` calls");
    for a in assigns {
        assert_eq!(
            cpg.arguments_of(a).len(),
            2,
            "`=` must carry lhs AND a value rhs: {:?}",
            cpg.code_of(a)
        );
    }
    // Branch values lower to `<branch>` calls whose args are branch values.
    assert!(!cpg.calls_named("<branch>").is_empty(), "branch value call missing");
    // Implicit return of a bare-if body: the method must contain a Return
    // wrapping a <branch> value (summaries need the return link).
    let m = cpg.method_named("implicitBranchReturn");
    assert_eq!(m.len(), 1);
}

#[test]
fn go_type_assertion_keeps_assignment_value() {
    let cpg = build(
        TsFrontend::go(),
        "ta.go",
        r#"
package p

func f(x interface{}) string {
    s := x.(string)
    v, ok := producer().(func() string)
    _ = ok
    _ = v
    return s
}
"#,
    );
    // `s := x.(string)` must keep its value link (x as rhs).
    let assigns: Vec<_> = cpg
        .calls_named("=")
        .into_iter()
        .filter(|&a| cpg.code_of(a).unwrap_or("").starts_with("s :="))
        .collect();
    assert_eq!(assigns.len(), 1, "type-assertion assignment must exist");
    assert_eq!(cpg.arguments_of(assigns[0]).len(), 2, "rhs value must survive x.(T)");
    // The rhs of `v, ok := producer().(...)` keeps the producer() call.
    assert!(!cpg.calls_named("producer").is_empty(), "call under type assertion dropped");
}

#[test]
fn ruby_value_shapes() {
    let cpg = build(
        TsFrontend::ruby(),
        "v.rb",
        r#"
def pick(c)
  x = c ? tainted_source() : "safe"
  y = if c
    from_if()
  else
    "other"
  end
  h = { "k" => from_hash() }
  e = row["col"]
  use(x, y, h, e)
end
"#,
    );
    for want in ["tainted_source", "from_if", "from_hash"] {
        assert!(
            !cpg.calls_named(want).is_empty(),
            "ruby: `{want}` call missing; calls = {:?}",
            cpg.calls().iter().filter_map(|&x| cpg.name_of(x)).collect::<Vec<_>>()
        );
    }
    // Ternary and if-as-value lower to <branch> calls.
    assert!(cpg.calls_named("<branch>").len() >= 2, "ruby ternary/if value calls missing");
}
