//! Entry mining from framework registration sites.
//!
//! IDL mining covers services whose handlers are declared in .proto/.thrift,
//! but plenty of attacker-facing methods are wired up purely in code: HTTP
//! routers (`http.HandleFunc("/x", handler)`, `mux.Handle(...)`,
//! `r.GET(path, h)`), event/queue consumers (`Subscribe(topic, onMessage)`),
//! and their equivalents in other languages. The registration call itself is
//! the evidence: a function value handed to one of these APIs IS a handler,
//! no name-shape heuristics needed.
//!
//! The miner scans every call named like a registration API and collects the
//! methods its function-valued arguments refer to. A function value appears
//! in the graph as a bare [`NodeKind::Identifier`], as a
//! [`NodeKind::MethodRef`] (an inline closure handler, mined as a
//! position-qualified `name@file:line` entry since closure names are
//! ambiguous), or — via the member-read lowering — a Call named after the
//! method with the receiver as its only argument and no `(` in its code
//! (`s.handleFoo`, not `s.handleFoo()`). One level of call nesting is also
//! searched so the ubiquitous middleware wrap (`Handle("/x", auth(handleFoo))`)
//! still mines `handleFoo`.
//!
//! Three precision guards keep name collisions from flooding the entry set:
//! - Registration names that double as generic verbs (`Get`, `Delete`,
//!   `on`, ...) only count when the call's first argument is a
//!   route-shaped string literal (`"/users"`) — that is how routers are
//!   invoked, and how a CQL builder's `Delete(table, cols)` is not.
//! - A reference name matching more than [`MAX_MATCHES`] same-named methods
//!   is skipped: registration evidence points at one function, and mining
//!   every `Run` in the repo is noise, not evidence.
//! - Methods defined in test/mock/fake files (path convention shared with
//!   the call graph's test-double demotion) are never mined.
//!
//! Mined names feed [`super::taint::TaintSpec::source_methods_registered`]:
//! matched by simple or full name, exempt from the IDL handler-shape gate
//! (the registration is stronger evidence than a parameter type), but still
//! subject to the `Context`-parameter skip so `func(ctx, msg)` consumers do
//! not taint the entire program through their context argument.

use cpg_core::{Cpg, NodeId, NodeKind, Query};
use std::collections::{BTreeSet, HashMap};

/// Position-qualified entry spelling for methods whose name alone is
/// ambiguous — inline closures are all named `<anon>` (or a borrowed binding
/// name), so the mined entry pins the definition site instead.
pub(crate) fn positional_entry(name: &str, file: &str, line: u32) -> String {
    format!("{name}@{file}:{line}")
}

/// Parse a `name@file:line` entry back into its parts. Strict on purpose:
/// the suffix after the LAST `:` must be a line number and the middle part
/// must look like a path (contains `/` or `.`), so sink specs (`open@1`)
/// and glob entries (`NAMEPAT@FILEPAT`) never parse as positional.
pub(crate) fn parse_positional(entry: &str) -> Option<(&str, &str, u32)> {
    let (name, rest) = entry.split_once('@')?;
    let (file, line) = rest.rsplit_once(':')?;
    if name.is_empty() || file.is_empty() || !(file.contains('/') || file.contains('.')) {
        return None;
    }
    Some((name, file, line.parse().ok()?))
}

/// Registration APIs whose name alone is evidence — these are not used for
/// anything else in practice.
pub(crate) const STRONG_CALLS: &[&str] = &[
    "HandleFunc",
    "Handle",
    "Subscribe",
    "subscribe",
    "RegisterHandler",
    "AddHandler",
    "AddEventHandler",
    "HandleMessage",
    "OnMessage",
    "Consume",
    "RegisterConsumer",
];

/// Registration APIs named after HTTP verbs or generic hooks — real router
/// use (`r.GET("/path", h)`) passes a route literal first; a same-named
/// query-builder or mock API (`qb.Delete(table, cols)`, mockery's
/// `m.On("Method", ...)`) does not.
pub(crate) const VERB_CALLS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ALL", "Any", "Get", "Post",
    "Put", "Patch", "Delete", "get", "post", "put", "patch", "delete", "all", "use", "on", "On",
];

/// A name matching more methods than this is too ambiguous to mine.
const MAX_MATCHES: usize = 3;

/// One mined registration call site: the file it lives in, the receiver
/// variable it registers on (`authorized.GET(..)` -> `authorized`, from the
/// member-call signature stamp), and the entry spellings of the handlers it
/// registers. The receiver is what the route-group gate lane keys on; the
/// verb/route/line feed the route table ([`mine_routes`]).
pub(crate) struct Registration {
    pub file: cpg_core::FileId,
    pub recv: Option<String>,
    pub entries: Vec<String>,
    pub verb: String,
    pub route: Option<String>,
    pub line: u32,
}

/// One row of the mined route table: URL (or topic), registration verb,
/// handler entry spelling, and the registration site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct RouteEntry {
    pub route: String,
    pub verb: String,
    pub handler: String,
    pub file: String,
    pub line: u32,
}

/// The route table: every registration site whose first argument is a string
/// literal, joined with the handlers it registers. This is the census's URL
/// column — a `none` verdict with its route attached reads as "THIS path is
/// unauthenticated", not just "this function is".
pub fn mine_routes(cpg: &Cpg) -> Vec<RouteEntry> {
    let mut out = Vec::new();
    for r in mine_registrations(cpg) {
        let Some(route) = r.route else { continue };
        let file = cpg.path_of(r.file).unwrap_or("").to_string();
        for h in r.entries {
            out.push(RouteEntry {
                route: route.clone(),
                verb: r.verb.clone(),
                handler: h,
                file: file.clone(),
                line: r.line,
            });
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Mine handler entry points from registration call sites: every method a
/// registration call receives by value, as its matching name (full name when
/// the method has one — receiver-qualified in Go, `Type::m` in C++ — else
/// the simple name). Deterministically ordered.
pub fn mine_registration_entries(cpg: &Cpg) -> Vec<String> {
    let mut mined = BTreeSet::new();
    for r in mine_registrations(cpg) {
        mined.extend(r.entries);
    }
    mined.into_iter().collect()
}

/// Per-call-site registration mining (see [`Registration`]); the flat entry
/// list is [`mine_registration_entries`].
pub(crate) fn mine_registrations(cpg: &Cpg) -> Vec<Registration> {
    // Simple name -> defined methods. Zero-parameter methods are excluded
    // (a handler always takes at least the request/message), and so are
    // test/mock/fake files — accidental identifier collisions land exactly
    // there (`Mock*_Call.Run`).
    let mut by_name: HashMap<&str, Vec<NodeId>> = HashMap::new();
    let mut by_line: HashMap<(&str, u32), Vec<NodeId>> = HashMap::new();
    let mut by_file: HashMap<cpg_core::FileId, Vec<(u32, NodeId)>> = HashMap::new();
    for m in cpg.methods() {
        if cpg.parameters_of(m).is_empty() {
            continue;
        }
        if cpg
            .path_of(cpg.file_of(m))
            .is_some_and(crate::callgraph::is_test_path)
        {
            continue;
        }
        if let Some(n) = cpg.name_of(m) {
            by_name.entry(n).or_default().push(m);
            if let Some(ln) = cpg.line_of(m) {
                by_line.entry((n, ln)).or_default().push(m);
                by_file.entry(cpg.file_of(m)).or_default().push((ln, m));
            }
        }
    }
    for v in by_file.values_mut() {
        v.sort_unstable();
    }
    let mut regs = Vec::new();
    for c in cpg.calls() {
        let Some(name) = cpg.name_of(c) else { continue };
        let strong = STRONG_CALLS.contains(&name);
        if !strong && !VERB_CALLS.contains(&name) {
            continue;
        }
        let args = cpg.arguments_of(c);
        if !strong
            && !args.first().is_some_and(|&a| is_route_literal(cpg, a))
            && !receiver_is_router(cpg, c)
        {
            continue;
        }
        let mut mined = BTreeSet::new();
        for &a in &args {
            collect_function_refs(cpg, a, &by_name, &by_line, &mut mined, true);
        }
        // Decorator registration (`@app.post("/score")` over `def score(..)`):
        // the handler is the DECORATED function, not an argument, so the call
        // carries only the route. When no function-valued argument resolved,
        // mine the nearest method DEFINED in the same file within a few lines
        // below the call — the decorated function starts on the next line
        // (or a couple further under stacked decorators).
        if mined.is_empty() {
            if let (Some(cl), Some(ms)) = (cpg.line_of(c), by_file.get(&cpg.file_of(c))) {
                let i = ms.partition_point(|&(ln, _)| ln <= cl);
                if let Some(&(ml, m)) = ms.get(i) {
                    if ml <= cl + 3 {
                        // A decorated handler is always NAMED (`def score`);
                        // an adjacent anonymous lambda is a coincidence of
                        // line numbers, and its bare `<anon>` spelling would
                        // resolve to every lambda in the module.
                        if let Some(n) = cpg.name_of(m).filter(|n| !n.starts_with('<')) {
                            mined.insert(cpg.full_name_of(m).unwrap_or(n).to_string());
                        }
                    }
                }
            }
        }
        if mined.is_empty() {
            continue;
        }
        // Middleware-chain sibling args (martini/express):
        // `r.Get("/dash", PageRequireUserAuth, HTTPDashboard)` passes the
        // GUARD as a sibling function value. An authz-NAMED function among
        // several registered values is the guard, not a handler — it becomes
        // wrapper evidence (mine_route_wrappers), never an entry.
        if mined.len() > 1 {
            let handlers: BTreeSet<String> = mined
                .iter()
                .filter(|e| {
                    let simple = e.rsplit('.').next().unwrap_or(e);
                    let simple = simple.split('@').next().unwrap_or(simple);
                    !crate::authz::is_authz_name(simple)
                })
                .cloned()
                .collect();
            if !handlers.is_empty() {
                mined = handlers;
            }
        }
        // The route (or topic) string: a literal first argument, unquoted;
        // for resources-table registrations the template EXPRESSION
        // (`Resources.Members.Template()`) is kept verbatim — still the most
        // useful URL column available.
        let route = args.first().and_then(|&a| {
            let t = cpg.code_of(a)?.trim();
            if cpg.kind_of(a) == NodeKind::Literal {
                Some(t.trim_matches(|q| q == '"' || q == '\'' || q == '`').to_string())
            } else {
                // Route constant / template expression — keep it verbatim
                // (`utils.ClusterIDEndpoint`, `Resources.X.Template()`).
                Some(t.to_string())
            }
        });
        regs.push(Registration {
            file: cpg.file_of(c),
            recv: cpg.signature_of(c).map(str::to_string),
            entries: mined.into_iter().collect(),
            verb: name.to_string(),
            route,
            line: cpg.line_of(c).unwrap_or(0),
        });
    }
    regs
}

/// A verb call dispatched through a variable whose LOCAL type hint is a
/// router type (`router *routing.Router`, in-house REST frameworks whose
/// route templates come from a resources table instead of a literal —
/// `router.GET(Resources.Members.Template(), h)`). The type hint is stamped
/// on the call by the frontend when the receiver's type is locally visible
/// (typed parameter, `var x T`, `T{}`); a query-builder's `Delete(table,
/// cols)` receiver is typed `*queryBuilder`, never a router type.
pub(crate) fn receiver_is_router(cpg: &Cpg, c: NodeId) -> bool {
    cpg.type_full_name_of(c).is_some_and(is_router_type)
}

/// Router-shaped type name, by base segment: shared by the verb-call
/// receiver gate above and the mount-forwarding PARAMETER gate (a route
/// group forwarded into a callee only registers through a router-typed
/// parameter — `gin.IRouter`, `chi.Router`, `*mux.Router`, `EchoRouter`).
pub(crate) fn is_router_type(t: &str) -> bool {
    let base = t
        .rsplit(|ch: char| ch == '.' || ch == ':' || ch == '/' || ch == '*' || ch == '&')
        .next()
        .unwrap_or(t);
    matches!(base, "Router" | "Engine" | "ServeMux" | "RouterGroup" | "Mux")
        || base.ends_with("Router")
}

/// A string literal whose text begins with `/` — the universal route shape.
pub(crate) fn is_route_literal(cpg: &Cpg, n: NodeId) -> bool {
    if cpg.kind_of(n) != NodeKind::Literal {
        return false;
    }
    let code = cpg.code_of(n).unwrap_or("").trim_start();
    code.starts_with("\"/") || code.starts_with("'/") || code.starts_with("`/")
}

/// If `n` is a function-valued reference to a defined method, record it.
/// `descend` allows one level into a wrapping call's arguments (middleware).
fn collect_function_refs(
    cpg: &Cpg,
    n: NodeId,
    by_name: &HashMap<&str, Vec<NodeId>>,
    by_line: &HashMap<(&str, u32), Vec<NodeId>>,
    mined: &mut BTreeSet<String>,
    descend: bool,
) {
    match cpg.kind_of(n) {
        NodeKind::Identifier => record(cpg, n, cpg.name_of(n), by_name, mined),
        // An inline closure (`HandleFunc("/x", func(w, r) {...})`) is lowered
        // to a separate Method with a MethodRef left in argument position.
        // Its name (`<anon>`, or a borrowed binding name) is ambiguous, so
        // the mined entry is position-qualified: `name@file:line`, resolved
        // by name AND line like the authz closure walk.
        NodeKind::MethodRef => {
            if let (Some(nm), Some(ln)) = (cpg.name_of(n), cpg.line_of(n)) {
                for &m in by_line.get(&(nm, ln)).map(Vec::as_slice).unwrap_or(&[]) {
                    // Name+line alone is ambiguous ACROSS files (`<anon>` at
                    // line 103 exists in dozens of files) — the closure
                    // Method lives in the same file as its MethodRef.
                    if cpg.file_of(m) != cpg.file_of(n) {
                        continue;
                    }
                    if let Some(file) = cpg.path_of(cpg.file_of(m)) {
                        mined.insert(positional_entry(nm, file, ln));
                    }
                }
            }
        }
        NodeKind::Call => {
            // A member-read lowering (`s.handleFoo`) is a Call whose code has
            // no argument list — that is a method VALUE. Code with `(` is an
            // actual invocation: not itself a handler reference, but its
            // arguments may be (`auth(handleFoo)`), so search one level down.
            let code = cpg.code_of(n).unwrap_or("");
            if !code.contains('(') {
                record(cpg, n, cpg.name_of(n), by_name, mined);
            } else if descend {
                for a in cpg.arguments_of(n) {
                    collect_function_refs(cpg, a, by_name, by_line, mined, false);
                }
            }
        }
        _ => {}
    }
}

fn record(
    cpg: &Cpg,
    n: NodeId,
    name: Option<&str>,
    by_name: &HashMap<&str, Vec<NodeId>>,
    mined: &mut BTreeSet<String>,
) {
    let Some(name) = name else { return };
    let Some(methods) = by_name.get(name) else { return };
    if methods.len() > MAX_MATCHES {
        // Ambiguity rescue: a member-value reference whose base is locally
        // typed carries the receiver TYPE as its hint (`tokenController.
        // ActivateToken` -> `TokenController`), which names the intended
        // method outright — an OpenAPI-generated package deliberately reuses
        // every operation name (wrapper, strict handler, real handler), so
        // simple names routinely exceed the cap the moment the generated
        // file is in the graph.
        if let Some(hint) = cpg.type_full_name_of(n) {
            // The hint may name the INTERFACE the base is declared as
            // (`tokenController middleware.TokenControllerInterface`); Go's
            // `XInterface` naming convention bridges it to the impl type.
            let prefixes =
                [format!("{hint}."), format!("{}.", hint.strip_suffix("Interface").unwrap_or(hint))];
            for &m in methods {
                if cpg
                    .full_name_of(m)
                    .is_some_and(|f| prefixes.iter().any(|p| f.starts_with(p.as_str())))
                {
                    mined.insert(cpg.full_name_of(m).unwrap_or(name).to_string());
                }
            }
        }
        return;
    }
    for &m in methods {
        mined.insert(cpg.full_name_of(m).unwrap_or(name).to_string());
    }
}
