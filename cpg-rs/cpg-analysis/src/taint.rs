//! Interprocedural source→sink taint queries over function summaries.
//!
//! This is the security-facing query the whole platform exists to answer:
//! *does attacker-controlled data from a `source` reach a dangerous `sink`?*
//! It runs on top of the summary cache, so it inherits the engine's two key
//! properties — it scales (summaries are precomputed and reused, no per-query
//! re-exploration of callees) and it stays correct across edits (a changed
//! file invalidates exactly the affected summaries before the next query).
//!
//! The analysis is intraprocedural taint *within* each method, lifted
//! interprocedurally by consulting callee summaries: a call propagates taint
//! from a tainted argument to the call's result iff the callee's summary maps
//! that parameter to its return *raw* (unsanitized). A finding is raised when
//! a tainted value reaches an argument of a configured sink.
//!
//! Sanitizers: a call to a name in [`TaintSpec::sanitizers`] (or in the
//! summary store's sanitizer set) never propagates taint from its arguments
//! to its result, and callee-summary flows marked sanitized are not lifted.
//! When a computed callee's raw flow is lifted, the callee's internal
//! expression chain is additionally re-checked against the sanitizer set, so
//! a path that is only realisable through a sanitizer inside the callee is
//! not reported either.
//!
//! Witness paths are auditable: every [`Step`] records its [`Provenance`]
//! (intraprocedural propagation, a computed-summary lift, or an external
//! summary with no body) and a `depth` marker. Lifting through an analysable
//! callee splices the callee's internal source-param→return chain into the
//! path at `depth + 1`; external (JSON) summaries have no body, so those hops
//! appear as a single summary-only step.

use crate::pass::method_name_index;
use crate::summaries::{is_operator, lhs_name, SummaryOrigin, SummaryStore};
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Callee-splicing recursion bound: beyond this depth a lifted hop is shown
/// summary-only (no internal steps) rather than expanded further.
const MAX_SPLICE_DEPTH: u32 = 8;

/// Sentinel argument index for `name@out*`: every argument is an out-param.
pub const OUT_ALL_ARGS: usize = usize::MAX;

/// What counts as a source, a sink, and a sanitizer, by function name.
#[derive(Clone)]
pub struct TaintSpec {
    /// Calls to these names produce tainted values (their return is tainted).
    pub sources: HashSet<String>,
    /// Sources written `name@out<k>`: the call writes attacker data INTO its
    /// argument at position `k` (0-based) instead of returning it — the
    /// C read-into-buffer convention (`read(fd, buf, n)`, `fread(buf, ...)`,
    /// `JetRetrieveColumn(..., pvData, ...)`). After such a call, the
    /// variable passed at `k` is tainted. `name@out*` (stored as
    /// [`OUT_ALL_ARGS`]) taints EVERY argument — for wrapper families like a
    /// codebase's `Read(...)` overloads where the buffer position varies by
    /// interface; recall-first, triage owns the precision.
    pub out_param_sources: HashMap<String, usize>,
    /// Calls to these names are dangerous; a tainted argument is a finding.
    /// A sink may be written `name@k` to restrict the dangerous position to
    /// argument `k` (0-based) — `QueryContext@1` fires on the query string,
    /// not on the `ctx` handed to every call in the program.
    pub sinks: HashSet<String>,
    /// Parsed `name@k` restrictions: sink name → the only dangerous argument.
    pub sink_arg: HashMap<String, usize>,
    /// Sinks written `=<pattern>`: ASSIGNMENT sinks. Fire when a tainted
    /// value is STORED under a key whose lowercased name contains
    /// `<pattern>` — a member store (`r.Account = tenantID`), a setter
    /// (`SetAccount(tainted)`), or a named argument
    /// (`AuthzContext(accountId = tainted)`). Plain local rebinds
    /// (`account := x`) deliberately do NOT fire: the dangerous shape is
    /// identity persisted into an authority-carrying object, not a temp.
    /// Patterns are stored lowercased; matching is case-insensitive
    /// substring, so the pack names the identity vocabulary (`=account`,
    /// `=tenant`) and the code spells it any way it likes (`Account`,
    /// `TenantId`, `accountUuid`). Born from two confirmed cross-tenant
    /// escalations of exactly this shape: an authz/account field
    /// overwritten from an attacker-decoded payload after the front door.
    pub assign_sinks: HashSet<String>,
    /// Sinks written `::name` fire only on bare (non-member) calls — the
    /// libc syscall `unlink(p)`, not a same-named client stub
    /// `client->unlink(req)`.
    pub bare_only: HashSet<String>,
    /// Sinks written `name@recv` fire when the RECEIVER object is tainted —
    /// the builder/executor pattern (`ps.AddScript(evil); ps.Invoke()`),
    /// where the dangerous call itself takes no arguments.
    pub recv_sinks: HashSet<String>,
    /// Sinks written `recv.name` (`os.Create@0`) fire only when the call
    /// dispatches through the named receiver root: `os.Create(p)` matches;
    /// `permissionEvaluatorFactory.Create(...)` — a name-colliding method on
    /// an unrelated object — does not, and neither does a bare `Create(p)`.
    /// Keyed by the simple name; the value is every declared receiver root.
    /// The receiver is matched textually against the call code
    /// (`recv.name(` / `recv->name(` / `recv::name(`, whole-name-bounded),
    /// so the mechanism is frontend-agnostic. The dotted spelling is ALSO
    /// registered as an exact sink name, because frontends that name member
    /// calls by full callee text (cpg-lang-c) need it verbatim.
    pub recv_qual: HashMap<String, HashSet<String>>,
    /// Calls to these names neutralise taint: the result does NOT inherit
    /// taint from the arguments, so a path that only exists through one of
    /// them is never reported.
    pub sanitizers: HashSet<String>,
    /// Methods whose *parameters* are attacker-controlled — the entry-point
    /// model for RPC/handler services, where input arrives as a request
    /// object rather than through a source call. Every parameter of every
    /// method with one of these names is treated as tainted. These are
    /// CURATED entries (a rule's entryMethods, --entry): trusted verbatim.
    pub source_methods: HashSet<String>,
    /// Entries mined in bulk from IDL (proto rpc names): same model, but a
    /// simple-name match must additionally look like a handler (a
    /// Request/Args-typed parameter), and framework Context params are
    /// skipped — bulk names collide with utilities, curated names don't.
    pub source_methods_guarded: HashSet<String>,
    /// Entries mined from framework REGISTRATION sites (a function value
    /// passed to `HandleFunc`/`Subscribe`/...): the registration is direct
    /// evidence the method is a handler, so no parameter-shape gate applies —
    /// but unlike curated entries the Context-parameter skip still does.
    pub source_methods_registered: HashSet<String>,
    /// Identifiers whose *reads* are attacker-controlled wherever they
    /// appear — framework globals rather than calls or parameters: Flask's
    /// `request`, `sys.argv`, `os.environ`. Any expression rooted at one of
    /// these names is tainted in every method.
    pub source_idents: HashSet<String>,
    /// Names of authorization-check calls for this codebase (a rule pack's
    /// `authz` array). Matched exactly by simple call name; merged with the
    /// general name heuristic in [`crate::authz::is_authz_name`]. Feeds the
    /// advisory authz-dominance annotation only — never affects recall.
    pub authz_methods: HashSet<String>,
    /// Component-confiner names for this rule (a rule pack's `confiners`
    /// array). A confiner is a call or member-store field through which
    /// taint passing means its PLACEMENT at the sink is structurally
    /// confined — for authority-sensitive sinks (SSRF), the URL query/path
    /// writes (`u.RawQuery = q.Encode()`, `QueryEscape(v)`) that leave the
    /// host fixed. Feeds the advisory confinement annotation only — never
    /// affects recall.
    pub confiners: HashSet<String>,
    /// Source names added by the persistence stitch's phase 2 (getter
    /// variants of stored keys). Reads matching these are DATA reads only —
    /// ORM schema-metadata accessors spelling the same field name
    /// (`XColumns.ChildAccountUUID`, `TableNames.X`) are excluded at match
    /// time (see [`is_schema_metadata_read`]). Empty outside phase 2.
    pub persisted_sources: HashSet<String>,
}

/// Is this read an ORM schema-metadata accessor rather than a data read?
/// The sqlboiler-convention bases (`…Columns.Field`, `TableNames.Table`)
/// produce member reads named exactly like the model field, but their value
/// is the column/table NAME CONSTANT — stitching them taints every query
/// built from generated schema constants (observed: fully-parameterized
/// `queries.Raw(insertQuery, args...)` flagged because the INSERT's column
/// list was "tainted" by `ChildAccountUserRoleColumns.ChildAccountUUID`).
fn is_schema_metadata_read(code: &str) -> bool {
    code.contains("Columns.") || code.contains("TableNames.")
}

/// Shape gate for a persisted-source match: a stored key is read back as a
/// FIELD READ (`t.Token` — a paren-less Call under the member-read lowering)
/// or through a get-accessor INVOCATION (`GetToken()`). A bare-key variant
/// that is itself an invocation (`qb.Col(colFid)`, `uint(len(b))`) is a
/// same-named FUNCTION, not a read of stored data — matching those tainted
/// every query-builder column expression and integer cast in reach.
fn persisted_read_shape_ok(name: &str, code: &str) -> bool {
    if name.starts_with("get") || name.starts_with("Get") {
        return true;
    }
    !crate::authz::is_invocation(code, name)
}

/// Names that can carry OBJECT STATE in `method`: its parameters (incl. the
/// receiver) and locally assigned identifiers. Object-state transfer must be
/// restricted to these — a member call through any other root (`slice.
/// Chunked`, `fmt.Sprintf`, `qb.Select`) dispatches through a stateless
/// package/namespace, and letting it accumulate taint makes every later call
/// through the same package return taint (observed: `slice.Chunked(tainted)`
/// tainting `slice`, so `slice.Repeated("?", n)` "returned" attacker data
/// into a fully-parameterized SQL string).
fn method_local_names(cpg: &Cpg, method: NodeId) -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    for p in cpg.parameters_of(method) {
        if let Some(n) = cpg.name_of(p) {
            names.insert(n.to_string());
        }
    }
    for n in crate::pass::ast_descendants(cpg, method) {
        if cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some("=") {
            if let Some(&lhs) = cpg.arguments_of(n).first() {
                if cpg.kind_of(lhs) == NodeKind::Identifier {
                    if let Some(nm) = cpg.name_of(lhs) {
                        names.insert(nm.to_string());
                    }
                }
            }
        }
    }
    names
}

/// Byte offset of the `.` through which `name` is member-dispatched as an
/// invocation in `code` (`recv.name(` — the dot before the LAST such
/// occurrence), or None for bare calls.
fn find_member_dispatch(code: &str, name: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut best = None;
    let mut from = 0;
    while let Some(pos) = code[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        from = start + 1;
        // Walk back over whitespace: Go fluent chains break the line AFTER
        // the dot (`).\n\tQueryRowContext(`).
        let mut i = start;
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        let boundary = i > 0 && bytes[i - 1] == b'.';
        if boundary && code[end..].trim_start().starts_with('(') {
            best = Some(i - 1);
        }
    }
    best
}

impl TaintSpec {
    pub fn new(sources: &[&str], sinks: &[&str]) -> Self {
        Self::with_sanitizers(sources, sinks, &[])
    }

    pub fn with_sanitizers(sources: &[&str], sinks: &[&str], sanitizers: &[&str]) -> Self {
        let mut names = HashSet::new();
        let mut sink_arg = HashMap::new();
        let mut bare_only = HashSet::new();
        let mut recv_sinks = HashSet::new();
        let mut recv_qual: HashMap<String, HashSet<String>> = HashMap::new();
        let mut assign_sinks = HashSet::new();
        for s in sinks {
            // `=<pattern>`: an assignment sink — matched against store keys,
            // not call names, so it never enters the name-keyed sets below.
            if let Some(pat) = s.strip_prefix('=') {
                if !pat.is_empty() {
                    assign_sinks.insert(pat.to_ascii_lowercase());
                    continue;
                }
            }
            let (s, bare) = match s.strip_prefix("::") {
                Some(rest) => (rest, true),
                None => (*s, false),
            };
            if let Some(n) = s.strip_suffix("@recv") {
                recv_sinks.insert(n.to_string());
                continue;
            }
            let (name, arg_k) =
                match s.split_once('@').and_then(|(n, k)| Some((n, k.parse::<usize>().ok()?))) {
                    Some((n, k)) => (n, Some(k)),
                    None => (s, None),
                };
            let mut register = |n: &str| {
                names.insert(n.to_string());
                if let Some(k) = arg_k {
                    sink_arg.insert(n.to_string(), k);
                }
                if bare {
                    bare_only.insert(n.to_string());
                }
            };
            register(name);
            // `recv.name`: register the simple name too (that is what member
            // calls are named in the ts frontends) and record the receiver
            // qualification the shape check will enforce.
            if let Some((recv, simple)) = name.rsplit_once('.') {
                if !recv.is_empty() && !simple.is_empty() {
                    register(simple);
                    recv_qual.entry(simple.to_string()).or_default().insert(recv.to_string());
                }
            }
        }
        let mut plain_sources = HashSet::new();
        let mut out_param_sources = HashMap::new();
        for s in sources {
            let parsed = s.split_once("@out").and_then(|(n, k)| {
                if k == "*" {
                    Some((n, OUT_ALL_ARGS))
                } else {
                    Some((n, k.parse::<usize>().ok()?))
                }
            });
            match parsed {
                Some((n, k)) => {
                    out_param_sources.insert(n.to_string(), k);
                }
                None => {
                    plain_sources.insert(s.to_string());
                }
            }
        }
        TaintSpec {
            sources: plain_sources,
            out_param_sources,
            sinks: names,
            sink_arg,
            assign_sinks,
            bare_only,
            recv_sinks,
            recv_qual,
            sanitizers: sanitizers.iter().map(|s| s.to_string()).collect(),
            source_methods: HashSet::new(),
            source_methods_guarded: HashSet::new(),
            source_methods_registered: HashSet::new(),
            source_idents: HashSet::new(),
            authz_methods: HashSet::new(),
            confiners: HashSet::new(),
            persisted_sources: HashSet::new(),
        }
    }

    /// `::name` sinks only fire on bare calls: the call text must start with
    /// the name itself (optionally `::`-prefixed), not `recv.name(`.
    /// `recv.name` sinks only fire when the call code dispatches through one
    /// of the declared receiver roots.
    fn sink_shape_matches(&self, name: &str, code: &str) -> bool {
        if let Some(recvs) = self.recv_qual.get(name) {
            if !recvs.iter().any(|r| contains_recv_call(code, r, name)) {
                return false;
            }
        }
        // A position-qualified sink (`Raw@0`) names an argument LIST. The
        // field-read lowering makes every member read a Call (`token.Raw` is
        // a Call named `Raw` whose arg 0 is the base) — without this guard,
        // `.Raw`/`.url` field READS match invocation sinks. The rejected
        // shape is precisely "name at the code's tail with no argument
        // list": real invocations (`queries.Raw(q)`), JSX attribute calls
        // (`dangerouslySetInnerHTML={{…}}`), and unqualified sinks (Scala
        // postfix `.!`) all keep firing.
        if self.sink_arg.contains_key(name) {
            let t = code.trim_end();
            if t.ends_with(name) && !crate::authz::is_invocation(t, name) {
                return false;
            }
            // Call-result receiver: `queries.Raw(q, args).ExecContext(ctx, db)`
            // / `Models(mods...).QueryRowContext(ctx, db)` are bound-query
            // fluent APIs — the query text was already consumed upstream (and
            // checked there, e.g. `Raw@0`); the @k position here holds an
            // executor handle. database/sql's real sinks dispatch through a
            // plain identifier receiver (`db.ExecContext(...)`), which keeps
            // firing.
            if let Some(dot) = find_member_dispatch(t, name) {
                if t[..dot].trim_end().ends_with(')') {
                    return false;
                }
            }
        }
        if !self.bare_only.contains(name) {
            return true;
        }
        let head = code.trim_start();
        let head = head.strip_prefix("::").unwrap_or(head);
        head.strip_prefix(name).is_some_and(|rest| rest.trim_start().starts_with('('))
    }

    /// Is argument position `k` of sink `name` a dangerous position?
    fn sink_arg_matches(&self, name: &str, k: usize) -> bool {
        self.sink_arg.get(name).map_or(true, |&want| want == k)
    }

    /// Does store key `key` (a member-store field, setter suffix, or named
    /// argument) match an `=<pattern>` assignment sink? Case-insensitive
    /// substring over the pack's identity vocabulary.
    fn assign_sink_match(&self, key: &str) -> bool {
        if self.assign_sinks.is_empty() {
            return false;
        }
        let k = key.to_ascii_lowercase();
        self.assign_sinks.iter().any(|p| k.contains(p.as_str()))
    }
}

/// What produced a witness step — makes every finding auditable, which
/// matters once non-computed summary tiers (external JSON today, an LLM tier
/// later) can influence results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum Provenance {
    /// Intraprocedural propagation within the enclosing method's body.
    IntraProc,
    /// Taint lifted through the *computed* summary of an analysable callee.
    SummaryFlow { callee_fqn: String },
    /// Taint lifted through an external (JSON) summary — no body exists, so
    /// the hop is summary-only and cannot be expanded.
    ExternalSummary { callee_fqn: String },
}

/// One step along a taint witness (a tainted expression and where it occurs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub code: String,
    pub line: Option<u32>,
    /// What produced this hop.
    pub provenance: Provenance,
    /// Call nesting: 0 = in the method the finding is reported in; k+1 = a
    /// step spliced from inside a callee whose summary lifted the taint at
    /// depth k.
    pub depth: u32,
}

impl Step {
    fn intra(code: &str, line: Option<u32>, depth: u32) -> Step {
        Step { code: code.to_string(), line, provenance: Provenance::IntraProc, depth }
    }
}

/// Compound assignment (`a += b`, `s |= t`): the target READS its old value,
/// so an untainted right-hand side must not clear existing taint. The
/// frontend collapses all assignment forms to a `=` call, so detect the
/// operator from the source text.
fn is_compound_assign(code: &str) -> bool {
    code.find('=').is_some_and(|i| {
        i > 0
            && matches!(
                code.as_bytes()[i - 1],
                b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'<' | b'>'
            )
    })
}

/// Field key of a member STORE (`cfg.executionUser = v`, `p->key = v`): the
/// trailing identifier of the assignment's left-hand side, when that side is
/// a member chain. Plain `x = v` yields None — only stores that survive in
/// the enclosing object (and are therefore re-readable elsewhere) count for
/// the persistence stitch. Recovered from the `=` call's source text because
/// the frontends collapse a member-store LHS to its base identifier.
fn member_store_key(code: &str) -> Option<String> {
    let i = code.find('=')?;
    let bytes = code.as_bytes();
    if i == 0 || bytes.get(i + 1) == Some(&b'=') {
        return None;
    }
    if matches!(
        bytes[i - 1],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'<' | b'>' | b'!' | b'='
    ) {
        return None;
    }
    let lhs = code[..i].trim_end();
    if !lhs.contains('.') && !lhs.contains("->") {
        return None;
    }
    let key: String = lhs
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    (key.len() >= 2 && key.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_'))
        .then_some(key)
}

/// The bare identifier an assignment's rhs reduces to, if it is one. `p = x`,
/// `p = &x` and `p := &x` all arrive here as an Identifier rhs — unary
/// address-of/deref wrappers collapse to the identifier during graph
/// construction in every frontend.
fn rhs_ident(cpg: &Cpg, n: NodeId) -> Option<String> {
    (cpg.kind_of(n) == NodeKind::Identifier)
        .then(|| cpg.name_of(n).map(str::to_string))
        .flatten()
}

/// Alias bookkeeping for a plain (non-member, non-compound) assignment:
/// rebinding dissolves `lhs`'s previous links; an identifier rhs then links
/// the pair both ways. Deliberately ONE level — no transitive closure — the
/// smallest useful model: after `p := &cfg`, object-state taint landing on
/// `p` lands on `cfg` too (and vice versa).
fn record_alias(alias: &mut HashMap<String, HashSet<String>>, lhs: &str, rhs: Option<&str>) {
    if let Some(old) = alias.remove(lhs) {
        for o in old {
            if let Some(s) = alias.get_mut(&o) {
                s.remove(lhs);
            }
        }
    }
    if let Some(r) = rhs.filter(|r| *r != lhs) {
        alias.entry(lhs.to_string()).or_default().insert(r.to_string());
        alias.entry(r.to_string()).or_default().insert(lhs.to_string());
    }
}

/// Copy an OBJECT-STATE taint event onto every alias of `name`: a member
/// store through the variable, an out-param write into it, or object-state
/// transfer via one of its methods — writes through the variable that are
/// visible through its aliases. The walk is the TRANSITIVE closure of the
/// pairwise links (`q := p; p := &cfg` — a write through `q` lands on `cfg`),
/// visited-bounded; rebind-dissolution in [`record_alias`] keeps the classes
/// honest, so the closure only ever spans live links. A value REBIND must
/// never spread: it replaces the variable rather than writing through it (it
/// dissolves the alias instead). Generic because the two walks carry taint
/// as different value types (`Trace` / `Vec<Step>`).
fn spread_to_aliases<V: Clone>(
    alias: &HashMap<String, HashSet<String>>,
    map: &mut HashMap<String, V>,
    name: &str,
    v: &V,
) {
    let mut seen: HashSet<&str> = HashSet::from([name]);
    let mut work: Vec<&str> = vec![name];
    while let Some(cur) = work.pop() {
        let Some(partners) = alias.get(cur) else { continue };
        for p in partners {
            if seen.insert(p.as_str()) {
                map.insert(p.clone(), v.clone());
                work.push(p.as_str());
            }
        }
    }
}

/// FIELD-SENSITIVE taint keys. A member store `cfg.key = tainted` used to
/// taint the whole base object, so `sink(cfg.other)` false-fired. The taint
/// maps now also hold DOTTED keys (`"cfg.key"`, `"p.cfg.key"`): a member
/// store with a clean identifier chain taints only its path; whole-object
/// entries (plain rebinds, object-state transfer, out-param writes,
/// unparseable store bases) keep their plain keys and always win. The
/// helpers below are the entire dotted-key algebra.
///
/// Full dotted LHS path of a member store (`x.a.b = v` / `p->cfg.key = v`
/// -> `x.a.b` / `p.cfg.key`), when EVERY segment is a plain identifier.
/// Subscripted/deref'd/parenthesised bases return None — those stores fall
/// back to whole-object taint on the base.
fn member_store_path(code: &str) -> Option<String> {
    let i = code.find('=')?;
    let bytes = code.as_bytes();
    if i == 0 || bytes.get(i + 1) == Some(&b'=') {
        return None;
    }
    if matches!(
        bytes[i - 1],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'<' | b'>' | b'!' | b'='
    ) {
        return None;
    }
    let lhs = code[..i].trim_end().replace("->", ".");
    if !lhs.contains('.') {
        return None;
    }
    let segs: Vec<&str> = lhs.split('.').map(str::trim).collect();
    if segs.len() < 2
        || !segs.iter().all(|s| {
            let mut ch = s.chars();
            ch.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && ch.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
    {
        return None;
    }
    Some(segs.join("."))
}

/// First field key strictly under `name.` — the deterministic witness that
/// the object CONTAINS tainted data (lowest path wins; map order is
/// arbitrary).
fn field_key_under<'a, V>(map: &'a HashMap<String, V>, name: &str) -> Option<&'a V> {
    map.iter()
        .filter(|(k, _)| {
            k.len() > name.len() + 1
                && k.starts_with(name)
                && k.as_bytes()[name.len()] == b'.'
        })
        .min_by(|a, b| a.0.cmp(b.0))
        .map(|(_, v)| v)
}

/// Whole-name lookup extended with field CONTAINMENT: a variable whose
/// object holds a tainted field is itself tainted when it flows as a whole
/// value (`f(cfg)`, `return cfg`, `cfg.Method()`).
fn lookup_contained<'a, V>(map: &'a HashMap<String, V>, name: &str) -> Option<&'a V> {
    map.get(name).or_else(|| field_key_under(map, name))
}

/// Root identifier + selected fields of a member-READ chain
/// (`cfg.sub.key` = Call "key"(Call "sub"(Ident cfg)) after the field-read
/// lowering). None when the chain passes through anything but no-paren
/// single-argument member reads over a plain identifier — those keep the
/// generic conservative rules.
fn member_read_path(cpg: &Cpg, node: NodeId) -> Option<(String, Vec<String>)> {
    let mut fields: Vec<String> = Vec::new();
    let mut cur = node;
    loop {
        match cpg.kind_of(cur) {
            NodeKind::Identifier => {
                let root = cpg.name_of(cur)?.to_string();
                fields.reverse();
                return Some((root, fields));
            }
            NodeKind::Call => {
                if cpg.code_of(cur).unwrap_or("").contains('(') {
                    return None;
                }
                let name = cpg.name_of(cur)?;
                if name == "=" || is_operator(name) {
                    return None;
                }
                let args = cpg.arguments_of(cur);
                if args.len() != 1 {
                    return None;
                }
                fields.push(name.to_string());
                cur = args[0];
            }
            _ => return None,
        }
    }
}

/// Taint of a member-read chain over `root` selecting `fields`: whole-object
/// (or stored-prefix) taint on any prefix of the path wins — reading a field
/// of a tainted object yields tainted data; a stored key EXTENDING the path
/// means the read returns a struct containing the tainted field.
fn read_path_taint<'a, V>(
    map: &'a HashMap<String, V>,
    root: &str,
    fields: &[String],
) -> Option<&'a V> {
    let mut path = root.to_string();
    if let Some(v) = map.get(&path) {
        return Some(v);
    }
    for f in fields {
        path.push('.');
        path.push_str(f);
        if let Some(v) = map.get(&path) {
            return Some(v);
        }
    }
    field_key_under(map, &path)
}

/// The source-origination test shared by expr_taint's call arm and the
/// member-read chain walk: a spec-source name, minus persisted reads that
/// fail their read-site shape gates or sit in test code — phase-2 keys are
/// generic getter names, so a test harness reading the same key is noise,
/// not a production store→load chain (same demotion class as the call
/// graph's test-double handling). Explicit spec sources are unaffected.
fn call_is_source(ctx: &Ctx, node: NodeId, name: &str) -> bool {
    ctx.spec.sources.contains(name)
        && !(ctx.spec.persisted_sources.contains(name) && {
            let code = ctx.cpg.code_of(node).unwrap_or("");
            is_schema_metadata_read(code)
                || !persisted_read_shape_ok(name, code)
                || ctx
                    .cpg
                    .path_of(ctx.cpg.file_of(node))
                    .is_some_and(crate::callgraph::is_test_path)
        })
}

/// Remove a variable's whole-object entry AND every field entry under it —
/// a plain rebind replaces the value, stale field taint included.
fn remove_subtree<V>(map: &mut HashMap<String, V>, name: &str) {
    map.remove(name);
    map.retain(|k, _| {
        !(k.len() > name.len() && k.starts_with(name) && k.as_bytes()[name.len()] == b'.')
    });
}

/// Field-suffixed sibling of [`spread_to_aliases`]: a member store through
/// one alias is visible through the others at the SAME field path
/// (`p := &cfg; p.key = t` taints `cfg.key`, not all of `cfg`).
fn spread_field_to_aliases<V: Clone>(
    alias: &HashMap<String, HashSet<String>>,
    map: &mut HashMap<String, V>,
    root: &str,
    suffix: &str,
    v: &V,
) {
    let mut seen: HashSet<&str> = HashSet::from([root]);
    let mut work: Vec<&str> = vec![root];
    while let Some(cur) = work.pop() {
        let Some(partners) = alias.get(cur) else { continue };
        for p in partners {
            if seen.insert(p.as_str()) {
                map.insert(format!("{p}{suffix}"), v.clone());
                work.push(p.as_str());
            }
        }
    }
}

/// The provenance of a tainted value: where it originated and the chain of
/// expressions that carried it. Cloned as taint propagates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trace {
    pub origin: String,
    pub steps: Vec<Step>,
}

impl Trace {
    fn extend(&self, code: &str, line: Option<u32>, provenance: Provenance, depth: u32) -> Trace {
        let mut steps = self.steps.clone();
        steps.push(Step { code: code.to_string(), line, provenance, depth });
        Trace { origin: self.origin.clone(), steps }
    }

    /// Append pre-built steps (a callee's internal chain) before a hop.
    fn splice(&self, inner: Vec<Step>) -> Trace {
        let mut steps = self.steps.clone();
        steps.extend(inner);
        Trace { origin: self.origin.clone(), steps }
    }
}

/// A source→sink flow found in one method, with a witness path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub method: String,
    pub sink: String,
    pub sink_line: Option<u32>,
    /// File the sink call itself lives in — the entry method's file is often
    /// several interprocedural hops away, so triage needs this separately.
    pub sink_file: Option<String>,
    /// The source that tainted the value.
    pub origin: String,
    /// The witness: source expression → … → sink, each with its line, the
    /// provenance that produced the hop, and a callee-nesting depth.
    pub path: Vec<Step>,
    /// Guard evidence near the sink (set by [`annotate_guards`]):
    /// `grow-guarded@<line>` — a capacity-growing call (realloc/reserve/
    /// resize/expand/grow) tied to the sink's identifiers precedes the sink
    /// (the grow-before-copy expandable-buffer shape — strongest evidence);
    /// `guarded@<line>` — a bounds/validation statement mentioning a sink
    /// argument precedes the sink; `post-sink-check@<line>` — the only such
    /// check comes AFTER the sink (the check-after-write bug shape); `None`
    /// — no guard found (triage these and post-sink first).
    pub guard: Option<String>,
    /// Authorization-dominance evidence (set by [`crate::authz::annotate_authz`]):
    /// `authz-dominated@<line>` — an authz-shaped call dominates the flow on
    /// the CFG (every execution reaching the sink ran the check);
    /// `authz-partial@<line>` — an authz call exists on the flow but some
    /// path reaches the sink without it (the authz-bypass shape; triage
    /// SECOND); `None` — no authz call near the flow at all (unguarded or
    /// middleware-gated; triage FIRST). Advisory only.
    pub authz: Option<String>,
    /// Component-confinement evidence (set by [`annotate_confined`]):
    /// `confined@<line>:<name>` — the witness path passes through a
    /// pack-declared confiner (rule `confiners` array), so the tainted
    /// value's placement at the sink is structurally confined. For
    /// authority-sensitive sinks (SSRF) this marks query/path-only taint on
    /// a fixed host — still reportable (parameter injection), but triage
    /// AFTER unconfined flows. `None` = no confiner on the witness path (or
    /// the rule declares none). Advisory only; note the witness is ONE path
    /// — an alternate unconfined route with the same origin is not
    /// re-examined.
    pub confined: Option<String>,
}

/// Shared, immutable state for one `find_flows` run.
struct Ctx<'a> {
    cpg: &'a Cpg,
    summaries: &'a SummaryStore,
    spec: &'a TaintSpec,
    /// name -> defining method nodes, for locating callee bodies to splice.
    methods_by_name: HashMap<String, Vec<NodeId>>,
    /// Persistence phase-1 harvest: key -> distinct store call sites where a
    /// `set<Key>` / member-store / named-arg store received tainted data.
    /// With CPG_PERSIST set, phase 2 re-runs the analysis treating the
    /// matching getters as sources — the store→load over-approximation for
    /// API-writes-config, job-reads-it-later chains that dataflow alone
    /// cannot follow. Store sites feed the ubiquity filter, which counts
    /// the DISTINCT ENCLOSING METHODS: a key stored from many methods
    /// (jobInstance, requestContext) would taint every same-named read
    /// program-wide and drowns the report.
    stored: std::cell::RefCell<HashMap<String, HashSet<NodeId>>>,
}

impl Ctx<'_> {
    /// Query-time sanitizers = the spec's plus whatever the summary store was
    /// computed with (so both walkers agree on what neutralises taint).
    fn is_sanitizer(&self, name: &str) -> bool {
        self.spec.sanitizers.contains(name) || self.summaries.sanitizer_names().contains(name)
    }

    /// The body of the callee a computed summary describes, if present.
    fn body_of(&self, name: &str, fqn: &str) -> Option<NodeId> {
        let candidates = self.methods_by_name.get(name)?;
        candidates
            .iter()
            .find(|&&m| self.cpg.full_name_of(m) == Some(fqn))
            .or_else(|| candidates.first())
            .copied()
    }
}

/// The getter-shaped read names a stored key may surface as in phase 2:
/// `get{K}`/`Get{K}`, the key itself, lcfirst (`ExecutionUser` -> `executionUser`)
/// and ucfirst (`executionUser` -> a Go exported field / Java accessor
/// `ExecutionUser`) — plus Go initialism cross-casing, where the SAME field is
/// spelled differently per naming lint: a DB-model `URL` is read back
/// through proto-gen `Url`/`GetUrl`, and a titlecase `Url` store through an
/// exported `URL` field. The reverse (all-caps) direction is bounded to
/// short single-word keys so `Description` never fans out to
/// `DESCRIPTION`-style noise. Used by both the read-ubiquity counter and
/// the phase-2 source set, which must agree on the fan-out.
fn persist_variants(k: &str) -> Vec<String> {
    // Casing bases first, then get/Get accessor forms over EVERY base — the
    // groups must be symmetric (variants(lcfirst(K)) ⊇ the heavy names in
    // variants(K)), or the read-ubiquity filter counts different sums for
    // keys that stitch the same reads (observed: `AssetStore` dropped at 180
    // reads while `assetStore` — same reads minus its GetAssetStore getter
    // count — passed and stitched a DB-handle key).
    let mut bases = vec![k.to_string()];
    let mut cs = k.chars();
    if let Some(c0) = cs.next() {
        bases.push(c0.to_lowercase().chain(cs.clone()).collect());
        bases.push(c0.to_uppercase().chain(cs).collect());
    }
    if k.len() > 1 && k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        bases.push(k[..1].to_string() + &k[1..].to_ascii_lowercase());
    } else if (2..=5).contains(&k.len())
        && k.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && k.chars().skip(1).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        bases.push(k.to_ascii_uppercase());
    }
    let mut v = Vec::new();
    for b in &bases {
        v.push(format!("get{b}"));
        v.push(format!("Get{b}"));
        v.push(b.clone());
    }
    v.sort();
    v.dedup();
    v
}

/// Run the taint query across every method, returning all findings.
pub fn find_flows(cpg: &Cpg, summaries: &SummaryStore, spec: &TaintSpec) -> Vec<Finding> {
    let ctx = Ctx {
        cpg,
        summaries,
        spec,
        methods_by_name: method_name_index(cpg),
        stored: Default::default(),
    };
    let mut findings = run_analysis(&ctx);
    // Persistence stitching (opt-in: CPG_PERSIST=1): treat getters of every
    // key that was STORED with tainted data as sources and re-run. Findings
    // gain a `persisted:` origin prefix. Over-approximates on purpose —
    // any store of key K taints every load of K program-wide.
    if std::env::var_os("CPG_PERSIST").is_some() {
        // Ubiquity filter: a key stored from many DISTINCT METHODS is
        // infrastructure (jobInstance, requestContext, File), not a config
        // field — stitching it taints every same-named read program-wide.
        // Distinct methods, not raw store sites: recall improvements make
        // more store sites visible inside already-counted methods (multiline
        // copy chains), so an absolute site threshold silently ate confirmed
        // keys whenever the graph got better. Threshold tunable via
        // CPG_PERSIST_UBIQ (default 5 distinct store methods).
        let ubiq: usize = std::env::var("CPG_PERSIST_UBIQ")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        // One call-name index for the whole filter stage: the read counter,
        // the sink-relevance rescue and the sink-method map all look up
        // calls by name, and each `calls_named` is a full node scan.
        let mut calls_by_name: HashMap<&str, Vec<NodeId>> = HashMap::new();
        for c in cpg.calls() {
            if let Some(n) = cpg.name_of(c) {
                calls_by_name.entry(n).or_default().push(c);
            }
        }
        // AST-parent walk; returns the node itself if detached (defensive —
        // every harvested store site sits under a method).
        let enclosing_method = |mut n: NodeId| -> NodeId {
            for _ in 0..512 {
                if cpg.kind_of(n) == NodeKind::Method {
                    return n;
                }
                match cpg.in_kind(n, EdgeKind::Ast).next() {
                    Some(p) => n = p,
                    None => return n,
                }
            }
            n
        };
        let (keys, over): (Vec<(String, usize)>, Vec<(String, usize)>) = ctx
            .stored
            .borrow()
            .iter()
            .map(|(k, sites)| {
                let methods: HashSet<NodeId> =
                    sites.iter().map(|&s| enclosing_method(s)).collect();
                (k.clone(), methods.len())
            })
            .partition(|&(_, n)| n < ubiq);
        // Sink-relevance rescue: an over-threshold key whose getter variants
        // are read in a method that also calls a spec sink is exactly what
        // the scan is looking for — dropping it is guaranteed TP loss.
        // Rescued keys still face the read-side filter below, which is what
        // keeps requestContext-shaped keys (thousands of reads) out even
        // when some of those reads share a method with a sink.
        let mut sink_methods: HashSet<NodeId> = HashSet::new();
        for s in spec.sinks.iter().chain(spec.recv_sinks.iter()) {
            for &c in calls_by_name.get(s.as_str()).into_iter().flatten() {
                sink_methods.insert(enclosing_method(c));
            }
        }
        let (rescued, skipped): (Vec<(String, usize)>, Vec<(String, usize)>) =
            over.into_iter().partition(|(k, _)| {
                persist_variants(k).iter().any(|v| {
                    calls_by_name
                        .get(v.as_str())
                        .into_iter()
                        .flatten()
                        .any(|&c| sink_methods.contains(&enclosing_method(c)))
                })
            });
        if !skipped.is_empty() {
            let mut skipped = skipped;
            skipped.sort();
            let list: Vec<String> =
                skipped.iter().map(|(k, n)| format!("{k}({n} methods)")).collect();
            eprintln!(
                "persist: ubiquity filter dropped {} key(s) stored in >= {ubiq} methods: {}",
                list.len(),
                list.join(", ")
            );
        }
        if !rescued.is_empty() {
            let mut listed = rescued.clone();
            listed.sort();
            let list: Vec<String> =
                listed.iter().map(|(k, n)| format!("{k}({n} methods)")).collect();
            eprintln!(
                "persist: sink-relevance rescued {} over-threshold key(s): {}",
                list.len(),
                list.join(", ")
            );
        }
        let mut keys = keys;
        keys.extend(rescued);
        // Read-side ubiquity: a key like `size`/`string`/`data` has few
        // tainted STORES but thousands of same-named reads — stitching it
        // reports program-wide noise. Sum the call sites its getter variants
        // would match; drop past CPG_PERSIST_READS (default 150).
        let max_reads: usize = std::env::var("CPG_PERSIST_READS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(150);
        let read_count = |k: &str| -> usize {
            persist_variants(k)
                .iter()
                .map(|v| calls_by_name.get(v.as_str()).map_or(0, |cs| cs.len()))
                .sum()
        };
        let (keys, read_skipped): (Vec<(String, usize)>, Vec<(String, usize)>) = keys
            .into_iter()
            .map(|(k, _)| { let r = read_count(&k); (k, r) })
            .partition(|&(_, r)| r < max_reads);
        if !read_skipped.is_empty() {
            let mut read_skipped = read_skipped;
            read_skipped.sort();
            let list: Vec<String> =
                read_skipped.iter().map(|(k, r)| format!("{k}({r} reads)")).collect();
            eprintln!(
                "persist: read-ubiquity filter dropped {} key(s) with >= {max_reads} read sites: {}",
                list.len(),
                list.join(", ")
            );
        }
        // Primitive-type key filter: a stored "key" spelled like a language
        // primitive (`uint`, `int64`, `string`) is cast-shaped harvest noise
        // — no real persisted field is named after a bare primitive.
        const PRIMITIVE_KEYS: &[&str] = &[
            "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16",
            "uint32", "uint64", "float32", "float64", "string", "bool", "byte",
            "rune", "uintptr", "long", "short", "double", "float", "char",
        ];
        let keys: Vec<String> = keys
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| !PRIMITIVE_KEYS.contains(&k.to_ascii_lowercase().as_str()))
            .collect();
        if !keys.is_empty() {
            let mut sorted = keys.clone();
            sorted.sort();
            eprintln!("persist: stitching {} key(s): {}", sorted.len(), sorted.join(", "));
        }
        if !keys.is_empty() {
            let mut added: HashSet<String> = HashSet::new();
            for k in &keys {
                added.extend(persist_variants(k));
            }
            let mut spec2 = spec.clone();
            spec2.sources.extend(added.iter().cloned());
            spec2.persisted_sources = added.clone();
            let ctx2 = Ctx {
                cpg,
                summaries,
                spec: &spec2,
                methods_by_name: method_name_index(cpg),
                stored: Default::default(),
            };
            for mut f in run_analysis(&ctx2) {
                if added.contains(&f.origin) {
                    f.origin = format!("persisted:{}", f.origin);
                    findings.push(f);
                }
            }
        }
    }
    annotate_guards(cpg, &mut findings);
    crate::authz::annotate_authz(cpg, spec, &mut findings);
    annotate_confined(spec, &mut findings);
    // Optional triage ordering (CPG_DOWNRANK_GUARDED=1): surface the
    // unguarded and check-after-write witnesses first. Nothing is dropped.
    if std::env::var_os("CPG_DOWNRANK_GUARDED").is_some() {
        downrank_guarded(&mut findings);
    }
    findings
}

/// Stable-sorts findings for triage: unguarded and `post-sink-check@` first,
/// `guarded@` next, `grow-guarded@` last. Order within each tier is
/// preserved; no finding is removed (advisory ordering only).
pub fn downrank_guarded(findings: &mut [Finding]) {
    findings.sort_by_key(|f| match f.guard.as_deref() {
        Some(g) if g.starts_with("grow-guarded") => 2,
        Some(g) if g.starts_with("guarded") => 1,
        _ => 0,
    });
}

/// Does `code` contain a CALL to `name` — `name(` preceded by a
/// non-identifier character (or the start of the string)? Plain substring
/// would let `Encode` match `ReEncode(`.
fn contains_call(code: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(i) = code[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0
            || code[..at]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric() && c != '_');
        let after_ok = code[at + name.len()..].trim_start().starts_with('(');
        if before_ok && after_ok {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// Does `code` contain a call to `name` dispatched through the receiver root
/// `recv` — `recv.name(`, `recv->name(` or `recv::name(`, with `recv`
/// whole-name-bounded on the left and the argument list opening right after
/// `name`? Textual on purpose: it works identically for every frontend and
/// for merged CPGs, at the cost of not seeing through receiver aliasing
/// (`o := os; o.Create(p)`), which is conservative for an opt-in narrowing.
fn contains_recv_call(code: &str, recv: &str, name: &str) -> bool {
    for sep in [".", "->", "::"] {
        let pat = format!("{recv}{sep}{name}");
        let mut from = 0;
        while let Some(i) = code[from..].find(&pat) {
            let at = from + i;
            let before_ok = at == 0
                || code[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| !c.is_alphanumeric() && c != '_');
            let after_ok = code[at + pat.len()..].trim_start().starts_with('(');
            if before_ok && after_ok {
                return true;
            }
            from = at + pat.len();
        }
    }
    false
}

/// Post-pass: attach component-confinement evidence to each finding. A step
/// matches a confiner `N` (from the rule's `confiners` array) when it is a
/// call to `N` or a member STORE into a field named `N`
/// (`u.RawQuery = q.Encode()` matches both `Encode` and `RawQuery`). The
/// LAST matching step on the witness path wins — `confined@<line>:<name>`.
/// Purely advisory: nothing is suppressed, recall is untouched. The
/// confiners concept is general (any "taint passed through X, so its
/// placement at the sink is structurally limited" evidence); the names are
/// pack-supplied because they are library vocabulary, not engine knowledge.
pub fn annotate_confined(spec: &TaintSpec, findings: &mut [Finding]) {
    if spec.confiners.is_empty() {
        return;
    }
    for f in findings.iter_mut() {
        let mut hit: Option<(String, Option<u32>)> = None;
        for s in &f.path {
            let store_key = member_store_key(&s.code);
            for n in &spec.confiners {
                if contains_call(&s.code, n) || store_key.as_deref() == Some(n.as_str()) {
                    hit = Some((n.clone(), s.line));
                }
            }
        }
        f.confined = hit.map(|(n, l)| match l {
            Some(l) => format!("confined@{l}:{n}"),
            None => format!("confined@?:{n}"),
        });
    }
}

/// The core query: intraprocedural walk of every method plus the entry-point
/// model, no post-processing.
fn run_analysis(ctx: &Ctx) -> Vec<Finding> {
    let cpg = ctx.cpg;
    let spec = ctx.spec;
    let mut findings = Vec::new();
    for m in cpg.methods() {
        analyse_method(ctx, m, &mut findings);
    }
    // Entry-point model: every parameter of a source method is tainted; a
    // sanitizer-free path from one to a sink (here or in a callee) reports.
    // A qualified entry (`GatewayServer::mkdir`, `Svc.handle`) matches the
    // method's full name — use those when handler names collide with
    // internal helpers.
    // Position-qualified registered entries (`name@file:line`, how inline
    // closure handlers are mined) need a per-method key build — only pay
    // for it when the set actually carries that form.
    let has_positional = spec
        .source_methods_registered
        .iter()
        .any(|e| crate::entries::parse_positional(e).is_some());
    for m in cpg.methods() {
        let Some(name) = cpg.name_of(m) else { continue };
        let full = cpg.full_name_of(m);
        // Curated entries (rule entryMethods / --entry) are trusted verbatim
        // — by simple name or by a truly qualified full name (a bare
        // function's full name EQUALS its simple name, so it never counts
        // as qualified).
        let trusted = spec.source_methods.contains(name)
            || full.is_some_and(|f| spec.source_methods.contains(f));
        let qualified_guarded =
            full.is_some_and(|f| f != name && spec.source_methods_guarded.contains(f));
        // Registration-mined entries (HandleFunc/Subscribe arguments): the
        // registration site is the handler evidence, so no shape gate — but
        // the Context-parameter skip below still applies.
        let registered = spec.source_methods_registered.contains(name)
            || full.is_some_and(|f| spec.source_methods_registered.contains(f))
            || (has_positional
                && cpg
                    .path_of(cpg.file_of(m))
                    .zip(cpg.line_of(m))
                    .is_some_and(|(file, ln)| {
                        spec.source_methods_registered
                            .contains(&crate::entries::positional_entry(name, file, ln))
                    }));
        let simple_guarded = !trusted
            && !qualified_guarded
            && !registered
            && (spec.source_methods_guarded.contains(name)
                || full.is_some_and(|f| spec.source_methods_guarded.contains(f)));
        if !trusted && !qualified_guarded && !registered && !simple_guarded {
            continue;
        }
        // A bulk-mined SIMPLE-name entry (a flat rpc name from IDL) must
        // look like a handler — some parameter typed `*Request`/`*Args`
        // (the gRPC/thrift convention). Otherwise an rpc named `ReadFile`
        // would taint every same-named utility in the repo.
        if simple_guarded && !looks_like_handler(cpg, m) {
            continue;
        }
        let fqn = cpg.full_name_of(m).unwrap_or(name).to_string();
        for (k, &p) in cpg.parameters_of(m).iter().enumerate() {
            // Framework plumbing, not attacker data: a `ctx context.Context`
            // (or any *Context) parameter is threaded into nearly every call
            // and would taint the whole program. Curated entries opt out of
            // this heuristic — the user named the method deliberately (a Koa
            // `ctx` IS the request).
            if !trusted
                && cpg.type_full_name_of(p).is_some_and(|t| t.ends_with("Context"))
            {
                continue;
            }
            let mut visiting = HashSet::new();
            if let Some(hit) = param_to_sink(ctx, m, k, 0, &mut visiting) {
                let pname = cpg.name_of(p).unwrap_or("?");
                findings.push(Finding {
                    method: fqn.clone(),
                    sink: hit.sink,
                    sink_line: hit.line,
                    sink_file: hit.file,
                    origin: format!("{name}({pname})"),
                    path: hit.steps,
                    guard: None,
                    authz: None,
                    confined: None,
                });
            }
        }
    }
    findings
}

/// Post-pass: attach guard evidence to each finding. A "guard" is a
/// bounds/validation statement in the sink's own method that mentions one of
/// the sink call's identifiers — a `CCHECK_LE(len, cap)`, an
/// `if (n > bufsiz)`, a `min(...)` clamp, a `require(...)`. A "grow" is a
/// capacity-growing call (realloc/reserve/resize/expand/grow/ensure-capacity)
/// before the sink whose arguments tie to the sink's identifiers, directly or
/// through one single-assignment hop (`required = len + n; Expand(required);
/// memcpy(&buf[len], src, n)`) — the grow-before-copy expandable-buffer
/// shape. Classification precedence: `grow-guarded@<line>` (dest capacity is
/// grown before the copy — triage last), `guarded@<line>` (a guard precedes
/// the sink — triage later), `post-sink-check@<line>` (the ONLY check comes
/// after the sink — the check-after-write bug shape; triage FIRST), `None`
/// (no guard at all). Purely advisory: nothing is suppressed, recall is
/// untouched.
pub fn annotate_guards(cpg: &Cpg, findings: &mut [Finding]) {
    for f in findings.iter_mut() {
        let (Some(sink_line), Some(sink_file)) = (f.sink_line, f.sink_file.as_deref()) else {
            continue;
        };
        // The method CONTAINING the sink (often an interprocedural callee,
        // not f.method): closest preceding method start in the same file.
        let mut best: Option<(u32, NodeId)> = None;
        for m in cpg.methods() {
            if cpg.path_of(cpg.file_of(m)) != Some(sink_file) {
                continue;
            }
            let Some(ml) = cpg.line_of(m) else { continue };
            if ml <= sink_line && best.is_none_or(|(bl, _)| ml > bl) {
                best = Some((ml, m));
            }
        }
        let Some((_, method)) = best else { continue };
        let Some(sink_code) = f.path.last().map(|s| s.code.clone()) else { continue };
        let idents = ident_tokens(&sink_code, &f.sink);
        if idents.is_empty() {
            continue;
        }
        let (mut pre, mut post): (Option<u32>, Option<u32>) = (None, None);
        let mut grow: Option<u32> = None;
        // lhs → rhs-ident map for one-hop widening of grow-call arguments;
        // built lazily on the first grow candidate.
        let mut assign_map: Option<HashMap<String, Vec<String>>> = None;
        for n in crate::pass::ast_descendants(cpg, method) {
            let kind = cpg.kind_of(n);
            if kind != NodeKind::Call && kind != NodeKind::ControlStructure {
                continue;
            }
            let Some(line) = cpg.line_of(n) else { continue };
            if line == sink_line {
                continue;
            }
            let code = cpg.code_of(n).unwrap_or("");
            if kind == NodeKind::Call && line < sink_line {
                let g = cpg.name_of(n).unwrap_or("").trim_start_matches("::");
                if is_grow_name(g) {
                    let grow_idents = ident_tokens(code, g);
                    // An argless grow (`Grow()`) grows the receiver's own
                    // buffer — accept it; otherwise the grow's identifiers
                    // must tie to the sink's, directly or one assignment hop
                    // away (`required = len + n; Expand(required)`).
                    let hit = grow_idents.is_empty() || {
                        let map = assign_map
                            .get_or_insert_with(|| assignment_ident_map(cpg, method));
                        grow_idents.iter().any(|gi| {
                            idents.contains(gi)
                                || map.get(gi).is_some_and(|rhs| {
                                    rhs.iter().any(|ri| idents.contains(ri))
                                })
                        })
                    };
                    if hit && grow.is_none_or(|p| line > p) {
                        grow = Some(line);
                    }
                }
            }
            let is_guard_shape = if kind == NodeKind::ControlStructure {
                // Some frontends store only the node kind as code; the
                // comparison itself then appears as an operator Call below.
                ["<", ">", "<=", ">=", "=="].iter().any(|op| code.contains(op))
            } else {
                let g = cpg.name_of(n).unwrap_or("").trim_start_matches("::");
                // A bare comparison (if-condition, loop bound) is an
                // operator call named "<"/">"/"==" etc.
                matches!(g, "<" | ">" | "<=" | ">=" | "==" | "!=")
                    || g.starts_with("CCHECK")
                    || g.starts_with("CHECK")
                    || g.starts_with("DCHECK")
                    || g.starts_with("assert")
                    || g.starts_with("require")
                    || g.starts_with("verify")
                    || g.starts_with("Verify")
                    || g.starts_with("validate")
                    || g.starts_with("Validate")
                    || matches!(g, "min" | "Min" | "max" | "Max" | "clamp")
            };
            if !is_guard_shape || !idents.iter().any(|id| contains_word(code, id)) {
                continue;
            }
            if line < sink_line {
                if pre.is_none_or(|p| line > p) {
                    pre = Some(line);
                }
            } else if post.is_none_or(|p| line < p) {
                post = Some(line);
            }
        }
        f.guard = match (grow, pre, post) {
            (Some(l), _, _) => Some(format!("grow-guarded@{l}")),
            (None, Some(l), _) => Some(format!("guarded@{l}")),
            (None, None, Some(l)) => Some(format!("post-sink-check@{l}")),
            (None, None, None) => None,
        };
    }
}

/// The IDL simple-name handler-shape gate, shared by the taint entry matcher
/// and the authz census: a bulk-mined SIMPLE-name entry (a flat rpc name from
/// .proto/.thrift) must have some parameter typed `*Request`/`*Req`/`*Args`/
/// `*Argument` (the gRPC/thrift handler convention) — otherwise an rpc named
/// `Get` would match every same-named utility in the repo.
pub(crate) fn looks_like_handler(cpg: &Cpg, m: NodeId) -> bool {
    cpg.parameters_of(m).iter().any(|&p| {
        cpg.type_full_name_of(p).is_some_and(|t| {
            // Versioned request types (`GetIssueRequestV2`) count too:
            // strip one trailing `V<digits>`/`v<digits>` before the test.
            let no_digits = t.trim_end_matches(|c: char| c.is_ascii_digit());
            let base = if no_digits.len() < t.len()
                && (no_digits.ends_with('V') || no_digits.ends_with('v'))
            {
                &no_digits[..no_digits.len() - 1]
            } else {
                t
            };
            base.ends_with("Request")
                || base.ends_with("Req")
                || base.ends_with("Args")
                || base.ends_with("Argument")
        })
    })
}

/// Capacity-growing call vocabulary for the grow-before-copy recognizer.
fn is_grow_name(g: &str) -> bool {
    let l = g.to_ascii_lowercase();
    l.contains("realloc")
        || l.starts_with("reserve")
        || l.starts_with("resize")
        || l.starts_with("expand")
        || l.starts_with("grow")
        || (l.contains("ensure") && l.contains("capacity"))
}

/// lhs identifier → rhs identifier tokens for every plain `x = <expr>`
/// assignment in the method — the one-hop widening table that ties
/// `Expand(required)` to `memcpy(&buf[len], src, n)` through
/// `required = len + n + 3`.
fn assignment_ident_map(cpg: &Cpg, method: NodeId) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for n in crate::pass::ast_descendants(cpg, method) {
        if cpg.kind_of(n) != NodeKind::Call || cpg.name_of(n) != Some("=") {
            continue;
        }
        let args = cpg.arguments_of(n);
        let (Some(&l), Some(&r)) = (args.first(), args.get(1)) else { continue };
        if cpg.kind_of(l) != NodeKind::Identifier {
            continue;
        }
        let Some(lname) = cpg.name_of(l) else { continue };
        let rhs_code = cpg.code_of(r).or_else(|| cpg.code_of(n)).unwrap_or("");
        map.insert(lname.to_string(), ident_tokens(rhs_code, lname));
    }
    map
}

/// Identifier-shaped tokens in a code snippet, minus the sink's own name and
/// keywords too common to be guard evidence.
fn ident_tokens(code: &str, skip: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "if", "else", "return", "new", "const", "void", "char", "int", "auto", "static",
        "size_t", "uint8", "uint32", "uint64", "reinterpret_cast", "static_cast", "cast",
    ];
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in code.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            let t = std::mem::take(&mut cur);
            if t.len() >= 2
                && !t.chars().next().is_some_and(|c| c.is_ascii_digit())
                && t != skip
                && !STOP.contains(&t.as_str())
                && !out.contains(&t)
            {
                out.push(t);
            }
        }
    }
    out
}

/// Word-boundary containment: `len` must not match inside `length`.
fn contains_word(hay: &str, word: &str) -> bool {
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(word) {
        let i = start + pos;
        let before_ok = i == 0
            || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        let end = i + word.len();
        let after_ok = end >= bytes.len()
            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

fn analyse_method(ctx: &Ctx, method: NodeId, out: &mut Vec<Finding>) {
    let cpg = ctx.cpg;
    let method_name = cpg.full_name_of(method).unwrap_or("<anon>").to_string();

    // Tainted variable names, each carrying the provenance that tainted it.
    let mut taint: HashMap<String, Trace> = HashMap::new();
    // Intra-method alias pairs (`p := &cfg`) — object-state taint events on
    // one side spread to the other.
    let mut alias: HashMap<String, HashSet<String>> = HashMap::new();
    // Lazily-computed local/param names (object-state transfer gate).
    let mut local_names: Option<HashSet<String>> = None;

    let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return))
        .filter(|&n| cpg.line_of(n).is_some())
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    // Framework globals (`request`, `argv`) are tainted at every read — but
    // only where the name really IS the global: a parameter or local
    // assignment of the same name shadows it for the whole method (Python's
    // actual scoping rule, and the right call for other languages too).
    if !ctx.spec.source_idents.is_empty() {
        let shadowed = shadowed_names(ctx, method, &stmts);
        for ident in &ctx.spec.source_idents {
            if !shadowed.contains(ident) {
                taint.insert(
                    ident.clone(),
                    Trace {
                        origin: ident.clone(),
                        steps: vec![Step::intra(ident, cpg.line_of(method), 0)],
                    },
                );
            }
        }
    }

    for n in stmts {
        if cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some("=") {
            let args = cpg.arguments_of(n);
            if args.len() == 2 {
                let code = cpg.code_of(n).unwrap_or("");
                let store_key = member_store_key(code);
                // The parsed store path decides store-vs-rebind; store_key
                // (with its length gate) only feeds the persistence harvest.
                let store_path = member_store_path(code);
                let is_member_store = store_key.is_some() || store_path.is_some();
                // Alias bookkeeping is shape-based, taint-independent: a
                // plain rebind re-links (or dissolves) lhs's alias — a
                // member store writes THROUGH lhs and must not touch it.
                if !is_member_store && !is_compound_assign(code) {
                    if let Some(l) = lhs_name(cpg, args[0]) {
                        record_alias(&mut alias, &l, rhs_ident(cpg, args[1]).as_deref());
                    }
                }
                if let Some(trace) = expr_taint(ctx, args[1], &taint) {
                    // Persistence phase-1 (member store): `cfg.key = tainted`
                    // survives in cfg — harvest the field key.
                    if let Some(k) = &store_key {
                        ctx.stored.borrow_mut().entry(k.clone()).or_default().insert(n);
                        // Assignment sink: attacker data stored into an
                        // identity-named field (`r.Account = tenantID`).
                        if ctx.spec.assign_sink_match(k) {
                            out.push(Finding {
                                method: method_name.clone(),
                                sink: format!("={k}"),
                                sink_line: cpg.line_of(n),
                                sink_file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                                origin: trace.origin.clone(),
                                path: trace
                                    .extend(code, cpg.line_of(n), Provenance::IntraProc, 0)
                                    .steps,
                                guard: None,
                                authz: None,
                                confined: None,
                            });
                        }
                    }
                    if let Some(name) = lhs_name(cpg, args[0]) {
                        let trace = trace.extend(
                            cpg.code_of(n).unwrap_or(&name),
                            cpg.line_of(n),
                            Provenance::IntraProc,
                            0,
                        );
                        match store_path
                            .as_ref()
                            .filter(|p| p.starts_with(&name) && p.as_bytes().get(name.len()) == Some(&b'.'))
                        {
                            // Field-sensitive member store: taint the dotted
                            // path (visible through aliases at the SAME
                            // path), not the whole base object.
                            Some(path) => {
                                let suffix = path[name.len()..].to_string();
                                spread_field_to_aliases(&alias, &mut taint, &name, &suffix, &trace);
                                taint.insert(path.clone(), trace);
                            }
                            // Member store with an unparseable base
                            // (subscript/deref): whole-object, through
                            // aliases — the pre-field-sensitivity behavior.
                            None if is_member_store => {
                                spread_to_aliases(&alias, &mut taint, &name, &trace);
                                taint.insert(name, trace);
                            }
                            // Plain rebind: replaces the whole value, stale
                            // field taint included.
                            None => {
                                remove_subtree(&mut taint, &name);
                                taint.insert(name, trace);
                            }
                        }
                    }
                } else if let Some(name) = lhs_name(cpg, args[0]) {
                    // Reassignment clears taint — but `x += clean` reads x,
                    // so compound assignment keeps whatever x already had. A
                    // clean MEMBER store clears only its own path: writing
                    // one clean field never launders the rest of the object.
                    if !is_compound_assign(code) {
                        match &store_path {
                            Some(path) => remove_subtree(&mut taint, path),
                            None if is_member_store => {
                                taint.remove(&name);
                            }
                            None => remove_subtree(&mut taint, &name),
                        }
                    }
                }
            }
        }
        // Object-state transfer: a call that feeds a tainted argument into a
        // method of `obj` taints `obj` itself (builder/executor pattern —
        // `ps.AddScript(evil)` leaves the payload inside `ps`). Field-level
        // precision is future work; object-level is the IRIS-faithful
        // over-approximation.
        if cpg.kind_of(n) == NodeKind::Call {
            let name = cpg.name_of(n).unwrap_or("");
            // Out-parameter source: `read(fd, buf, n)` writes attacker bytes
            // INTO buf — the taint appears at an argument position, not in
            // the return value.
            if let Some(&k) = ctx.spec.out_param_sources.get(name) {
                for var in out_arg_names(cpg, n, k) {
                    let tr = Trace {
                        origin: name.to_string(),
                        steps: vec![Step::intra(cpg.code_of(n).unwrap_or(name), cpg.line_of(n), 0)],
                    };
                    // The write into `var` is visible through its aliases
                    // (`p = buf; read(fd, p, n)` fills buf).
                    spread_to_aliases(&alias, &mut taint, &var, &tr);
                    taint.insert(var, tr);
                }
            }
            // Copy-family propagation: `strcpy(buf, user)` relays user's
            // taint into buf (and buf's aliases). Fires only when a
            // source-position argument is already tainted.
            if !ctx.is_sanitizer(name) {
                if let Some((dst, first_src)) = copy_propagation(name) {
                    let args = cpg.arguments_of(n);
                    if let Some(tr) =
                        args.iter().skip(first_src).find_map(|&a| expr_taint(ctx, a, &taint))
                    {
                        let tr = tr.extend(
                            cpg.code_of(n).unwrap_or(name),
                            cpg.line_of(n),
                            Provenance::IntraProc,
                            0,
                        );
                        for var in out_arg_names(cpg, n, dst) {
                            spread_to_aliases(&alias, &mut taint, &var, &tr);
                            taint.insert(var, tr.clone());
                        }
                    }
                }
            }
            // Persistence phase-1: a tainted value stored through a setter
            // is re-readable elsewhere — harvest the field key.
            if (name.starts_with("set") || name.starts_with("Set")) && name.len() > 3 {
                if let Some(trace) =
                    cpg.arguments_of(n).into_iter().find_map(|a| expr_taint(ctx, a, &taint))
                {
                    let k = &name[3..];
                    ctx.stored.borrow_mut().entry(k.to_string()).or_default().insert(n);
                    // Assignment sink: setter form (`SetAccount(tainted)`).
                    if ctx.spec.assign_sink_match(k) {
                        out.push(Finding {
                            method: method_name.clone(),
                            sink: format!("={k}"),
                            sink_line: cpg.line_of(n),
                            sink_file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                            origin: trace.origin.clone(),
                            path: trace
                                .extend(
                                    cpg.code_of(n).unwrap_or(name),
                                    cpg.line_of(n),
                                    Provenance::IntraProc,
                                    0,
                                )
                                .steps,
                            guard: None,
                            authz: None,
                            confined: None,
                        });
                    }
                }
            }
            // Persistence phase-1 (named-arg store): `copy(executionUser = v)` /
            // `Config(user = v)` — an argument that is itself a `=` call
            // with a tainted rhs stores the field named by its lhs.
            if name != "=" && !is_operator(name) {
                for a in cpg.arguments_of(n) {
                    if cpg.kind_of(a) != NodeKind::Call || cpg.name_of(a) != Some("=") {
                        continue;
                    }
                    let aa = cpg.arguments_of(a);
                    if aa.len() == 2 && cpg.kind_of(aa[0]) == NodeKind::Identifier {
                        if let Some(trace) = expr_taint(ctx, aa[1], &taint) {
                            if let Some(k) = cpg.name_of(aa[0]) {
                                ctx.stored
                                    .borrow_mut()
                                    .entry(k.to_string())
                                    .or_default()
                                    .insert(n);
                                // Assignment sink: named-argument form
                                // (`AuthzContext(accountId = tainted)`).
                                if ctx.spec.assign_sink_match(k) {
                                    out.push(Finding {
                                        method: method_name.clone(),
                                        sink: format!("={k}"),
                                        sink_line: cpg.line_of(n),
                                        sink_file: cpg
                                            .path_of(cpg.file_of(n))
                                            .map(str::to_string),
                                        origin: trace.origin.clone(),
                                        path: trace
                                            .extend(
                                                cpg.code_of(n).unwrap_or(name),
                                                cpg.line_of(n),
                                                Provenance::IntraProc,
                                                0,
                                            )
                                            .steps,
                                        guard: None,
                                        authz: None,
                                        confined: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            if name != "=" && !ctx.is_sanitizer(name) && !is_operator(name) {
                // Receiver-sink: a dangerous call on a tainted OBJECT
                // (`ps.Invoke()`), where the payload arrived earlier via
                // object-state transfer, or — fluent chaining — the receiver
                // is itself a chained call carrying the taint
                // (`ps->AddParameter(dn)->Invoke()`).
                if ctx.spec.recv_sinks.contains(name) {
                    let recv_trace = cpg
                        .signature_of(n)
                        .and_then(|r| lookup_contained(&taint, r).cloned())
                        .or_else(|| {
                            cpg.out_kind(n, EdgeKind::Receiver)
                                .next()
                                .and_then(|r| expr_taint(ctx, r, &taint))
                        });
                    if let Some(trace) = recv_trace {
                        out.push(Finding {
                            method: method_name.clone(),
                            sink: name.to_string(),
                            sink_line: cpg.line_of(n),
                            sink_file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                            origin: trace.origin.clone(),
                            path: trace
                                .extend(
                                    cpg.code_of(n).unwrap_or(name),
                                    cpg.line_of(n),
                                    Provenance::IntraProc,
                                    0,
                                )
                                .steps,
                            guard: None,
                            authz: None,
                            confined: None,
                        });
                    }
                }
                // Object-state transfer: a call that feeds a tainted
                // argument into a method of `obj` taints `obj` itself
                // (builder pattern — `ps.AddScript(evil)` leaves the payload
                // inside `ps`). Field-level precision is future work;
                // object-level is the IRIS-faithful over-approximation.
                if let Some(recv) = cpg
                    .signature_of(n)
                    .filter(|r| *r != "<literal>")
                    .filter(|_| !is_query_call(name))
                    .map(str::to_string)
                {
                    // Only real locals carry object state; package/namespace
                    // roots are stateless (see [`method_local_names`]).
                    if !taint.contains_key(&recv)
                        && local_names
                            .get_or_insert_with(|| method_local_names(cpg, method))
                            .contains(&recv)
                    {
                        let hit = cpg
                            .arguments_of(n)
                            .into_iter()
                            .find_map(|a| expr_taint(ctx, a, &taint));
                        if let Some(t) = hit {
                            let t = t.extend(
                                cpg.code_of(n).unwrap_or(name),
                                cpg.line_of(n),
                                Provenance::IntraProc,
                                0,
                            );
                            // Object-state transfer mutates the object — the
                            // payload is visible through its aliases too.
                            spread_to_aliases(&alias, &mut taint, &recv, &t);
                            taint.insert(recv, t);
                        }
                    }
                }
            }
        }
        // Any call (including the assignment's rhs) may be a sink.
        check_sinks(ctx, n, &taint, &method_name, out);
    }
}

/// A named argument at a call site: the nested `=` shape (arg 1 = key
/// Identifier, arg 2 = value) that python keyword_argument, Scala named
/// args, and Go keyed literal elements all lower to.
fn named_arg(cpg: &Cpg, a: NodeId) -> Option<(&str, NodeId)> {
    if cpg.kind_of(a) != NodeKind::Call || cpg.name_of(a) != Some("=") {
        return None;
    }
    let aa = cpg.arguments_of(a);
    if aa.len() == 2 && cpg.kind_of(aa[0]) == NodeKind::Identifier {
        return cpg.name_of(aa[0]).map(|k| (k, aa[1]));
    }
    None
}

/// (callee param index, value node) for each call-site argument — positional
/// by default, name-matched for named arguments (`run(cwd="/tmp", cmd=q)`
/// feeds q to the param NAMED cmd, not to param 1). The value node of a
/// named arg is the VALUE, not the `=` wrapper, so a tainted local sharing
/// the KEY's name can't leak in.
fn args_to_params(cpg: &Cpg, callee: NodeId, args: &[NodeId]) -> Vec<(usize, NodeId)> {
    let pnames: Vec<Option<&str>> =
        cpg.parameters_of(callee).iter().map(|&p| cpg.name_of(p)).collect();
    args.iter()
        .enumerate()
        .map(|(k, &a)| match named_arg(cpg, a) {
            Some((key, val)) => match pnames.iter().position(|pn| *pn == Some(key)) {
                Some(pi) => (pi, val),
                None => (k, val),
            },
            None => (k, a),
        })
        .collect()
}

/// The call-site argument feeding callee parameter `k` (the inverse of
/// [`args_to_params`], for summary flows keyed by param index). With no
/// named arguments this is `args[k]`; with named arguments present, the one
/// whose key matches param k's name wins, and a named argument sitting AT
/// position k never masquerades as the positional one.
fn arg_for_param(
    cpg: &Cpg,
    node: NodeId,
    callee_fqn: &str,
    args: &[NodeId],
    k: usize,
) -> Option<NodeId> {
    if args.iter().any(|&a| named_arg(cpg, a).is_some()) {
        if let Some(m) = cpg
            .call_targets(node)
            .into_iter()
            .find(|&m| cpg.full_name_of(m) == Some(callee_fqn))
        {
            if let Some(pn) = cpg.parameters_of(m).get(k).and_then(|&p| cpg.name_of(p)) {
                if let Some((_, val)) =
                    args.iter().filter_map(|&a| named_arg(cpg, a)).find(|(key, _)| *key == pn)
                {
                    return Some(val);
                }
                return args.get(k).copied().filter(|&a| named_arg(cpg, a).is_none());
            }
        }
    }
    args.get(k).copied()
}

/// Read-only query methods do not store their arguments in the receiver:
/// `entry.template.Match(urlPath)` must not taint `entry` — the builder-
/// pattern object-state transfer models mutators (`ps.AddScript(evil)`).
/// Leading word-token, so `HasPrefix`/`IsValid`/`MatchString` all qualify;
/// deliberately narrow (no `get`/`parse`/`read`: `GetOrCreate` stores,
/// `flag.Parse` stores, `Read` fills its argument).
fn is_query_call(name: &str) -> bool {
    matches!(
        crate::authz::word_tokens(name).first().map(|s| s.as_str()),
        Some(
            "match" | "contains" | "has" | "is" | "equal" | "equals" | "lookup"
                | "find" | "index" | "count" | "len" | "starts" | "ends"
                | "validate" | "verify" | "check"
        )
    )
}

/// A member READ lowers to a Call named after the field (`entry.handler`),
/// which can resolve by simple name to an unrelated same-named function —
/// interprocedural hand-off must only descend when the code actually spells
/// an invocation of `name` (same rejection shape as `sink_shape_matches`).
fn is_invoked(cpg: &Cpg, n: NodeId, name: &str) -> bool {
    let t = cpg.code_of(n).unwrap_or("").trim_end();
    !(t.ends_with(name) && !crate::authz::is_invocation(t, name))
}

/// Shell interpreters whose `-c` argument is a SCRIPT. When an exec-style
/// call (or a command-line assembly like a pod-spec `Command:` slice) spells
/// `…, <shell>, "-c", payload…`, the injectable position is the payload —
/// not wherever the sink spec's qualifier points (`Command@0` models
/// "tainted binary"; `Command("sh", "-c", t)` moves the danger to arg 2).
/// Windows spells the flag `/c`.
const SHELL_NAMES: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "ash", "cmd", "cmd.exe", "powershell",
    "powershell.exe", "pwsh",
];

/// The unquoted text of a string-literal node.
fn literal_text(cpg: &Cpg, n: NodeId) -> Option<String> {
    if cpg.kind_of(n) != NodeKind::Literal {
        return None;
    }
    let c = cpg.code_of(n)?.trim();
    let c = c
        .strip_prefix('"')
        .and_then(|c| c.strip_suffix('"'))
        .or_else(|| c.strip_prefix('\'').and_then(|c| c.strip_suffix('\'')))
        .or_else(|| c.strip_prefix('`').and_then(|c| c.strip_suffix('`')))?;
    Some(c.to_string())
}

fn is_shell_literal(s: &str) -> bool {
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    SHELL_NAMES.contains(&base.to_ascii_lowercase().as_str())
}

fn is_shell_c_flag(s: &str) -> bool {
    s == "-c" || s.eq_ignore_ascii_case("/c")
}

/// If `args` spell `…, <shell literal>, <-c literal>, payload…`, the index
/// of the first payload argument.
fn shellform_payload_start(cpg: &Cpg, args: &[NodeId]) -> Option<usize> {
    args.windows(2)
        .position(|w| {
            literal_text(cpg, w[0]).is_some_and(|s| is_shell_literal(&s))
                && literal_text(cpg, w[1]).is_some_and(|s| is_shell_c_flag(&s))
        })
        .map(|i| i + 2)
}

/// libc copy-family calls move bytes — and therefore taint — from a source
/// argument into a destination buffer argument: after `strcpy(dst, src)`,
/// `dst` carries `src`'s taint. Joern models these with per-function
/// `1->0` semantics files; here it's a fixed table of the C/C++ copy
/// idioms. Unlike an `@out` source the call introduces no taint of its
/// own — it only relays taint already present at a source position.
/// Returns `(dst_arg, first_src_arg)` (0-based).
fn copy_propagation(name: &str) -> Option<(usize, usize)> {
    match name {
        "strcpy" | "strcat" | "stpcpy" | "strlcpy" | "strlcat" | "wcscpy" | "wcscat"
        | "strncpy" | "strncat" | "wcsncpy" | "memcpy" | "memmove" | "mempcpy" | "wmemcpy"
        | "sprintf" | "vsprintf" => Some((0, 1)),
        "snprintf" | "vsnprintf" | "swprintf" => Some((0, 2)),
        _ => None,
    }
}

/// The variables an out-parameter call writes into: the root identifier of
/// the argument at position `k`, or of EVERY argument for `@out*`
/// ([`OUT_ALL_ARGS`]).
fn out_arg_names(cpg: &Cpg, call: NodeId, k: usize) -> Vec<String> {
    let args = cpg.arguments_of(call);
    if k == OUT_ALL_ARGS {
        args.into_iter().filter_map(|a| out_arg_name(cpg, a)).collect()
    } else {
        args.get(k).and_then(|&a| out_arg_name(cpg, a)).into_iter().collect()
    }
}

/// The variable an out-parameter call writes into: the root identifier of
/// the argument expression. `buf` and `&buf` and `buffer.data()` all name
/// the object `buf`/`buffer` — object-level, like the rest of the tracker.
fn out_arg_name(cpg: &Cpg, node: NodeId) -> Option<String> {
    if cpg.kind_of(node) == NodeKind::Identifier {
        return cpg.name_of(node).map(str::to_string);
    }
    let code = cpg.code_of(node)?;
    let trimmed = code.trim_start_matches(['&', '*', '(', ' ', '\t']);
    let root: String =
        trimmed.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if root.is_empty() || root.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(root)
}

/// Names that are method-local in `method`: its parameters plus every
/// assignment target. A source identifier of the same name is shadowed for
/// the whole method (Python scoping; and a deliberate local named `request`
/// in any language is not the framework global).
fn shadowed_names(ctx: &Ctx, method: NodeId, stmts: &[NodeId]) -> HashSet<String> {
    let cpg = ctx.cpg;
    let mut out: HashSet<String> = cpg
        .parameters_of(method)
        .iter()
        .filter_map(|&p| cpg.name_of(p))
        .map(str::to_string)
        .collect();
    for &n in stmts {
        if cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some("=") {
            let args = cpg.arguments_of(n);
            if args.len() == 2 {
                if let Some(l) = lhs_name(cpg, args[0]) {
                    out.insert(l);
                }
            }
        }
    }
    out
}

/// If `node` (or a nested call) is a sink reached by a tainted argument, record it.
fn check_sinks(
    ctx: &Ctx,
    node: NodeId,
    taint: &HashMap<String, Trace>,
    method_name: &str,
    out: &mut Vec<Finding>,
) {
    let cpg = ctx.cpg;
    if cpg.kind_of(node) != NodeKind::Call {
        return;
    }
    let name = cpg.name_of(node).unwrap_or("");
    let is_sink_here = ctx.spec.sinks.contains(name)
        && ctx.spec.sink_shape_matches(name, cpg.code_of(node).unwrap_or(""));
    let args_all = cpg.arguments_of(node);
    let shell_payload = shellform_payload_start(cpg, &args_all);
    let mut fired = false;
    if is_sink_here {
        for (k, &arg) in args_all.iter().enumerate() {
            // A position-qualified exec sink still fires on the payload of a
            // `<shell> -c` spelling: the qualifier models "tainted binary",
            // and `Command("sh", "-c", t)` moves the injection to arg 2.
            if !ctx.spec.sink_arg_matches(name, k)
                && !shell_payload.is_some_and(|s| k >= s)
            {
                continue;
            }
            if let Some(trace) = expr_taint(ctx, arg, taint) {
                let path = trace
                    .extend(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        Provenance::IntraProc,
                        0,
                    )
                    .steps;
                out.push(Finding {
                    method: method_name.to_string(),
                    sink: name.to_string(),
                    sink_line: cpg.line_of(node),
                    sink_file: cpg.path_of(cpg.file_of(node)).map(str::to_string),
                    origin: trace.origin,
                    path,
                    guard: None,
                    authz: None,
                    confined: None,
                });
                fired = true;
                break;
            }
        }
    }
    // Command-line ASSEMBLY sink (`<shellform>` in the pack's sinks): a
    // tainted payload after `<shell>, -c` in ANY call's argument list. A
    // pod-spec `Command: []string{"sh", "-c", cmd}` slice literal lowers to
    // a call whose elements are arguments, so assembling a shell script
    // from tainted data reports even when the executing side is out of
    // reach (Kubernetes runs it, not this process).
    if !fired && ctx.spec.sinks.contains("<shellform>") && name != "=" {
        if let Some(start) = shell_payload {
            for &arg in args_all.iter().skip(start) {
                if let Some(trace) = expr_taint(ctx, arg, taint) {
                    let path = trace
                        .extend(
                            cpg.code_of(node).unwrap_or(name),
                            cpg.line_of(node),
                            Provenance::IntraProc,
                            0,
                        )
                        .steps;
                    out.push(Finding {
                        method: method_name.to_string(),
                        sink: "<shellform>".to_string(),
                        sink_line: cpg.line_of(node),
                        sink_file: cpg.path_of(cpg.file_of(node)).map(str::to_string),
                        origin: trace.origin,
                        path,
                        guard: None,
                        authz: None,
                        confined: None,
                    });
                    break;
                }
            }
        }
    }
    // Interprocedural: a tainted argument handed to a resolved callee whose
    // parameter reaches a sink inside it (transitively). This is the flow a
    // param→return summary cannot express — the sink fires in the callee,
    // not in the caller. Gated on the SHAPE-matched sink test, not the bare
    // name: `client->unlink(req)` shares a name with the libc sink but is a
    // stitched RPC hop that must still hand off into its handlers.
    if !is_sink_here
        && !ctx.is_sanitizer(name)
        && !is_operator(name)
        && is_invoked(cpg, node, name)
    {
        // Every resolved target: a stitched RPC call fans out to several
        // handlers, and the sink may live in any of them. First hit wins.
        'callees: for callee in cpg.call_targets(node) {
            for (k, arg) in args_to_params(cpg, callee, &args_all) {
                if let Some(trace) = expr_taint(ctx, arg, taint) {
                    let mut visiting = HashSet::new();
                    if let Some(hit) = param_to_sink(ctx, callee, k, 1, &mut visiting) {
                        let callee_fqn = cpg.full_name_of(callee).unwrap_or(name).to_string();
                        let hop = trace.extend(
                            cpg.code_of(node).unwrap_or(name),
                            cpg.line_of(node),
                            Provenance::SummaryFlow { callee_fqn },
                            0,
                        );
                        out.push(Finding {
                            method: method_name.to_string(),
                            sink: hit.sink,
                            sink_line: hit.line,
                            sink_file: hit.file.clone(),
                            origin: trace.origin.clone(),
                            path: hop.splice(hit.steps).steps,
                            guard: None,
                            authz: None,
                            confined: None,
                        });
                        break 'callees;
                    }
                }
            }
        }
    }
    // No recursion into argument subtrees: `add_argument` links arguments
    // into the AST, so every nested call already gets its own visit from
    // `analyse_method`'s descendant walk — recursing here double-counted
    // any sink appearing as an argument of another call (JSX makes that the
    // COMMON case: the attribute sink is always an argument of its element).
}

/// A sink reached from a callee's parameter, with the internal witness steps.
struct SinkHit {
    sink: String,
    line: Option<u32>,
    file: Option<String>,
    steps: Vec<Step>,
}

/// Does `method`'s parameter `param_idx` reach a sink inside `method` (or
/// transitively inside one of its resolved callees) along a sanitizer-free
/// path? Mirrors `callee_chain`'s propagation rules, but the target is a
/// sink argument rather than the return value.
fn param_to_sink(
    ctx: &Ctx,
    method: NodeId,
    param_idx: usize,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<SinkHit> {
    if depth > MAX_SPLICE_DEPTH {
        return None;
    }
    let cpg = ctx.cpg;
    let fqn = cpg.full_name_of(method).unwrap_or("<anon>").to_string();
    if !visiting.insert(fqn.clone()) {
        return None; // recursion
    }
    let result = (|| {
        let params = cpg.parameters_of(method);
        let &pnode = params.get(param_idx)?;
        let pname = cpg.name_of(pnode)?.to_string();

        let mut chains: HashMap<String, Vec<Step>> = HashMap::new();
        chains.insert(
            pname.clone(),
            vec![Step::intra(cpg.code_of(pnode).unwrap_or(&pname), cpg.line_of(pnode), depth)],
        );
        // Intra-method alias pairs (mirrors analyse_method).
        let mut alias: HashMap<String, HashSet<String>> = HashMap::new();
        // Lazily-computed local/param names (object-state transfer gate).
        let mut local_names: Option<HashSet<String>> = None;

        let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
            .into_iter()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Call)
            .filter(|&n| cpg.line_of(n).is_some())
            .collect();
        stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

        // Framework globals are readable in callees too (shadow-aware).
        if !ctx.spec.source_idents.is_empty() {
            let shadowed = shadowed_names(ctx, method, &stmts);
            for ident in &ctx.spec.source_idents {
                if !shadowed.contains(ident) && !chains.contains_key(ident) {
                    chains.insert(
                        ident.clone(),
                        vec![Step::intra(ident, cpg.line_of(method), depth)],
                    );
                }
            }
        }

        // First sink hit in line order wins (unchanged reporting semantics),
        // but the walk CONTINUES after it: the persistence harvest blocks
        // below must see every store in the method, not just those before
        // the first hit — two rule packs with different sinks used to
        // harvest different key sets from the same module because each
        // pack's first hit truncated the walk at a different statement.
        let mut first_hit: Option<SinkHit> = None;
        for n in stmts {
            let name = cpg.name_of(n).unwrap_or("");
            // Out-parameter sources fire in callees too (mirrors analyse_method).
            if let Some(&k) = ctx.spec.out_param_sources.get(name) {
                for var in out_arg_names(cpg, n, k) {
                    let c =
                        vec![Step::intra(cpg.code_of(n).unwrap_or(name), cpg.line_of(n), depth)];
                    spread_to_aliases(&alias, &mut chains, &var, &c);
                    chains.insert(var, c);
                }
            }
            // Copy-family propagation in callees (mirrors analyse_method).
            if !ctx.is_sanitizer(name) {
                if let Some((dst, first_src)) = copy_propagation(name) {
                    let args = cpg.arguments_of(n);
                    if let Some(mut c) = args
                        .iter()
                        .skip(first_src)
                        .find_map(|&a| chain_expr(ctx, a, &chains, depth, visiting))
                    {
                        c.push(Step::intra(cpg.code_of(n).unwrap_or(name), cpg.line_of(n), depth));
                        for var in out_arg_names(cpg, n, dst) {
                            spread_to_aliases(&alias, &mut chains, &var, &c);
                            chains.insert(var, c.clone());
                        }
                    }
                }
            }
            if name == "=" {
                let args = cpg.arguments_of(n);
                if args.len() == 2 {
                    let code = cpg.code_of(n).unwrap_or("");
                    let store_key = member_store_key(code);
                    // Store-vs-rebind decided by the parsed path (mirrors
                    // analyse_method); store_key only feeds the harvest.
                    let store_path = member_store_path(code);
                    let is_member_store = store_key.is_some() || store_path.is_some();
                    // Alias bookkeeping (mirrors analyse_method).
                    if !is_member_store && !is_compound_assign(code) {
                        if let Some(l) = lhs_name(cpg, args[0]) {
                            record_alias(&mut alias, &l, rhs_ident(cpg, args[1]).as_deref());
                        }
                    }
                    match chain_expr(ctx, args[1], &chains, depth, visiting) {
                        Some(mut c) => {
                            // Persistence phase-1 (member store), mirrors
                            // analyse_method.
                            if let Some(k) = &store_key {
                                ctx.stored.borrow_mut().entry(k.clone()).or_default().insert(n);
                                // Assignment sink (mirrors analyse_method).
                                if first_hit.is_none() && ctx.spec.assign_sink_match(k) {
                                    let mut steps = c.clone();
                                    steps.push(Step::intra(code, cpg.line_of(n), depth));
                                    first_hit = Some(SinkHit {
                                        sink: format!("={k}"),
                                        line: cpg.line_of(n),
                                        file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                                        steps,
                                    });
                                }
                            }
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                c.push(Step::intra(cpg.code_of(n).unwrap_or(&lhs), cpg.line_of(n), depth));
                                match store_path.as_ref().filter(|p| {
                                    p.starts_with(&lhs)
                                        && p.as_bytes().get(lhs.len()) == Some(&b'.')
                                }) {
                                    // Field-sensitive member store (mirrors
                                    // analyse_method).
                                    Some(path) => {
                                        let suffix = path[lhs.len()..].to_string();
                                        spread_field_to_aliases(
                                            &alias, &mut chains, &lhs, &suffix, &c,
                                        );
                                        chains.insert(path.clone(), c);
                                    }
                                    None if is_member_store => {
                                        spread_to_aliases(&alias, &mut chains, &lhs, &c);
                                        chains.insert(lhs, c);
                                    }
                                    None => {
                                        remove_subtree(&mut chains, &lhs);
                                        chains.insert(lhs, c);
                                    }
                                }
                            }
                        }
                        None => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                if !is_compound_assign(code) {
                                    match &store_path {
                                        Some(path) => remove_subtree(&mut chains, path),
                                        None if is_member_store => {
                                            chains.remove(&lhs);
                                        }
                                        None => remove_subtree(&mut chains, &lhs),
                                    }
                                }
                            }
                        }
                    }
                }
                continue;
            }
            // Persistence phase-1 harvest (mirrors analyse_method).
            if (name.starts_with("set") || name.starts_with("Set")) && name.len() > 3 {
                let hit = cpg
                    .arguments_of(n)
                    .into_iter()
                    .find_map(|a| chain_expr(ctx, a, &chains, depth, visiting));
                if let Some(mut c) = hit {
                    let k = &name[3..];
                    ctx.stored.borrow_mut().entry(k.to_string()).or_default().insert(n);
                    // Assignment sink: setter form (mirrors analyse_method).
                    if first_hit.is_none() && ctx.spec.assign_sink_match(k) {
                        c.push(Step::intra(cpg.code_of(n).unwrap_or(name), cpg.line_of(n), depth));
                        first_hit = Some(SinkHit {
                            sink: format!("={k}"),
                            line: cpg.line_of(n),
                            file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                            steps: c,
                        });
                    }
                }
            }
            // Persistence phase-1 (named-arg store), mirrors analyse_method.
            if !is_operator(name) {
                for a in cpg.arguments_of(n) {
                    if cpg.kind_of(a) != NodeKind::Call || cpg.name_of(a) != Some("=") {
                        continue;
                    }
                    let aa = cpg.arguments_of(a);
                    if aa.len() == 2 && cpg.kind_of(aa[0]) == NodeKind::Identifier {
                        if let Some(mut c) = chain_expr(ctx, aa[1], &chains, depth, visiting) {
                            if let Some(k) = cpg.name_of(aa[0]) {
                                ctx.stored
                                    .borrow_mut()
                                    .entry(k.to_string())
                                    .or_default()
                                    .insert(n);
                                // Assignment sink: named-argument form
                                // (mirrors analyse_method).
                                if first_hit.is_none() && ctx.spec.assign_sink_match(k) {
                                    c.push(Step::intra(
                                        cpg.code_of(n).unwrap_or(name),
                                        cpg.line_of(n),
                                        depth,
                                    ));
                                    first_hit = Some(SinkHit {
                                        sink: format!("={k}"),
                                        line: cpg.line_of(n),
                                        file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                                        steps: c,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            // Object-state transfer + receiver-sinks (mirrors analyse_method).
            if !ctx.is_sanitizer(name) && !is_operator(name) {
                if ctx.spec.recv_sinks.contains(name) {
                    let recv_chain = cpg
                        .signature_of(n)
                        .and_then(|r| lookup_contained(&chains, r).cloned())
                        .or_else(|| {
                            cpg.out_kind(n, EdgeKind::Receiver)
                                .next()
                                .and_then(|r| chain_expr(ctx, r, &chains, depth, visiting))
                        });
                    if let Some(mut c) = recv_chain {
                        if first_hit.is_none() {
                            c.push(Step::intra(
                                cpg.code_of(n).unwrap_or(name),
                                cpg.line_of(n),
                                depth,
                            ));
                            first_hit = Some(SinkHit {
                                sink: name.to_string(),
                                line: cpg.line_of(n),
                                file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                                steps: c,
                            });
                        }
                    }
                }
                if let Some(recv) = cpg
                    .signature_of(n)
                    .filter(|r| *r != "<literal>")
                    .filter(|_| !is_query_call(name))
                    .map(str::to_string)
                {
                    // Only real locals carry object state; package/namespace
                    // roots are stateless (see [`method_local_names`]).
                    if !chains.contains_key(&recv)
                        && local_names
                            .get_or_insert_with(|| method_local_names(cpg, method))
                            .contains(&recv)
                    {
                        let hit = cpg
                            .arguments_of(n)
                            .into_iter()
                            .find_map(|a| chain_expr(ctx, a, &chains, depth, visiting));
                        if let Some(mut c) = hit {
                            c.push(Step::intra(cpg.code_of(n).unwrap_or(name), cpg.line_of(n), depth));
                            spread_to_aliases(&alias, &mut chains, &recv, &c);
                            chains.insert(recv, c);
                        }
                    }
                }
            }
            if ctx.spec.sinks.contains(name)
                && ctx.spec.sink_shape_matches(name, cpg.code_of(n).unwrap_or(""))
            {
                for (k, arg) in cpg.arguments_of(n).into_iter().enumerate() {
                    if !ctx.spec.sink_arg_matches(name, k) {
                        continue;
                    }
                    if let Some(mut c) = chain_expr(ctx, arg, &chains, depth, visiting) {
                        if first_hit.is_none() {
                            c.push(Step::intra(
                                cpg.code_of(n).unwrap_or(name),
                                cpg.line_of(n),
                                depth,
                            ));
                            first_hit = Some(SinkHit {
                                sink: name.to_string(),
                                line: cpg.line_of(n),
                                file: cpg.path_of(cpg.file_of(n)).map(str::to_string),
                                steps: c,
                            });
                        }
                    }
                }
                continue;
            }
            if ctx.is_sanitizer(name) || is_operator(name) {
                continue;
            }
            // Hand-off to deeper resolved callees (all stitched targets).
            // Still recursed after a hit: the callee walk harvests stores
            // even when its result is no longer needed for reporting.
            if !is_invoked(cpg, n, name) {
                continue;
            }
            for callee in cpg.call_targets(n) {
                for (k, arg) in args_to_params(cpg, callee, &cpg.arguments_of(n)) {
                    if let Some(mut c) = chain_expr(ctx, arg, &chains, depth, visiting) {
                        if let Some(hit) = param_to_sink(ctx, callee, k, depth + 1, visiting) {
                            if first_hit.is_none() {
                                let callee_fqn =
                                    cpg.full_name_of(callee).unwrap_or(name).to_string();
                                c.push(Step {
                                    code: cpg.code_of(n).unwrap_or(name).to_string(),
                                    line: cpg.line_of(n),
                                    provenance: Provenance::SummaryFlow { callee_fqn },
                                    depth,
                                });
                                c.extend(hit.steps);
                                first_hit = Some(SinkHit {
                                    sink: hit.sink,
                                    line: hit.line,
                                    file: hit.file,
                                    steps: c,
                                });
                            }
                        }
                    }
                }
            }
        }
        first_hit
    })();
    visiting.remove(&fqn);
    result
}

/// A format string held in a local (`queryFormat := "<literal>"` then
/// `Sprintf(queryFormat, ..)`) is still a literal format when the local has
/// exactly ONE assignment in its enclosing method and that assignment's rhs
/// is a literal — or, one hop deeper, another such single-assignment local.
/// Anything else disqualifies: a second write, a compound write (`+=`), a
/// member store into it, a same-named parameter (caller-controlled), or a
/// non-literal rhs. `None` sends the caller back to the conservative
/// any-arg rule, so a wrong bail costs precision, never soundness.
fn resolve_format_literal(cpg: &Cpg, fmt_node: NodeId, depth: u32) -> Option<NodeId> {
    if depth > 3 {
        return None;
    }
    let name = cpg.name_of(fmt_node)?;
    // Enclosing method: bounded AST-parent walk (defensive, same shape as
    // the persistence ubiquity filter's).
    let mut m = fmt_node;
    for _ in 0..512 {
        if cpg.kind_of(m) == NodeKind::Method {
            break;
        }
        m = cpg.in_kind(m, EdgeKind::Ast).next()?;
    }
    if cpg.kind_of(m) != NodeKind::Method {
        return None;
    }
    if cpg.parameters_of(m).iter().any(|&p| cpg.name_of(p) == Some(name)) {
        return None;
    }
    // One pass over the method subtree for `=` calls whose lhs is `name`.
    let mut rhs_found: Option<NodeId> = None;
    let mut stack = vec![m];
    let mut visited = 0usize;
    while let Some(n) = stack.pop() {
        visited += 1;
        if visited > 50_000 {
            return None; // pathological method — stay conservative
        }
        if cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some("=") {
            let args = cpg.arguments_of(n);
            if args.first().is_some_and(|&l| {
                cpg.kind_of(l) == NodeKind::Identifier && cpg.name_of(l) == Some(name)
            }) {
                let code = cpg.code_of(n).unwrap_or("");
                if rhs_found.is_some()
                    || is_compound_assign(code)
                    || member_store_key(code).is_some()
                {
                    return None; // not single-assignment
                }
                rhs_found = Some(*args.get(1)?);
            }
        }
        stack.extend(cpg.out_kind(n, EdgeKind::Ast));
    }
    let rhs = rhs_found?;
    match cpg.kind_of(rhs) {
        NodeKind::Literal => Some(rhs),
        NodeKind::Identifier => resolve_format_literal(cpg, rhs, depth + 1),
        _ => None,
    }
}

/// For a Sprintf-family call whose format string is a LITERAL, the argument
/// indices whose verbs can carry string taint: `%s`/`%v`/`%q`/`%U` do,
/// `%d`/`%x`/`%f`/`%t`/... cannot (an integer cannot smuggle SQL or shell).
/// The literal may sit at the callsite or be traced through
/// single-assignment locals (`resolve_format_literal`). `None` = filter not
/// applicable (different callee, or unresolvable format — stay
/// conservative). Width `*` consumes an argument as a non-carrier.
fn format_verb_carriers(cpg: &Cpg, name: &str, args: &[NodeId]) -> Option<Vec<usize>> {
    if !matches!(name, "Sprintf" | "Errorf" | "Wrapf" | "Newf") {
        return None;
    }
    let &fmt_arg = args.first()?;
    let fmt_node = match cpg.kind_of(fmt_arg) {
        NodeKind::Literal => fmt_arg,
        NodeKind::Identifier => resolve_format_literal(cpg, fmt_arg, 0)?,
        _ => return None,
    };
    let fmt = cpg.code_of(fmt_node)?.trim().trim_matches(['"', '`', '\'']);
    let mut carriers = Vec::new();
    let mut arg_idx = 1usize; // args[0] is the format string
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            continue;
        }
        // flags / width / precision — a `*` consumes an arg (non-carrier).
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() || matches!(d, '+' | '-' | '#' | ' ' | '0' | '.') {
                chars.next();
            } else if d == '*' {
                chars.next();
                arg_idx += 1;
            } else {
                break;
            }
        }
        let Some(verb) = chars.next() else { break };
        if matches!(verb, 's' | 'v' | 'q' | 'U') {
            carriers.push(arg_idx);
        }
        arg_idx += 1;
    }
    Some(carriers)
}

/// Returns `Some(trace)` describing provenance if the expression is tainted.
fn expr_taint(ctx: &Ctx, node: NodeId, taint: &HashMap<String, Trace>) -> Option<Trace> {
    let cpg = ctx.cpg;
    match cpg.kind_of(node) {
        // A bare identifier is tainted whole-object, or by CONTAINMENT when
        // one of its fields is (the value flowing here carries the field).
        NodeKind::Identifier => cpg.name_of(node).and_then(|n| lookup_contained(taint, n).cloned()),
        NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            if ctx.is_sanitizer(name) {
                // A sanitizer's result never carries its arguments' taint.
                return None;
            }
            if call_is_source(ctx, node, name) {
                return Some(Trace {
                    origin: name.to_string(),
                    steps: vec![Step::intra(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        0,
                    )],
                });
            }
            // An @out source is an explicit model: data goes INTO arg k
            // (handled at statement level); the return is a count/status,
            // so the conservative arg pass-through below must not fire.
            if ctx.spec.out_param_sources.contains_key(name) {
                return None;
            }
            // Member READ (a no-paren Call chain over a plain identifier —
            // `cfg.sub.key`): FIELD-SENSITIVE selection. Whole-object taint
            // on any prefix still taints the read; otherwise the selected
            // path must hit a member-store key. Chains through anything
            // else (real calls, subscripts) keep the generic rules below.
            if let Some((root, fields)) = member_read_path(cpg, node) {
                if let Some(t) = read_path_taint(taint, &root, &fields) {
                    return Some(t.extend(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        Provenance::IntraProc,
                        0,
                    ));
                }
                // A dead path may still ORIGINATE: an inner link can be a
                // source-named read (persisted getters ride member chains —
                // `cfg.executionUser.get`). Only origination is checked here;
                // descending generically would resurrect the sibling-field
                // FP this branch exists to kill.
                let mut cur = node;
                while cpg.kind_of(cur) == NodeKind::Call {
                    let Some(&base) = cpg.arguments_of(cur).first() else { break };
                    if cpg.kind_of(base) == NodeKind::Call {
                        if let Some(nm) = cpg.name_of(base) {
                            if call_is_source(ctx, base, nm) {
                                let t = Trace {
                                    origin: nm.to_string(),
                                    steps: vec![Step::intra(
                                        cpg.code_of(base).unwrap_or(nm),
                                        cpg.line_of(base),
                                        0,
                                    )],
                                };
                                return Some(t.extend(
                                    cpg.code_of(node).unwrap_or(name),
                                    cpg.line_of(node),
                                    Provenance::IntraProc,
                                    0,
                                ));
                            }
                        }
                    }
                    cur = base;
                }
                return None;
            }
            // Sprintf-family with a LITERAL format: only arguments landing
            // in string verbs (%s/%v/%q) carry taint — an int64 through %d
            // cannot smuggle SQL. Non-literal formats fall through to the
            // conservative any-arg rule.
            if let Some(carriers) = format_verb_carriers(cpg, name, &args) {
                return carriers
                    .into_iter()
                    .filter_map(|i| args.get(i).copied())
                    .find_map(|a| expr_taint(ctx, a, taint))
                    .map(|t| {
                        t.extend(
                            cpg.code_of(node).unwrap_or(name),
                            cpg.line_of(node),
                            Provenance::IntraProc,
                            0,
                        )
                    });
            }
            if is_operator(name) {
                // Operators taint their result if any operand is tainted.
                for a in &args {
                    if let Some(t) = expr_taint(ctx, *a, taint) {
                        return Some(t);
                    }
                }
                return None;
            }
            // Named callee: result is tainted iff a tainted argument flows to
            // the return RAW per the callee's summary (sanitized flows are
            // not lifted). Splice the callee's internal chain when we have
            // its body; external summaries are recorded summary-only.
            // Computed summaries are keyed by the callee's FULL name — try
            // the resolved target(s) first, then the simple name (externals).
            let key = cpg
                .call_targets(node)
                .into_iter()
                .filter_map(|m| cpg.full_name_of(m))
                .find(|f| ctx.summaries.get(f).is_some())
                .unwrap_or(name);
            // Receiver pass-through applies with or without a summary,
            // UNLESS the summary positively models the receiver and shows
            // no raw Recv→Return flow (Point::Recv, external entries only).
            // Absence of a summary — or of receiver modeling in one — can
            // never rule out `tainted.method()` producing a tainted result
            // (`path.c_str()`, `request.args`). The receiver's variable
            // name was stamped into signature.
            let recv_taint = |cpg: &Cpg| {
                let t = cpg
                    .signature_of(node)
                    .and_then(|recv| lookup_contained(taint, recv).cloned())
                    .or_else(|| {
                        // Fluent chain: the receiver is an expression
                        // (`new Builder(evil).build()`), not a variable.
                        cpg.out_kind(node, EdgeKind::Receiver)
                            .next()
                            .and_then(|r| expr_taint(ctx, r, taint))
                    })?;
                Some(t.extend(
                    cpg.code_of(node).unwrap_or(name),
                    cpg.line_of(node),
                    Provenance::IntraProc,
                    0,
                ))
            };
            // A member call with a receiver but NO type evidence is dynamic
            // dispatch (Python/JS, or an untyped chain): a summary matched
            // by bare name belongs to some unrelated same-named method —
            // `"{}".format(..)` must not bind to a log Formatter's `format`.
            let untrusted_dispatch =
                cpg.signature_of(node).is_some() && cpg.type_full_name_of(node).is_none();
            let looked_up =
                if untrusted_dispatch { None } else { ctx.summaries.get_with_origin(key) };
            let Some((summary, origin)) = looked_up else {
                // No (trustworthy) summary: an opaque call. Conservative
                // pass-through — its result carries the receiver's taint, or
                // any argument's (`"{}/{}".format(tainted, ..)`). IRIS-style
                // over-approximation; triage owns the precision.
                if let Some(t) = recv_taint(cpg) {
                    return Some(t);
                }
                return args.iter().find_map(|&a| expr_taint(ctx, a, taint)).map(|t| {
                    t.extend(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        Provenance::IntraProc,
                        0,
                    )
                });
            };
            let fqn = summary.fqn.clone();
            // Deterministic witness choice: flows is a HashSet and the FIRST
            // param whose argument is tainted wins — iterate in index order
            // or the byte-identical-rerun contract breaks (seen live: same
            // finding, witness flipping between two tainted args across
            // runs). Same discipline as the sorted call-returns below.
            let mut ks: Vec<usize> = summary.flows_to_return().collect();
            ks.sort_unstable();
            for k in ks {
                if let Some(a) = arg_for_param(cpg, node, &fqn, &args, k) {
                    if let Some(t) = expr_taint(ctx, a, taint) {
                        let mut visiting = HashSet::new();
                        match lift(ctx, name, &fqn, origin, k, &mut visiting) {
                            Some((inner, prov)) => {
                                return Some(t.splice(inner).extend(
                                    cpg.code_of(node).unwrap_or(name),
                                    cpg.line_of(node),
                                    prov,
                                    0,
                                ));
                            }
                            // The callee's only internal path for this flow
                            // goes through a sanitizer: not liftable.
                            None => continue,
                        }
                    }
                }
            }
            // Returns-tainted: the callee manufactures data from a source
            // call inside its own body (`fn f() { return getenv(..) }`) —
            // no param→return flow exists, so the loop above cannot see it.
            // Match the summary's raw call-returns against the spec sources
            // and originate taint AT THIS CALL SITE. Persisted phase-2
            // getter names are excluded: their read-site shape gates
            // (persisted_read_shape_ok) inspect the read call's code, which
            // a buried call cannot be checked against here.
            let mut srcs: Vec<&str> = summary
                .raw_call_returns()
                .filter(|s| ctx.spec.sources.contains(*s))
                .filter(|s| !ctx.spec.persisted_sources.contains(*s))
                .collect();
            srcs.sort(); // set order is arbitrary; the winning origin must not be
            for src in srcs {
                let mut visiting = HashSet::new();
                if let Some((inner, prov)) =
                    lift_source(ctx, name, &fqn, origin, src, &mut visiting)
                {
                    let t = Trace { origin: src.to_string(), steps: inner };
                    return Some(t.extend(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        prov,
                        0,
                    ));
                }
            }
            if summary.receiver_passes_through() { recv_taint(cpg) } else { None }
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(t) = expr_taint(ctx, c, taint) {
                    return Some(t);
                }
            }
            None
        }
    }
}

/// Lift a raw summary flow (param `k` → return of `name`): decide the hop's
/// provenance and reconstruct the callee's internal witness steps.
///
/// Returns `None` when the callee body shows every param-k→return path goes
/// through a sanitizer (the flow must not be lifted); `Some((steps, prov))`
/// otherwise, where `steps` is empty for external/unlocatable/recursive
/// callees (summary-only hop).
fn lift(
    ctx: &Ctx,
    name: &str,
    fqn: &str,
    origin: SummaryOrigin,
    k: usize,
    visiting: &mut HashSet<String>,
) -> Option<(Vec<Step>, Provenance)> {
    match origin {
        SummaryOrigin::External => Some((
            Vec::new(),
            Provenance::ExternalSummary { callee_fqn: fqn.to_string() },
        )),
        SummaryOrigin::Computed => {
            let prov = Provenance::SummaryFlow { callee_fqn: fqn.to_string() };
            let Some(body) = ctx.body_of(name, fqn) else {
                return Some((Vec::new(), prov)); // no body located: summary-only hop
            };
            if !visiting.insert(fqn.to_string()) {
                return Some((Vec::new(), prov)); // recursion: don't re-expand
            }
            let chain = callee_chain(ctx, body, k, 1, visiting);
            visiting.remove(fqn);
            chain.map(|steps| (steps, prov))
        }
    }
}

/// Reconstruct the intraprocedural witness chain inside `method` that carries
/// its parameter `param_idx` to its return, honouring sanitizers. Steps are
/// marked with `depth`. Returns `None` when no sanitizer-free path exists
/// (i.e. the raw summary flow is only realisable through a sanitizer per the
/// query's sanitizer set); `Some(steps)` otherwise.
fn callee_chain(
    ctx: &Ctx,
    method: NodeId,
    param_idx: usize,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    if depth > MAX_SPLICE_DEPTH {
        return Some(Vec::new()); // too deep: keep the hop, drop the expansion
    }
    let cpg = ctx.cpg;
    let params = cpg.parameters_of(method);
    let Some(&pnode) = params.get(param_idx) else {
        return Some(Vec::new()); // signature mismatch: summary-only hop
    };
    let pname = cpg.name_of(pnode)?.to_string();

    // var name -> witness chain from the parameter to that var.
    let mut chains: HashMap<String, Vec<Step>> = HashMap::new();
    chains.insert(
        pname.clone(),
        vec![Step::intra(cpg.code_of(pnode).unwrap_or(&pname), cpg.line_of(pnode), depth)],
    );

    let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return))
        .filter(|&n| cpg.line_of(n).is_some())
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    // First tainted return in line order wins (unchanged semantics), but the
    // walk continues afterwards so the member-store harvest sees stores
    // BELOW an early `return tainted` too (same completeness rule as
    // param_to_sink's first-sink-hit).
    let mut found: Option<Vec<Step>> = None;
    for n in stmts {
        match cpg.kind_of(n) {
            NodeKind::Call if cpg.name_of(n) == Some("=") => {
                let args = cpg.arguments_of(n);
                if args.len() == 2 {
                    match chain_expr(ctx, args[1], &chains, depth, visiting) {
                        Some(mut c) => {
                            // Persistence phase-1 (member store), mirrors
                            // analyse_method.
                            if let Some(k) = member_store_key(cpg.code_of(n).unwrap_or("")) {
                                ctx.stored.borrow_mut().entry(k).or_default().insert(n);
                            }
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                c.push(Step::intra(
                                    cpg.code_of(n).unwrap_or(&lhs),
                                    cpg.line_of(n),
                                    depth,
                                ));
                                chains.insert(lhs, c);
                            }
                        }
                        None => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                if !is_compound_assign(cpg.code_of(n).unwrap_or("")) {
                                    chains.remove(&lhs); // reassignment clears
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::Return if found.is_none() => {
                for c in cpg.out_kind(n, cpg_core::EdgeKind::Ast) {
                    if let Some(mut chain) = chain_expr(ctx, c, &chains, depth, visiting) {
                        chain.push(Step::intra(
                            cpg.code_of(n).unwrap_or("return"),
                            cpg.line_of(n),
                            depth,
                        ));
                        found = Some(chain);
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    found // None = no sanitizer-free param -> return path found
}

/// The witness chain carrying the tracked parameter into `node`, if any.
/// Mirrors the summary walker's propagation rules (identifiers, literals,
/// operators, raw callee flows, wrapper nodes) with sanitizers killing.
fn chain_expr(
    ctx: &Ctx,
    node: NodeId,
    chains: &HashMap<String, Vec<Step>>,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    let cpg = ctx.cpg;
    match cpg.kind_of(node) {
        // Containment lookup mirrors expr_taint's Identifier arm.
        NodeKind::Identifier => {
            cpg.name_of(node).and_then(|n| lookup_contained(chains, n).cloned())
        }
        NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            if ctx.is_sanitizer(name) {
                return None; // sanitized inside the callee: path dies here
            }
            if is_operator(name) {
                for a in &args {
                    if let Some(c) = chain_expr(ctx, *a, chains, depth, visiting) {
                        return Some(c);
                    }
                }
                return None;
            }
            // @out sources: return is a count/status, never data (mirrors
            // expr_taint).
            if ctx.spec.out_param_sources.contains_key(name) {
                return None;
            }
            // Member READ: field-sensitive selection (mirrors expr_taint).
            if let Some((root, fields)) = member_read_path(cpg, node) {
                return read_path_taint(chains, &root, &fields).map(|c| {
                    let mut c = c.clone();
                    c.push(Step::intra(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        depth,
                    ));
                    c
                });
            }
            // Format-verb filter (mirrors expr_taint).
            if let Some(carriers) = format_verb_carriers(cpg, name, &args) {
                for i in carriers {
                    if let Some(&a) = args.get(i) {
                        if let Some(mut c) = chain_expr(ctx, a, chains, depth, visiting) {
                            c.push(Step::intra(
                                cpg.code_of(node).unwrap_or(name),
                                cpg.line_of(node),
                                depth,
                            ));
                            return Some(c);
                        }
                    }
                }
                return None;
            }
            let key = cpg
                .call_targets(node)
                .into_iter()
                .filter_map(|m| cpg.full_name_of(m))
                .find(|f| ctx.summaries.get(f).is_some())
                .unwrap_or(name);
            // Mirrors expr_taint: receiver pass-through with or without a
            // summary — unless the summary positively models the receiver
            // and shows no raw Recv→Return flow; argument pass-through only
            // when no summary exists.
            let recv_chain = |chains: &HashMap<String, Vec<Step>>,
                              visiting: &mut HashSet<String>| {
                let mut c = cpg
                    .signature_of(node)
                    .and_then(|recv| lookup_contained(chains, recv).cloned())
                    .or_else(|| {
                        cpg.out_kind(node, EdgeKind::Receiver)
                            .next()
                            .and_then(|r| chain_expr(ctx, r, chains, depth, visiting))
                    })?;
                c.push(Step::intra(cpg.code_of(node).unwrap_or(name), cpg.line_of(node), depth));
                Some(c)
            };
            let untrusted_dispatch =
                cpg.signature_of(node).is_some() && cpg.type_full_name_of(node).is_none();
            let looked_up =
                if untrusted_dispatch { None } else { ctx.summaries.get_with_origin(key) };
            let Some((summary, origin)) = looked_up else {
                if let Some(c) = recv_chain(chains, visiting) {
                    return Some(c);
                }
                for a in &args {
                    if let Some(mut c) = chain_expr(ctx, *a, chains, depth, visiting) {
                        c.push(Step::intra(
                            cpg.code_of(node).unwrap_or(name),
                            cpg.line_of(node),
                            depth,
                        ));
                        return Some(c);
                    }
                }
                return None;
            };
            let fqn = summary.fqn.clone();
            // Sorted for deterministic witness choice — see expr_taint.
            let mut ks: Vec<usize> = summary.flows_to_return().collect();
            ks.sort_unstable();
            for k in ks {
                if let Some(a) = arg_for_param(cpg, node, &fqn, &args, k) {
                    if let Some(mut c) = chain_expr(ctx, a, chains, depth, visiting) {
                        match lift_nested(ctx, name, &fqn, origin, k, depth, visiting) {
                            Some((inner, prov)) => {
                                c.extend(inner);
                                c.push(Step {
                                    code: cpg.code_of(node).unwrap_or(name).to_string(),
                                    line: cpg.line_of(node),
                                    provenance: prov,
                                    depth,
                                });
                                return Some(c);
                            }
                            None => continue, // that flow is sanitized inside
                        }
                    }
                }
            }
            if summary.receiver_passes_through() {
                recv_chain(chains, visiting)
            } else {
                None
            }
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(chain) = chain_expr(ctx, c, chains, depth, visiting) {
                    return Some(chain);
                }
            }
            None
        }
    }
}

/// `lift` for hops encountered *inside* a spliced callee: identical policy,
/// but internal steps land one level deeper than the current chain.
fn lift_nested(
    ctx: &Ctx,
    name: &str,
    fqn: &str,
    origin: SummaryOrigin,
    k: usize,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<(Vec<Step>, Provenance)> {
    match origin {
        SummaryOrigin::External => Some((
            Vec::new(),
            Provenance::ExternalSummary { callee_fqn: fqn.to_string() },
        )),
        SummaryOrigin::Computed => {
            let prov = Provenance::SummaryFlow { callee_fqn: fqn.to_string() };
            let Some(body) = ctx.body_of(name, fqn) else {
                return Some((Vec::new(), prov));
            };
            if !visiting.insert(fqn.to_string()) {
                return Some((Vec::new(), prov));
            }
            let chain = callee_chain(ctx, body, k, depth + 1, visiting);
            visiting.remove(fqn);
            chain.map(|steps| (steps, prov))
        }
    }
}

/// `lift` for a returns-tainted flow (source call `src` → return of `name`):
/// decide the hop's provenance and reconstruct the callee's internal witness
/// from the source call to its return. Returns `None` when every occurrence
/// of the source inside the body is laundered per the QUERY's sanitizer set
/// (which may be larger than the one the summary was computed with).
fn lift_source(
    ctx: &Ctx,
    name: &str,
    fqn: &str,
    origin: SummaryOrigin,
    src: &str,
    visiting: &mut HashSet<String>,
) -> Option<(Vec<Step>, Provenance)> {
    match origin {
        SummaryOrigin::External => Some((
            Vec::new(),
            Provenance::ExternalSummary { callee_fqn: fqn.to_string() },
        )),
        SummaryOrigin::Computed => {
            let prov = Provenance::SummaryFlow { callee_fqn: fqn.to_string() };
            let Some(body) = ctx.body_of(name, fqn) else {
                return Some((Vec::new(), prov)); // no body located: summary-only hop
            };
            if !visiting.insert(fqn.to_string()) {
                return Some((Vec::new(), prov)); // recursion: don't re-expand
            }
            let chain = source_chain(ctx, body, src, 1, visiting);
            visiting.remove(fqn);
            chain.map(|steps| (steps, prov))
        }
    }
}

/// Reconstruct the intraprocedural witness chain inside `method` from a call
/// to `src` (directly, or through a callee that itself returns `src`'s
/// result) to the method's return, honouring the query's sanitizers. The
/// returns-tainted counterpart of [`callee_chain`]: `None` means no
/// sanitizer-free source→return path exists.
fn source_chain(
    ctx: &Ctx,
    method: NodeId,
    src: &str,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    if depth > MAX_SPLICE_DEPTH {
        return Some(Vec::new()); // too deep: keep the hop, drop the expansion
    }
    let cpg = ctx.cpg;

    // var name -> witness chain from the source call to that var.
    let mut chains: HashMap<String, Vec<Step>> = HashMap::new();

    let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return))
        .filter(|&n| cpg.line_of(n).is_some())
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    let mut found: Option<Vec<Step>> = None;
    for n in stmts {
        match cpg.kind_of(n) {
            NodeKind::Call if cpg.name_of(n) == Some("=") => {
                let args = cpg.arguments_of(n);
                if args.len() == 2 {
                    // A fresh source occurrence in the rhs seeds a chain;
                    // otherwise an already-seeded chain propagates exactly
                    // as in callee_chain.
                    let c = source_expr(ctx, args[1], src, depth, visiting)
                        .or_else(|| chain_expr(ctx, args[1], &chains, depth, visiting));
                    match c {
                        Some(mut c) => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                c.push(Step::intra(
                                    cpg.code_of(n).unwrap_or(&lhs),
                                    cpg.line_of(n),
                                    depth,
                                ));
                                chains.insert(lhs, c);
                            }
                        }
                        None => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                if !is_compound_assign(cpg.code_of(n).unwrap_or("")) {
                                    chains.remove(&lhs); // reassignment clears
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::Return if found.is_none() => {
                for c in cpg.out_kind(n, cpg_core::EdgeKind::Ast) {
                    let chain = source_expr(ctx, c, src, depth, visiting)
                        .or_else(|| chain_expr(ctx, c, &chains, depth, visiting));
                    if let Some(mut chain) = chain {
                        chain.push(Step::intra(
                            cpg.code_of(n).unwrap_or("return"),
                            cpg.line_of(n),
                            depth,
                        ));
                        found = Some(chain);
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    found // None = no sanitizer-free source -> return path found
}

/// The witness chain to a fresh occurrence of source call `src` inside an
/// expression, if any: the source call itself, a call whose summary says it
/// returns `src`'s result (expanded one level deeper), or either buried in
/// arguments/receiver/wrapper nodes — with sanitizer calls killing the path.
fn source_expr(
    ctx: &Ctx,
    node: NodeId,
    src: &str,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    let cpg = ctx.cpg;
    match cpg.kind_of(node) {
        NodeKind::Identifier | NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            if ctx.is_sanitizer(name) {
                return None; // laundered here: this occurrence doesn't count
            }
            if name == src {
                return Some(vec![Step::intra(
                    cpg.code_of(node).unwrap_or(name),
                    cpg.line_of(node),
                    depth,
                )]);
            }
            // Direct occurrence somewhere in the arguments or receiver.
            for a in cpg
                .arguments_of(node)
                .into_iter()
                .chain(cpg.out_kind(node, EdgeKind::Receiver))
            {
                if let Some(mut c) = source_expr(ctx, a, src, depth, visiting) {
                    c.push(Step::intra(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        depth,
                    ));
                    return Some(c);
                }
            }
            // Transitive: this callee's own summary returns src's result.
            if is_operator(name) {
                return None;
            }
            let key = cpg
                .call_targets(node)
                .into_iter()
                .filter_map(|m| cpg.full_name_of(m))
                .find(|f| ctx.summaries.get(f).is_some())
                .unwrap_or(name);
            let untrusted_dispatch =
                cpg.signature_of(node).is_some() && cpg.type_full_name_of(node).is_none();
            if untrusted_dispatch {
                return None;
            }
            let (summary, origin) = ctx.summaries.get_with_origin(key)?;
            if !summary.raw_call_returns().any(|s| s == src) {
                return None;
            }
            let fqn = summary.fqn.clone();
            let (inner, prov) = match origin {
                SummaryOrigin::External => (
                    Vec::new(),
                    Provenance::ExternalSummary { callee_fqn: fqn.clone() },
                ),
                SummaryOrigin::Computed => {
                    let prov = Provenance::SummaryFlow { callee_fqn: fqn.clone() };
                    match ctx.body_of(name, &fqn) {
                        Some(body) if visiting.insert(fqn.clone()) => {
                            let chain = source_chain(ctx, body, src, depth + 1, visiting);
                            visiting.remove(&fqn);
                            (chain?, prov)
                        }
                        _ => (Vec::new(), prov), // unlocatable/recursive: summary-only
                    }
                }
            };
            let mut c = inner;
            c.push(Step {
                code: cpg.code_of(node).unwrap_or(name).to_string(),
                line: cpg.line_of(node),
                provenance: prov,
                depth,
            });
            Some(c)
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(chain) = source_expr(ctx, c, src, depth, visiting) {
                    return Some(chain);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_frontend::Frontend;

    /// Build a CPG from C sources and run the standard pass pipeline —
    /// a minimal stand-in for the incremental driver, local to these tests.
    fn build(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_c::CFrontend::new();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    fn summarise(cpg: &Cpg) -> SummaryStore {
        let mut store = SummaryStore::new();
        store.compute_all(cpg);
        store
    }

    /// Like `build`, for Scala fixtures — the named-arg / field-read store
    /// shapes under test have no C spelling.
    fn build_scala(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_ts::TsFrontend::scala();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    /// Like `build`, for Go fixtures — composite-literal stores
    /// (`Endpoint{Url: u}`) have no C spelling.
    fn build_go(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_ts::TsFrontend::go();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    /// Like `build`, for TSX fixtures — the tsx dialect grammar, the JSX
    /// attribute lowering, and the JS object-literal lowering are under test.
    fn build_tsx(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_ts::TsFrontend::typescript();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    /// Like `build`, for Python fixtures — source_idents (`request`) and the
    /// f-string concat lowering have no C spelling.
    fn build_python(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_ts::TsFrontend::python();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    /// The shared shape of every persistence test: no finding with the
    /// stitch off, exactly one with it on. CPG_PERSIST is process-global and
    /// tests run in parallel, so BOTH assertions must sit under one lock —
    /// otherwise another test's phase-2 window turns "off by default" flaky.
    /// One lock for every test that toggles the process-global CPG_PERSIST /
    /// CPG_PERSIST_UBIQ env vars.
    static PERSIST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn assert_persist_stitch(cpg: &Cpg, spec: &TaintSpec, origin: &str, method: &str) {
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = summarise(cpg);
        assert_eq!(find_flows(cpg, &store, spec).len(), 0, "off by default");
        std::env::set_var("CPG_PERSIST", "1");
        let findings = find_flows(cpg, &store, spec);
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].origin, origin);
        assert!(findings[0].method.contains(method), "{findings:?}");
    }

    #[test]
    fn out_param_source_taints_buffer_not_return() {
        // `read(fd, buf, n)` writes attacker bytes into buf: buf must be
        // tainted (finding), while the return count `nr` must NOT be — and a
        // different buffer never touched by read stays clean.
        let cpg = build(&[(
            "o.c",
            "void h(int fd) {\n    char buf[64];\n    char other[64];\n    int nr = read(fd, buf, 64);\n    system(other);\n    system(buf);\n    printf(nr);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["system", "printf"]);
        assert!(spec.sources.is_empty(), "@out source must not be a return source");
        assert_eq!(spec.out_param_sources.get("read"), Some(&1));
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "only system(buf) may report: {findings:?}");
        assert_eq!(findings[0].sink, "system");
        assert!(findings[0].path.last().unwrap().code.contains("system(buf)"));
        assert_eq!(findings[0].origin, "read");
    }

    #[test]
    fn assign_sink_fires_on_member_store_not_local_rebind() {
        // `r->account = t` stores attacker data into an identity-named
        // field — the `=account` assignment sink fires. A plain local
        // rebind (`account = t`) and an unrelated field (`r->color = t`)
        // must NOT fire.
        let cpg = build(&[(
            "a.c",
            "void h() {\n    char* t = getenv(\"X\");\n    char* account = t;\n    r->color = t;\n    r->account = t;\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["=account"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "=account");
        assert_eq!(findings[0].origin, "getenv");
        assert!(findings[0].path.last().unwrap().code.contains("r->account = t"));
    }

    #[test]
    fn assign_sink_fires_on_setter_form() {
        let cpg = build(&[(
            "b.c",
            "void h() {\n    char* t = getenv(\"X\");\n    SetAccount(t);\n    SetColor(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["=account"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "=Account", "setter key keeps its spelling");
    }

    #[test]
    fn assign_sink_fires_from_entry_param_walk() {
        // The interprocedural walker (entry-model param_to_sink) must hit
        // assignment sinks too: handler's request data lands in an
        // identity field of an object another method trusts.
        let cpg = build(&[(
            "c.c",
            "void handler(char* q) {\n    r->tenant = q;\n}\n",
        )]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["=tenant"]);
        spec.source_methods.insert("handler".into());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "=tenant");
        assert_eq!(findings[0].origin, "handler(q)");
    }

    #[test]
    fn field_store_taints_only_its_path() {
        // The field-sensitivity contract: `c.a = tainted` reaches reads of
        // c.a and whole-value uses of c (containment), but NOT sibling
        // fields — the measured FP class of whole-object member stores.
        // Holds PERSIST_LOCK (as do the other member-store count tests):
        // the fixture stores AND reads the same fields, so a concurrent
        // test's CPG_PERSIST=1 window would stitch extra phase-2 findings
        // into the exact counts below.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "f.c",
            "void hit(void) {\n    struct C c;\n    c.a = getenv(\"X\");\n    system(c.a);\n}\n\nvoid sibling(void) {\n    struct C c;\n    c.a = getenv(\"X\");\n    system(c.b);\n}\n\nvoid contained(void) {\n    struct C c;\n    c.a = getenv(\"X\");\n    system(c);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        let methods: Vec<&str> = findings.iter().map(|f| f.method.as_str()).collect();
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(methods.iter().any(|m| m.contains("hit")), "{methods:?}");
        assert!(methods.iter().any(|m| m.contains("contained")), "{methods:?}");
        assert!(!methods.iter().any(|m| m.contains("sibling")), "{methods:?}");
    }

    #[test]
    fn nested_field_store_selects_and_contains() {
        // `x.sub.key = tainted`: the exact path fires, reading the
        // intermediate struct fires (it contains the field), a sibling
        // under the same intermediate does not. PERSIST_LOCK: see
        // field_store_taints_only_its_path.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "n.c",
            "void hit(void) {\n    struct A x;\n    x.sub.key = getenv(\"X\");\n    system(x.sub.key);\n}\n\nvoid contained(void) {\n    struct A x;\n    x.sub.key = getenv(\"X\");\n    system(x.sub);\n}\n\nvoid sibling(void) {\n    struct A x;\n    x.sub.key = getenv(\"X\");\n    system(x.sub.other);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        let methods: Vec<&str> = findings.iter().map(|f| f.method.as_str()).collect();
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(!methods.iter().any(|m| m.contains("sibling")), "{methods:?}");
    }

    #[test]
    fn alias_field_store_lands_on_partner_field() {
        // `p = &c; p->a = tainted` taints c.a (same path through the alias),
        // not all of c. PERSIST_LOCK: see field_store_taints_only_its_path.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "a.c",
            "void hit(void) {\n    struct C c;\n    struct C *p = &c;\n    p->a = getenv(\"X\");\n    system(c.a);\n}\n\nvoid sibling(void) {\n    struct C c;\n    struct C *p = &c;\n    p->a = getenv(\"X\");\n    system(c.b);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("hit"), "{findings:?}");
    }

    #[test]
    fn clean_field_store_does_not_launder_object() {
        // Writing one CLEAN field used to erase the whole object's taint
        // (whole-object member-store semantics); now it clears only its own
        // path. A plain rebind still clears everything, fields included.
        // PERSIST_LOCK: see field_store_taints_only_its_path.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "l.c",
            "void kept(struct C c) {\n    c = parse(getenv(\"X\"));\n    c.a = \"s\";\n    system(c);\n}\n\nvoid rebound(struct C c) {\n    c.a = getenv(\"X\");\n    c = fresh();\n    system(c.a);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("kept"), "{findings:?}");
    }

    #[test]
    fn entry_param_field_store_is_field_sensitive_too() {
        // The same contract holds in the entry-param walker: handler request
        // data stored into cfg.a must not fire a sink fed by cfg.b, while
        // cfg.a and whole-cfg uses do. PERSIST_LOCK: see
        // field_store_taints_only_its_path.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "e.c",
            "void handler(char* q) {\n    struct C cfg;\n    cfg.a = q;\n    system(cfg.a);\n}\n\nvoid handler2(char* q) {\n    struct C cfg;\n    cfg.a = q;\n    system(cfg.b);\n}\n",
        )]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods.insert("handler".into());
        spec.source_methods.insert("handler2".into());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("handler"), "{findings:?}");
        assert!(!findings[0].method.contains("handler2"), "{findings:?}");
    }

    #[test]
    fn assign_sink_matches_go_if_init_tenant_overwrite() {
        // Event-consumer tenant-isolation regression: a generated getter
        // feeds the account field through a Go if-init binding.
        // Matching is case-insensitive substring (`=account` ~ `Account`).
        let cpg = build_go(&[(
            "d.go",
            "package p\n\nfunc handle(evt *Evt, r *Req) {\n\tif tenantID := evt.GetTenantId(); tenantID != \"\" {\n\t\tr.Account = tenantID\n\t}\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["GetTenantId"], &["=account"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "=Account");
        assert_eq!(findings[0].origin, "GetTenantId");
    }

    #[test]
    fn assign_sink_fires_on_scala_named_argument() {
        // Policy-context regression: an authorization context is constructed
        // with a body-derived accountId named argument.
        let cpg = build_scala(&[(
            "E.scala",
            "class C {\n  def h(request: Request): Unit = {\n    val ctx = AuthzContext(accountId = request.body)\n  }\n}\n",
        )]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["=account"]);
        spec.source_methods.insert("h".into());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "=accountId");
    }

    #[test]
    fn persistence_stitch_connects_setter_to_getter_across_methods() {
        // writer() stores attacker data via setKey; reader() loads it via
        // getKey in a DIFFERENT method with no dataflow between them — the
        // store→load chain only reports under CPG_PERSIST.
        let cpg = build(&[(
            "s.c",
            "void writer() {\n    char* t = getenv(\"X\");\n    setKey(t);\n}\nvoid reader() {\n    char* v = getKey();\n    system(v);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_persist_stitch(&cpg, &spec, "persisted:getKey", "reader");
    }

    #[test]
    fn stores_after_the_first_sink_hit_are_still_harvested() {
        // handler() hits the sink FIRST, then stores the same tainted value.
        // The entry-model walk used to return at the hit, so `later` was
        // never harvested and job()'s read never stitched — two rule packs
        // with different sinks harvested different key sets from the same
        // module.
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cpg = build(&[(
            "p.c",
            "void handler(char* q) {\n    system(q);\n    cfg->later = q;\n}\nvoid job(struct C* cfg) {\n    system(cfg->later);\n}\n",
        )]);
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods.insert("handler".into());
        let store = summarise(&cpg);
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 1, "direct finding only");
        std::env::set_var("CPG_PERSIST", "1");
        let findings = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(
            findings.iter().any(|f| f.origin == "persisted:later" && f.method.contains("job")),
            "{findings:?}"
        );
    }

    #[test]
    fn persistence_stitch_member_store_to_field_read() {
        // writer() stores attacker data INTO A FIELD (`cfg->key = t`);
        // reader() reads the same field back in a different method. The
        // store is harvested from the `=` call's code text, the read
        // surfaces as a Call named `key` (field-read lowering).
        let cpg = build(&[(
            "m.c",
            "void writer(struct C* cfg) {\n    char* t = getenv(\"X\");\n    cfg->key = t;\n}\nvoid reader(struct C* cfg) {\n    system(cfg->key);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_persist_stitch(&cpg, &spec, "persisted:key", "reader");
    }

    #[test]
    fn witness_choice_among_tainted_args_is_deterministic() {
        // Both of pick()'s params flow to its return and BOTH arguments are
        // tainted: the reported witness must ride the LOWEST param index
        // (flows is a HashSet — unsorted iteration made the witness flip
        // between the two valid paths across runs).
        let cpg = build(&[(
            "w.c",
            "char* pick(char* a, char* b) {\n    if (a) return a;\n    return b;\n}\nvoid h() {\n    char* x = getenv(\"X\");\n    char* y = getenv(\"Y\");\n    char* r = pick(x, y);\n    system(r);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path: Vec<&str> = findings[0].path.iter().map(|s| s.code.as_str()).collect();
        assert!(
            path.iter().any(|c| c.contains("\"X\"")),
            "witness must ride param 0 (x): {path:?}"
        );
        assert!(
            !path.iter().any(|c| c.contains("\"Y\"")),
            "param 1 (y) must not be the witness: {path:?}"
        );
    }

    /// Scala fixture shared by the Point::Recv tests: a TYPED entry param
    /// (the type hint is what makes the callee summary trustworthy) whose
    /// method result feeds the sink only via receiver pass-through.
    fn recv_fixture() -> Cpg {
        build_scala(&[(
            "R.scala",
            "class C {\n  def h(t: Payload): Unit = {\n    val n = t.size()\n    system(n)\n  }\n}\n",
        )])
    }

    #[test]
    fn recv_modeled_summary_suppresses_receiver_pass_through() {
        // Without receiver knowledge `t.size()` conservatively carries t's
        // taint (the count could be anything). An external summary that
        // POSITIVELY models the receiver and declares no recv->return flow
        // licenses dropping it.
        let cpg = recv_fixture();
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods.insert("h".into());
        let store = summarise(&cpg);
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 1, "baseline pass-through");
        let mut store = summarise(&cpg);
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"Scala","methodName":"size"},
                     "receiverModeled":true}]"#,
            )
            .unwrap();
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 0, "recv-modeled: count is clean");
    }

    #[test]
    fn declared_recv_flow_keeps_pass_through_sanitized_does_not() {
        let cpg = recv_fixture();
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods.insert("h".into());
        let mut store = summarise(&cpg);
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"Scala","methodName":"size"},
                     "dataFlows":[{"from":"recv","to":"return"}]}]"#,
            )
            .unwrap();
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 1, "declared recv->return flows");
        let mut store = summarise(&cpg);
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"Scala","methodName":"size"},
                     "dataFlows":[{"from":"recv","to":"return","via":"clean"}]}]"#,
            )
            .unwrap();
        assert_eq!(
            find_flows(&cpg, &store, &spec).len(),
            0,
            "sanitized-only recv flow must not pass raw taint"
        );
    }

    #[test]
    fn untyped_receiver_ignores_recv_model() {
        // No type evidence on the receiver = dynamic dispatch: the summary
        // (matched by bare name) may belong to an unrelated method, so the
        // narrowing must NOT apply — pass-through stays conservative.
        let cpg = build_scala(&[(
            "U.scala",
            "class C {\n  def h(): Unit = {\n    val t = getenv(\"X\")\n    val n = t.size()\n    system(n)\n  }\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let mut store = summarise(&cpg);
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"Scala","methodName":"size"},
                     "receiverModeled":true}]"#,
            )
            .unwrap();
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 1, "{:?}", find_flows(&cpg, &store, &spec));
    }

    #[test]
    fn persisted_read_in_test_file_is_demoted() {
        // Phase-2 origination is a production-code contract: the SAME
        // getKey read sitting in a test harness (the DaoWrapperTest shape)
        // must not originate — only the production reader reports.
        let cpg = build(&[
            (
                "s.c",
                "void writer() {\n    char* t = getenv(\"X\");\n    setKey(t);\n}\nvoid reader() {\n    char* v = getKey();\n    system(v);\n}\n",
            ),
            (
                "dao_test.c",
                "void checkRoundTrip() {\n    char* v = getKey();\n    system(v);\n}\n",
            ),
        ]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_persist_stitch(&cpg, &spec, "persisted:getKey", "reader");
    }

    #[test]
    fn persistence_stitch_read_nested_in_member_chain_still_originates() {
        // The persisted read is an INNER link of a longer member chain
        // (`box.key.inner` — the Scala `cfg.executionUser.get` shape): the
        // field-sensitive chain branch must fall back to origination on the
        // inner source-named read, not kill the whole chain because the
        // path lookup is dead.
        let cpg = build(&[(
            "nm.c",
            "void writer(struct C* cfg) {\n    char* t = getenv(\"X\");\n    cfg->key = t;\n}\nvoid reader(struct C* box) {\n    system(box->key.inner);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_persist_stitch(&cpg, &spec, "persisted:key", "reader");
    }

    #[test]
    fn persistence_stitch_named_arg_store_to_getter() {
        // writer() stores via named-argument syntax (`store(Key = t)` — the
        // Scala copy/constructor shape, spelled in C); reader() loads via
        // the conventional getter.
        let cpg = build(&[(
            "n.c",
            "void writer() {\n    char* t = getenv(\"X\");\n    store(Key = t);\n}\nvoid reader() {\n    char* v = getKey();\n    system(v);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_persist_stitch(&cpg, &spec, "persisted:getKey", "reader");
    }

    #[test]
    fn persistence_stitch_scala_job_config_shape() {
        // Generic job-configuration shape end-to-end in Scala: an API method
        // stores attacker input into config via named-arg `copy`; a scheduled
        // task reads it back through a FIELD CHAIN and executes it. No
        // dataflow connects them — only the persistence stitch.
        let cpg = build_scala(&[(
            "JobConfig.scala",
            r#"
object JobConfigOps {
  def updateConfig(cfg: JobConfig): JobConfig = {
    val u = userInput()
    cfg.copy(executionUser = u)
  }
  def scheduledTask(cfg: JobConfig): Unit = {
    exec(cfg.executionSettings.executionUser)
  }
}
"#,
        )]);
        let spec = TaintSpec::new(&["userInput"], &["exec"]);
        assert_persist_stitch(&cpg, &spec, "persisted:executionUser", "scheduledTask");
    }

    #[test]
    fn c_copy_family_propagates_through_strcpy() {
        // The joern-parity fixture: getenv → strcpy into a stack buffer →
        // callee that runs system. Joern's `1->0` strcpy semantics carry
        // the taint into `buf`; without the copy table the flow died at
        // the copy.
        let cpg = build(&[(
            "cp.c",
            "void run_cmd(char *cmd) {\n    system(cmd);\n}\nint main() {\n    char buf[256];\n    char *user = getenv(\"X\");\n    strcpy(buf, user);\n    run_cmd(buf);\n    return 0;\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].origin, "getenv");
        assert_eq!(findings[0].sink, "system");
    }

    #[test]
    fn c_copy_family_propagates_inside_callee_summary() {
        // The copy happens INSIDE the callee whose summary lifts the
        // taint: param → snprintf into a local buffer → sink. Exercises
        // the summary walker's copy arm, not just analyse_method's.
        let cpg = build(&[(
            "cps.c",
            "void helper(char *in) {\n    char b[64];\n    snprintf(b, 64, \"%s\", in);\n    system(b);\n}\nint main() {\n    char *u = getenv(\"X\");\n    helper(u);\n    return 0;\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].origin, "getenv");
    }

    #[test]
    fn go_struct_literal_propagates_taint() {
        // `Foo{Path: t}` must behave like a constructor call: taint on the
        // keyed element's value reaches the constructed object. Before the
        // composite-literal lowering, `is_literal`'s substring match built
        // the whole literal as an untainted constant and DROPPED the flow.
        let cpg = build_go(&[(
            "l.go",
            "package main\n\nfunc h() {\n\tt := getenv(\"X\")\n\tc := Cmd{Path: t}\n\trun(c)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["run"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "run");
    }

    #[test]
    fn alias_member_store_taints_aliasee() {
        // `p = &cfg; p->key = tainted` writes THROUGH p into cfg — the read
        // `cfg.key` must fire. `other` was never aliased and stays clean.
        let cpg = build(&[(
            "a.c",
            "void h() {\n    struct C cfg;\n    struct C other;\n    struct C *p = &cfg;\n    p->key = getenv(\"X\");\n    system(other.key);\n    system(cfg.key);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink_line, Some(7), "must fire on cfg.key, not other.key");
    }

    #[test]
    fn alias_out_param_write_taints_aliasee() {
        // The C pointer-alias shape: `p = buf; read(fd, p, n)` fills buf.
        let cpg = build(&[(
            "b.c",
            "void h(int fd) {\n    char buf[64];\n    char *p = buf;\n    read(fd, p, 64);\n    system(buf);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn alias_rebind_dissolves_the_link() {
        // After `p = &other`, a store through p must no longer taint cfg —
        // only other. One finding, on other.key.
        let cpg = build(&[(
            "c.c",
            "void h() {\n    struct C cfg;\n    struct C other;\n    struct C *p = &cfg;\n    p = &other;\n    p->key = getenv(\"X\");\n    system(cfg.key);\n    system(other.key);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink_line, Some(8), "must fire on other.key, not cfg.key");
    }

    #[test]
    fn alias_member_store_taints_aliasee_inside_callee() {
        // The same shape one call deep: the callee is walked by
        // param_to_sink, which must carry the alias map too.
        let cpg = build(&[(
            "d.c",
            "void helper(char *t) {\n    struct C cfg;\n    struct C *p = &cfg;\n    p->key = t;\n    system(cfg.key);\n}\n\nvoid h() {\n    char *t = getenv(\"X\");\n    helper(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains('h'), "{findings:?}");
    }

    #[test]
    fn alias_transitive_chain_taints_root() {
        // Two-hop chain: `p = &cfg; q = p; q->key = tainted` — the write
        // through q must land on cfg via the transitive closure, and a
        // rebind mid-chain must still sever it (rebind_dissolves covers the
        // direct link; this covers the closure).
        let cpg = build(&[(
            "e.c",
            "void h() {\n    struct C cfg;\n    struct C *p = &cfg;\n    struct C *q = p;\n    q->key = getenv(\"X\");\n    system(cfg.key);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink_line, Some(6), "cfg.key fires through the 2-hop chain");
    }

    #[test]
    fn recv_qualified_sink_parses_into_simple_name_plus_qualification() {
        let spec = TaintSpec::new(&[], &["os.Create@0"]);
        // Simple name registered (ts frontends name member calls that way),
        // dotted name kept verbatim (cpg-lang-c names them by full text).
        assert!(spec.sinks.contains("Create"));
        assert!(spec.sinks.contains("os.Create"));
        assert_eq!(spec.sink_arg.get("Create"), Some(&0));
        assert_eq!(spec.sink_arg.get("os.Create"), Some(&0));
        assert!(spec.recv_qual["Create"].contains("os"));
        // Shape check: only the declared receiver root fires.
        assert!(spec.sink_shape_matches("Create", "os.Create(p)"));
        assert!(spec.sink_shape_matches("Create", "s.os.Create(p)"));
        assert!(!spec.sink_shape_matches("Create", "permissionEvaluatorFactory.Create(x)"));
        assert!(!spec.sink_shape_matches("Create", "Create(p)"));
        assert!(!spec.sink_shape_matches("Create", "myos.Create(p)"));
        // An unqualified sink name is unaffected by the mechanism.
        let plain = TaintSpec::new(&[], &["Create@0"]);
        assert!(plain.sink_shape_matches("Create", "permissionEvaluatorFactory.Create(x)"));
    }

    #[test]
    fn position_qualified_sink_rejects_field_reads() {
        // The field-read lowering makes `token.Raw` a Call named `Raw`
        // (arg 0 = base), which matched `Raw@0` — a JWT field read is not a
        // SQL sink. A position qualifier implies an argument list, so only
        // invocation shapes fire.
        let spec = TaintSpec::new(&[], &["Raw@0"]);
        assert!(spec.sink_shape_matches("Raw", "queries.Raw(query, args...)"));
        assert!(!spec.sink_shape_matches("Raw", "token.Raw"));
        assert!(!spec.sink_shape_matches("Raw", "cert.Raw"));
        // Unqualified sinks keep matching paren-less shapes (Scala postfix).
        let postfix = TaintSpec::new(&[], &["!"]);
        assert!(postfix.sink_shape_matches("!", "Process(cmd).!"));
    }

    #[test]
    fn persist_variants_symmetric_across_casings() {
        // The read-ubiquity filter and phase-2 sources must count the same
        // reads for keys differing only in first-letter casing — asymmetric
        // groups let `assetStore` stitch after `AssetStore` was dropped.
        let lower = persist_variants("assetStore");
        let upper = persist_variants("AssetStore");
        for heavy in ["GetAssetStore", "getAssetStore", "AssetStore", "assetStore"] {
            assert!(lower.iter().any(|v| v == heavy), "lower missing {heavy}: {lower:?}");
            assert!(upper.iter().any(|v| v == heavy), "upper missing {heavy}: {upper:?}");
        }
    }

    #[test]
    fn recv_qualified_sink_fires_only_through_declared_receiver() {
        // Receiver-name collision regression: `Create@0` fired on
        // `permissionEvaluatorFactory.Create(...)` because member calls are
        // named by their trailing member. `os.Create@0` must fire on the
        // real file sink and stay quiet on the name-colliding factory.
        let src = "package main\n\nfunc fileWrite() {\n\tp := getenv(\"X\")\n\tos.Create(p)\n}\n\nfunc factoryUse() {\n\tp := getenv(\"X\")\n\tfactory.Create(p)\n}\n";
        let cpg = build_go(&[("s.go", src)]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["os.Create@0"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("fileWrite"), "{findings:?}");
        // Control: the unqualified spelling keeps today's recall (both fire).
        let plain = TaintSpec::new(&["getenv"], &["Create@0"]);
        assert_eq!(find_flows(&cpg, &store, &plain).len(), 2);
    }

    #[test]
    fn tsx_jsx_attribute_sink_fires_from_hook_source() {
        // The React XSS shape: a URL-derived value bound into
        // `dangerouslySetInnerHTML`. Three lowerings compose: the tsx
        // dialect grammar parses the markup, the JSX attribute becomes a
        // named call (the sink), and the object literal `{__html: q}`
        // carries its value's taint into it.
        let src = "function Page() {\n  const q = useSearchParams();\n  return <div className=\"x\" dangerouslySetInnerHTML={{__html: q}} />;\n}\n";
        let cpg = build_tsx(&[("p.tsx", src)]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["useSearchParams"], &["dangerouslySetInnerHTML@0"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("Page"), "{findings:?}");
    }

    #[test]
    fn python_fstring_carries_source_ident_taint() {
        // The Flask shape: `request` is tainted at every read
        // (source_idents), and an f-string is concatenation, not a constant
        // — without the interp lowering the whole argument is a literal and
        // the flow is dropped.
        let src = "def handler():\n    q = request.args\n    os.system(f\"convert {q}\")\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("handler"), "{findings:?}");
    }

    #[test]
    fn python_list_argument_carries_element_taint() {
        // `subprocess.check_output([binary, q])` — the list literal must not
        // swallow its elements; sink_arg 0 sees the collection call whose
        // taint comes from the tainted element.
        let src = "def handler():\n    q = request.args\n    subprocess.check_output([\"convert\", q])\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["check_output@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn python_tuple_for_binding_carries_element_taint() {
        // `for k, v in cfg.items()` binds BOTH names — the single-name loop
        // binding dropped everything after the first identifier, which is
        // exactly where dict iteration carries its values (the
        // config-decode handler shape).
        let src = "def handler():\n    cfg = request.json\n    for k, v in cfg.items():\n        os.system(v)\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn python_dict_comprehension_binds_loop_vars() {
        // A comprehension's for_in_clause is a loop binding in expression
        // position, and its body sits textually ABOVE the clause — the
        // binding must be emitted first (stamped at the comprehension's
        // start line) or the line-ordered pass reads the body untainted.
        let src = "def handler():\n    cfg = request.json\n    fns = {k: os.system(v[\"enc\"]) for k, v in cfg.get(\"functions\", {}).items()}\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn python_keyword_argument_carries_value_taint() {
        // `os.system(cmd=q)`: keyword_argument used to drop the whole
        // argument from the graph; it now lowers to the nested `=`
        // named-argument shape, through which the value's taint passes.
        let src = "def handler():\n    q = request.args\n    os.system(cmd=q)\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn python_call_nested_in_keyword_argument_exists() {
        // A call nested in a kwarg VALUE (`Spec(fn=os.system(q))`) used to
        // vanish with the dropped argument — the sink inside it must fire.
        let src = "def handler():\n    q = request.args\n    s = Spec(fn=os.system(q))\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn shell_dash_c_payload_flags_despite_position_qualifier() {
        // `Command@0` models "tainted binary" — but `Command("sh", "-c", t)`
        // moves the injection to the payload after the -c flag, which the
        // position qualifier must not silence. A tainted argv element to a
        // FIXED non-shell binary stays quiet (the accepted trade-off).
        let cpg = build_go(&[(
            "c.go",
            "package main\n\nfunc bad() {\n\tt := getenv(\"X\")\n\texec.Command(\"sh\", \"-c\", t)\n}\n\nfunc ok() {\n\tt := getenv(\"X\")\n\texec.Command(\"ls\", \"-l\", t)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["Command@0"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("bad"), "{findings:?}");
    }

    #[test]
    fn shellform_assembly_in_slice_literal_flags() {
        // The pod-spec shape: `Command: []string{"/bin/sh", "-c", cmd}` is a
        // command-line ASSEMBLY — Kubernetes runs it, not this process, so
        // no exec sink ever sees it. The `<shellform>` pseudo-sink flags the
        // tainted payload; a constant payload stays quiet.
        let cpg = build_go(&[(
            "k.go",
            "package main\n\nfunc bad() {\n\tcmd := getenv(\"X\")\n\tc := Container{Command: []string{\"/bin/sh\", \"-c\", cmd}}\n\tapply(c)\n}\n\nfunc ok() {\n\tc := Container{Command: []string{\"/bin/sh\", \"-c\", \"ray stop\"}}\n\tapply(c)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["<shellform>"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "<shellform>");
        assert!(findings[0].method.contains("bad"), "{findings:?}");
    }

    #[test]
    fn query_call_does_not_transfer_object_state() {
        // `entry.Match(tainted)` READS its argument — the builder-pattern
        // object-state transfer must not taint `entry` (the router-dispatch
        // FP shape); a real mutator (`AddScript`) still transfers.
        let cpg = build_go(&[(
            "q.go",
            "package main\n\nfunc quiet() {\n\tq := getenv(\"X\")\n\tentry := lookup()\n\tentry.Match(q)\n\tsystem(entry)\n}\n\nfunc loud() {\n\tq := getenv(\"X\")\n\tps := lookup()\n\tps.AddScript(q)\n\tsystem(ps)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("loud"), "{findings:?}");
    }

    #[test]
    fn field_read_call_does_not_hand_off_to_same_named_function() {
        // `cfg.job` is a member READ lowered to a Call named `job` — it must
        // not descend into the unrelated FUNCTION `job` as if invoked (the
        // route-table `handlerEntry.handler` FP shape). A real invocation
        // still hands off.
        let cpg = build_go(&[(
            "f.go",
            "package main\n\nfunc job(a string) {\n\tsystem(a)\n}\n\nfunc quiet() {\n\tcfg := getenv(\"X\")\n\tuse(cfg.job)\n}\n\nfunc loud() {\n\tcfg := getenv(\"X\")\n\tjob(cfg)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("loud"), "{findings:?}");
    }

    #[test]
    fn python_kwarg_maps_to_named_parameter() {
        // Swapped keyword order: `run(cwd="/tmp", cmd=q)` must feed q to the
        // param NAMED cmd (which reaches the sink), not to positional param
        // 1 (cwd, unused) — and `run(cwd=q, cmd="ls")` must stay quiet even
        // though its tainted kwarg sits at position 0.
        let src = "def run(cmd, cwd):\n    os.system(cmd)\n\ndef loud():\n    q = request.args\n    run(cwd=\"/tmp\", cmd=q)\n\ndef quiet():\n    q = request.args\n    run(cwd=q, cmd=\"ls\")\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("loud"), "{findings:?}");
    }

    #[test]
    fn python_kwarg_maps_by_name_through_summary_splice() {
        // The summary path (param k → return): wrap's flow is param 0 (cmd)
        // → return, and the call site names it by keyword in swapped order —
        // the splice must pick the cmd-keyed value, not args[0].
        let src = "def wrap(cmd, cwd):\n    return cmd\n\ndef handler():\n    q = request.args\n    out = wrap(cwd=\"/tmp\", cmd=q)\n    os.system(out)\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("handler"), "{findings:?}");
    }

    #[test]
    fn python_parallel_assignment_binds_pairwise() {
        // `a, b = "safe", q` binds b to q (second value), NOT to the whole
        // rhs — and `c, d = q, "safe"` leaves d clean.
        let src = "def loud():\n    q = request.args\n    a, b = \"safe\", q\n    os.system(b)\n\ndef quiet():\n    q = request.args\n    c, d = q, \"safe\"\n    os.system(d)\n";
        let cpg = build_python(&[("h.py", src)]);
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system@0"]);
        spec.source_idents.insert("request".to_string());
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("loud"), "{findings:?}");
    }

    #[test]
    fn destructure_from_call_binds_every_name() {
        // `ok, data := fetch(tainted)`: the taint-relevant value lands in the
        // SECOND name — the single-name binding lost it entirely.
        let cpg = build_go(&[(
            "d.go",
            "package main\n\nfunc handler() {\n\tok, data := fetch(getenv(\"X\"))\n\tsystem(data)\n\tuse(ok)\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system@0"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn go_range_binding_carries_element_taint() {
        // Go spells the loop binding on a range_clause CHILD of the
        // for_statement, not on the control node itself — the clause-holder
        // scan must find it or range iteration drops all taint.
        let cpg = build_go(&[(
            "r.go",
            "package main\n\nfunc handler() {\n\tm := getenv(\"X\")\n\tfor _, v := range m {\n\t\tsystem(v)\n\t}\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system@0"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn persistence_stitch_go_struct_literal_store_to_field_read() {
        // The event-delivery service shape: the API handler stores attacker input
        // through a struct literal (`Endpoint{Url: u}` — Go's named-arg
        // spelling); the delivery path reads the field back in a different
        // method with no dataflow between them.
        let cpg = build_go(&[(
            "w.go",
            "package main\n\nfunc createEndpoint() {\n\tu := getenv(\"X\")\n\tsave(Endpoint{Url: u})\n}\n\nfunc deliver(e EndpointConfig) {\n\tfetch(e.Url)\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["fetch"]);
        assert_persist_stitch(&cpg, &spec, "persisted:Url", "deliver");
    }

    #[test]
    fn go_return_flow_through_local_helper() {
        // `u := pass(t); run(u)` — taint must survive the resolved local
        // helper's return via its param->return summary.
        let cpg = build_go(&[(
            "r.go",
            "package main\n\nfunc pass(s string) string {\n\tx := s\n\treturn x\n}\n\nfunc h() {\n\tt := getenv(\"X\")\n\tu := pass(t)\n\trun(u)\n}\n",
        )]);
        let store = summarise(&cpg);
        assert!(
            store.get("pass").is_some_and(|s| s.flows_to_return().count() > 0),
            "pass summary missing param->return flow: {:?}",
            store.get("pass")
        );
        let spec = TaintSpec::new(&["getenv"], &["run"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn persistence_harvest_through_return_and_splice() {
        // The full event-delivery service shape: taint crosses a helper's RETURN,
        // then a spliced callee stores it through a struct literal whose
        // value is a FIELD READ of the tainted param. Harvest must fire in
        // the param_to_sink chain, not just in the entry method itself.
        let cpg = build_go(&[(
            "w2.go",
            "package main\n\nfunc clean(p string) string {\n\treturn p\n}\n\nfunc handler() {\n\tt := getenv(\"X\")\n\tq := clean(t)\n\tconvert(q)\n}\n\nfunc convert(proto Payload) {\n\tsave(Hook{URL: proto.Url})\n}\n\nfunc deliver(g Prov) {\n\tfetch(g.Url)\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["fetch"]);
        assert_persist_stitch(&cpg, &spec, "persisted:Url", "deliver");
    }

    #[test]
    fn persistence_stitch_initialism_cross_casing() {
        // Go initialism lint splits the SAME field across casings: the DB
        // model stores `URL` (event-delivery service a persistence model), the
        // proto-gen read side spells it `Url` (gp.Url). The variant set
        // must bridge the two spellings.
        let cpg = build_go(&[(
            "i.go",
            "package main\n\nfunc create() {\n\tu := getenv(\"X\")\n\tsave(Hook{URL: u})\n}\n\nfunc deliver(g Prov) {\n\tfetch(g.Url)\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["fetch"]);
        assert_persist_stitch(&cpg, &spec, "persisted:Url", "deliver");
    }

    #[test]
    fn persistence_ubiquity_filter_drops_widely_stored_keys() {
        // `Key` is stored from TWO distinct METHODS. With the ubiquity
        // threshold at 2 the key is infrastructure-like and must NOT stitch;
        // at the default threshold (5) it still does. The read and the sink
        // sit in DIFFERENT methods (reader -> helper -> system) so the
        // sink-relevance rescue stays out of this test's way.
        let cpg = build(&[(
            "u.c",
            "void writerA() {\n    char* t = getenv(\"X\");\n    setKey(t);\n}\nvoid writerB() {\n    char* t = getenv(\"Y\");\n    setKey(t);\n}\nvoid reader() {\n    char* v = getKey();\n    helper(v);\n}\nvoid helper(char* x) {\n    system(x);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let store = summarise(&cpg);
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CPG_PERSIST", "1");
        std::env::set_var("CPG_PERSIST_UBIQ", "2");
        let dropped = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST_UBIQ");
        let stitched = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(dropped.len(), 0, "ubiquitous key must be filtered: {dropped:?}");
        assert_eq!(stitched.len(), 1, "default threshold must keep 2-method keys: {stitched:?}");
        assert_eq!(stitched[0].origin, "persisted:getKey");
    }

    #[test]
    fn persistence_ubiquity_counts_methods_not_store_sites() {
        // `Key` is stored from TWO sites in ONE method — a copy-chain shape,
        // and exactly what better recall surfaces more of. Under a site
        // count the key would cross a threshold of 2 and vanish (a validation-corpus
        // regression: recall gains ate confirmed keys); under the method
        // count it stays. Read and sink are in different methods so the
        // rescue cannot mask a metric bug.
        let cpg = build(&[(
            "mc.c",
            "void writer() {\n    char* t = getenv(\"X\");\n    setKey(t);\n    char* u = getenv(\"Y\");\n    setKey(u);\n}\nvoid reader() {\n    char* v = getKey();\n    helper(v);\n}\nvoid helper(char* x) {\n    system(x);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let store = summarise(&cpg);
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CPG_PERSIST", "1");
        std::env::set_var("CPG_PERSIST_UBIQ", "2");
        let findings = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST_UBIQ");
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(findings.len(), 1, "1 store method < threshold 2: {findings:?}");
        assert_eq!(findings[0].origin, "persisted:getKey");
    }

    #[test]
    fn persistence_sink_relevance_rescues_over_threshold_key() {
        // `Key` is stored from TWO distinct methods (over the threshold of
        // 2), but its getter is read in a method that ALSO calls the spec
        // sink — dropping it would be guaranteed TP loss, so the rescue
        // must stitch it anyway.
        let cpg = build(&[(
            "r.c",
            "void writerA() {\n    char* t = getenv(\"X\");\n    setKey(t);\n}\nvoid writerB() {\n    char* t = getenv(\"Y\");\n    setKey(t);\n}\nvoid reader() {\n    char* v = getKey();\n    system(v);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let store = summarise(&cpg);
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CPG_PERSIST", "1");
        std::env::set_var("CPG_PERSIST_UBIQ", "2");
        let findings = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST_UBIQ");
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(findings.len(), 1, "sink-relevant key must be rescued: {findings:?}");
        assert_eq!(findings[0].origin, "persisted:getKey");
        assert!(findings[0].method.contains("reader"), "{findings:?}");
    }

    #[test]
    fn persistence_rescue_does_not_bypass_read_side_filter() {
        // Same rescue shape as above, but the read-side ubiquity threshold
        // is forced to 1 — the read filter is the noise backstop that keeps
        // requestContext-shaped keys out, and a rescued key must NOT skip
        // it.
        let cpg = build(&[(
            "rb.c",
            "void writerA() {\n    char* t = getenv(\"X\");\n    setKey(t);\n}\nvoid writerB() {\n    char* t = getenv(\"Y\");\n    setKey(t);\n}\nvoid reader() {\n    char* v = getKey();\n    system(v);\n}\n",
        )]);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let store = summarise(&cpg);
        let _g = PERSIST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CPG_PERSIST", "1");
        std::env::set_var("CPG_PERSIST_UBIQ", "2");
        std::env::set_var("CPG_PERSIST_READS", "1");
        let findings = find_flows(&cpg, &store, &spec);
        std::env::remove_var("CPG_PERSIST_READS");
        std::env::remove_var("CPG_PERSIST_UBIQ");
        std::env::remove_var("CPG_PERSIST");
        assert_eq!(findings.len(), 0, "read backstop must still apply: {findings:?}");
    }

    #[test]
    fn format_verb_filter_blocks_int_verbs_and_passes_string_verbs() {
        // %d cannot smuggle taint; %s can. Non-literal formats stay
        // conservative (any tainted arg passes through).
        let cpg = build(&[(
            "f.c",
            "void h() {\n    char* t = getenv(\"X\");\n    char* a = Sprintf(\"q %d w\", t);\n    system(a);\n    char* b = Sprintf(\"q %s w\", t);\n    system(b);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "only the %s flow may survive: {findings:?}");
        assert!(findings[0].path.iter().any(|s| s.code.contains("%s")));
    }

    #[test]
    fn format_verb_filter_traces_literal_through_local() {
        // h1: format in a single-assignment local, %d only -> filtered.
        // h2: same shape, %s -> flow survives.
        // h3: local reassigned -> unresolvable -> conservative -> survives.
        // h4: format is a parameter -> caller-controlled -> survives.
        let cpg = build(&[(
            "fl.c",
            concat!(
                "void h1() {\n    char* t = getenv(\"X\");\n    char* q = \"sel %d upd\";\n    char* a = Sprintf(q, t);\n    system(a);\n}\n",
                "void h2() {\n    char* t = getenv(\"X\");\n    char* q = \"sel %s upd\";\n    char* a = Sprintf(q, t);\n    system(a);\n}\n",
                "void h3() {\n    char* t = getenv(\"X\");\n    char* q = \"sel %d upd\";\n    q = pick();\n    char* a = Sprintf(q, t);\n    system(a);\n}\n",
                "void h4(char* q) {\n    char* t = getenv(\"X\");\n    char* a = Sprintf(q, t);\n    system(a);\n}\n",
            ),
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        let hit = |m: &str| findings.iter().any(|f| f.method.contains(m));
        assert!(!hit("h1"), "int-verb local format must be filtered: {findings:?}");
        assert!(hit("h2"), "string-verb local format must survive: {findings:?}");
        assert!(hit("h3"), "reassigned local must stay conservative: {findings:?}");
        assert!(hit("h4"), "parameter format must stay conservative: {findings:?}");
    }

    #[test]
    fn format_verb_filter_traces_two_hop_local_chain() {
        // base = literal; q = base; Sprintf(q, t) — one identifier hop
        // through resolve_format_literal's recursion.
        let cpg = build(&[(
            "fl2.c",
            "void h() {\n    char* t = getenv(\"X\");\n    char* base = \"sel %d upd\";\n    char* q = base;\n    char* a = Sprintf(q, t);\n    system(a);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 0, "two-hop literal chain must be filtered: {findings:?}");
    }

    #[test]
    fn guard_annotation_classifies_pre_and_post_checks() {
        // g1: CCHECK_LE BEFORE the memcpy -> guarded@; g2: the only check
        // AFTER the copy -> post-sink-check@ (check-after-write shape);
        // g3: no check at all -> None.
        let cpg = build(&[(
            "g.c",
            concat!(
                "void g1(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    CCHECK_LE(buf, 8);\n    memcpy(dst, src, buf);\n}\n",
                "void g2(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    memcpy(dst, src, buf);\n    if (buf > 8) { fail(); }\n}\n",
                "void g3(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    memcpy(dst, src, buf);\n}\n",
            ),
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["memcpy@2"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 3, "{findings:?}");
        let by_m = |m: &str| {
            findings.iter().find(|f| f.method.contains(m)).unwrap().guard.clone()
        };
        assert!(by_m("g1").is_some_and(|g| g.starts_with("guarded@")), "{findings:?}");
        assert!(by_m("g2").is_some_and(|g| g.starts_with("post-sink-check@")), "{findings:?}");
        assert_eq!(by_m("g3"), None, "{findings:?}");
    }

    #[test]
    fn grow_guard_recognizes_grow_before_copy() {
        // n1: the ndc shape — Expand's argument ties to the sink only through
        // the single-assignment `required = leng + nbuf + 3` (one-hop
        // widening); n2: direct ident overlap (dbuf.reserve(nbuf));
        // n3: argless grow on the receiver's own buffer.
        let cpg = build(&[(
            "n.c",
            concat!(
                "void n1(int fd) {\n    char nbuf[8];\n    read(fd, nbuf, 8);\n    required = leng + nbuf + 3;\n    if (required > cap) { Expand(required); }\n    memcpy(dst, src, nbuf);\n}\n",
                "void n2(int fd) {\n    char nbuf[8];\n    read(fd, nbuf, 8);\n    reserve(dbuf, nbuf);\n    memcpy(dbuf, src, nbuf);\n}\n",
                "void n3(int fd) {\n    char nbuf[8];\n    read(fd, nbuf, 8);\n    Grow();\n    memcpy(dst, src, nbuf);\n}\n",
            ),
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["memcpy@2"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 3, "{findings:?}");
        let by_m = |m: &str| {
            findings.iter().find(|f| f.method.contains(m)).unwrap().guard.clone()
        };
        assert!(by_m("n1").is_some_and(|g| g.starts_with("grow-guarded@")), "{findings:?}");
        assert!(by_m("n2").is_some_and(|g| g.starts_with("grow-guarded@")), "{findings:?}");
        assert!(by_m("n3").is_some_and(|g| g.starts_with("grow-guarded@")), "{findings:?}");
    }

    #[test]
    fn grow_guard_ignores_unrelated_grow() {
        // The grow's arguments tie to a DIFFERENT buffer (no shared idents,
        // no assignment hop into the sink's identifiers) — must stay None.
        let cpg = build(&[(
            "u.c",
            "void u1(int fd) {\n    char nbuf[8];\n    read(fd, nbuf, 8);\n    reserve(other, extra);\n    memcpy(dst, src, nbuf);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["memcpy@2"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].guard, None, "{findings:?}");
    }

    #[test]
    fn downrank_guarded_orders_unguarded_first() {
        // Same fixture as the classification test: g1 guarded@, g2
        // post-sink-check@, g3 None. Down-ranked order: g2, g3 (tier 0,
        // original relative order) before g1 (tier 1).
        let cpg = build(&[(
            "d.c",
            concat!(
                "void g1(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    CCHECK_LE(buf, 8);\n    memcpy(dst, src, buf);\n}\n",
                "void g2(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    memcpy(dst, src, buf);\n    if (buf > 8) { fail(); }\n}\n",
                "void g3(int fd) {\n    char buf[8];\n    read(fd, buf, 8);\n    memcpy(dst, src, buf);\n}\n",
            ),
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["memcpy@2"]);
        let mut findings = find_flows(&cpg, &store, &spec);
        downrank_guarded(&mut findings);
        let order: Vec<&str> = findings
            .iter()
            .map(|f| {
                if f.method.contains("g1") { "g1" }
                else if f.method.contains("g2") { "g2" }
                else { "g3" }
            })
            .collect();
        assert_eq!(order, vec!["g2", "g3", "g1"], "{findings:?}");
    }

    #[test]
    fn out_all_args_variant_taints_every_argument() {
        // `Read@out*`: overload families where the buffer position varies —
        // every argument's root identifier becomes tainted.
        let cpg = build(&[(
            "w.c",
            "void h(int fd) {\n    char a[8];\n    char b[8];\n    Read(a, b, 8);\n    system(a);\n    system(b);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["Read@out*"], &["system"]);
        assert_eq!(spec.out_param_sources.get("Read"), Some(&OUT_ALL_ARGS));
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 2, "both buffers must report: {findings:?}");
    }

    #[test]
    fn out_param_source_reaches_sink_through_callee() {
        // The out-param write happens in the caller; the tainted buffer is
        // then handed to a helper whose parameter reaches the sink —
        // exercises the param_to_sink mirror via the interproc hand-off.
        let cpg = build(&[(
            "p.c",
            "void run(char* s) {\n    system(s);\n}\nvoid h(int fd) {\n    char buf[64];\n    read(fd, &buf, 64);\n    run(buf);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["read@out1"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert!(
            findings.iter().any(|f| f.sink == "system" && f.method.contains('h')),
            "read(&buf) -> run(buf) -> system must report: {findings:?}"
        );
    }

    #[test]
    fn sanitizer_kills_finding_but_raw_path_still_reports() {
        // `u` is laundered through clean() — must NOT be reported; `t` flows
        // raw into the second sink — MUST be reported. One program, both cases.
        let cpg = build(&[(
            "v.c",
            "void h() {\n    char* t = getenv(\"X\");\n    char* u = clean(t);\n    system(u);\n    system(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "only the raw path may report: {findings:?}");
        assert_eq!(findings[0].sink, "system");
        assert!(
            findings[0].path.last().unwrap().code.contains("system(t)"),
            "the surviving finding must be the raw one: {:?}",
            findings[0].path
        );

        // Without the sanitizer configured, clean() has no summary at all, so
        // still only the raw path reports — but with a passthrough summary for
        // clean and no sanitizer marking, both would report. Prove the
        // sanitizer (not summary absence) is what kills the laundered path:
        let mut store2 = SummaryStore::new();
        store2
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"clean"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store2.compute_all(&cpg);
        let no_san = TaintSpec::new(&["getenv"], &["system"]);
        assert_eq!(find_flows(&cpg, &store2, &no_san).len(), 2, "both paths report without sanitizer");
        let with_san = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        assert_eq!(find_flows(&cpg, &store2, &with_san).len(), 1, "sanitizer kills the laundered path");
    }

    #[test]
    fn sanitized_callee_summary_does_not_propagate() {
        // wrap()'s summary is param0 -> return VIA escape (sanitized) because
        // the store knows `escape` is a sanitizer. The lift must not happen.
        let cpg = build(&[(
            "w.c",
            "char* wrap(char* s) {\n    return escape(s);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["escape"]);
        store.compute_all(&cpg);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["escape"]);
        assert_eq!(
            find_flows(&cpg, &store, &spec).len(),
            0,
            "a sanitized summary flow must not lift raw taint"
        );
    }

    #[test]
    fn spec_only_sanitizer_inside_callee_kills_via_chain_recheck() {
        // The store computed wrap's summary WITHOUT knowing `clean` is a
        // sanitizer (clean has a raw external passthrough summary), so wrap's
        // flow looks raw. The query-time spec names `clean` a sanitizer; the
        // callee-chain recheck must discover the path is sanitizer-only and
        // kill the finding.
        let cpg = build(&[(
            "x.c",
            "char* wrap(char* s) {\n    return clean(s);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"clean"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store.compute_all(&cpg);
        // Sanity: without the sanitizer the flow reports.
        assert_eq!(find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"])).len(), 1);
        // With it, the only path is through clean() inside wrap(): killed.
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 0);
    }

    #[test]
    fn witness_includes_callee_internal_steps_with_provenance() {
        let cpg = build(&[(
            "y.c",
            "char* wrap(char* s) {\n    char* r = s;\n    return r;\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;

        // Ends run in the reporting method at depth 0.
        assert!(path.first().unwrap().code.contains("getenv"));
        assert_eq!(path.first().unwrap().depth, 0);
        assert!(path.last().unwrap().code.contains("system"));
        assert_eq!(path.last().unwrap().depth, 0);

        // The callee's internal chain is spliced in at depth 1.
        assert!(
            path.iter().any(|s| s.depth == 1 && s.code.contains("r = s")),
            "expected wrap's internal assignment in the witness: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 1 && s.code.contains("return r")),
            "expected wrap's return in the witness: {path:?}"
        );

        // The hop through wrap carries computed-summary provenance at depth 0.
        assert!(
            path.iter().any(|s| s.depth == 0
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "wrap".into() }),
            "expected a SummaryFlow hop for wrap: {path:?}"
        );
        // Internal steps are ordered between the source and the hop.
        let src = path.iter().position(|s| s.code.contains("getenv")).unwrap();
        let internal = path.iter().position(|s| s.code.contains("r = s")).unwrap();
        let hop = path
            .iter()
            .position(|s| matches!(s.provenance, Provenance::SummaryFlow { .. }))
            .unwrap();
        assert!(src < internal && internal < hop, "splice order wrong: {path:?}");
    }

    #[test]
    fn external_summary_hop_is_marked_summary_only() {
        let cpg = build(&[(
            "z.c",
            "void h() {\n    char* t = getenv(\"X\");\n    system(strdup(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"strdup"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store.compute_all(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;
        assert!(
            path.iter().any(|s| s.provenance
                == Provenance::ExternalSummary { callee_fqn: "strdup".into() }),
            "expected an ExternalSummary hop: {path:?}"
        );
        // External summaries have no body: nothing spliced below depth 0.
        assert!(
            path.iter().all(|s| s.depth == 0),
            "external hops must be summary-only: {path:?}"
        );
    }

    #[test]
    fn intraproc_steps_carry_intraproc_provenance() {
        let cpg = build(&[(
            "p.c",
            "void h() {\n    char* t = getenv(\"X\");\n    system(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let findings = find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .path
                .iter()
                .all(|s| s.provenance == Provenance::IntraProc && s.depth == 0),
            "pure intraprocedural flow: {:?}",
            findings[0].path
        );
    }

    #[test]
    fn nested_lift_splices_two_levels() {
        // h -> outer -> inner: the witness must contain inner's steps at
        // depth 2 and outer's at depth 1, each hop with its own provenance.
        let cpg = build(&[(
            "n.c",
            "char* inner(char* a) {\n    return a;\n}\nchar* outer(char* b) {\n    return inner(b);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(outer(t));\n}\n",
        )]);
        let store = summarise(&cpg);
        let findings = find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;
        assert!(
            path.iter().any(|s| s.depth == 2 && s.code.contains("return a")),
            "inner's return should appear at depth 2: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 1
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "inner".into() }),
            "the inner() hop inside outer should be at depth 1: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 0
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "outer".into() }),
            "the outer() hop should be at depth 0: {path:?}"
        );
    }

    #[test]
    fn tainted_argument_reaches_sink_inside_callee() {
        // The sink fires inside run(), not in h() — a param→return summary
        // cannot express this; param→sink reachability must.
        let cpg = build(&[(
            "q.c",
            "void run(char* c) {\n    system(c);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    run(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let findings = find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink, "system");
        assert_eq!(findings[0].method, "h", "reported in the caller where the taint enters");
        assert!(
            findings[0].path.iter().any(|s| s.depth == 1 && s.code.contains("system(c)")),
            "callee-internal sink step expected: {:?}",
            findings[0].path
        );
        // Two levels deep, and sanitized hand-off must NOT report.
        let cpg2 = build(&[(
            "q2.c",
            "void run(char* c) {\n    system(c);\n}\nvoid mid(char* b) {\n    run(b);\n}\nvoid safe(char* s) {\n    system(clean(s));\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    mid(t);\n    safe(t);\n}\n",
        )]);
        let store2 = summarise(&cpg2);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        let findings2 = find_flows(&cpg2, &store2, &spec);
        assert_eq!(findings2.len(), 1, "only the unsanitized 2-hop path: {findings2:?}");
        assert!(findings2[0].path.iter().any(|s| s.depth == 2 && s.code.contains("system(c)")));
    }

    #[test]
    fn sink_arg_position_restricts_findings() {
        // Taint reaches argument 1 of the sink. `run_sql@1` must report,
        // `run_sql@0` must not (position 0 holds an untainted handle).
        let cpg = build(&[(
            "s.c",
            "void h() {\n    char* t = getenv(\"X\");\n    run_sql(db, t);\n}\n",
        )]);
        let store = summarise(&cpg);
        assert_eq!(
            find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["run_sql@1"])).len(),
            1,
            "dangerous position tainted: must report"
        );
        assert_eq!(
            find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["run_sql@0"])).len(),
            0,
            "only a safe position tainted: must not report"
        );
        assert_eq!(
            find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["run_sql"])).len(),
            1,
            "no restriction: any tainted argument reports"
        );
    }

    #[test]
    fn provenance_serializes() {
        let step = Step {
            code: "wrap(t)".into(),
            line: Some(3),
            provenance: Provenance::SummaryFlow { callee_fqn: "wrap".into() },
            depth: 0,
        };
        let js = serde_json::to_value(&step).unwrap();
        assert_eq!(js["provenance"]["SummaryFlow"]["callee_fqn"], "wrap");
        assert_eq!(js["depth"], 0);
    }

    #[test]
    fn mines_router_registration_and_flows() {
        let cpg = build_go(&[(
            "srv.go",
            r#"package main
func handleQ(w ResponseWriter, r *Request) {
	q := r.FormValue("q")
	system(q)
}
func main() {
	HandleFunc("/q", handleQ)
}
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert_eq!(mined, vec!["handleQ".to_string()], "bare function ref mined");
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system"]);
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 0, "no entries, no flow");
        spec.source_methods_registered = mined.into_iter().collect();
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("handleQ"), "{findings:?}");
    }

    #[test]
    fn mines_inline_closure_registration_and_flows() {
        // Gin/stdlib idiom: the handler is an inline closure, not a named
        // function. The MethodRef arm mines it as a position-qualified
        // entry (`name@file:line`), and both the taint entry matcher and
        // the census resolve that spelling.
        let cpg = build_go(&[(
            "srv.go",
            r#"package main
func main() {
	GET("/q", func(w ResponseWriter, r *Request) {
		q := r.FormValue("q")
		system(q)
	})
	GET("/ok", func(w ResponseWriter, r *Request) {
		render(w)
	})
}
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert_eq!(mined.len(), 2, "both closures mined: {mined:?}");
        assert!(
            mined.iter().all(|e| e.contains("@srv.go:")),
            "position-qualified: {mined:?}"
        );
        // Cross-file ambiguity guard: another file with an `<anon>` closure
        // at the SAME line numbers must not be mined — name+line resolution
        // is constrained to the MethodRef's own file.
        let cpg2 = build_go(&[
            (
                "srv.go",
                r#"package main
func main() {
	GET("/q", func(w ResponseWriter, r *Request) {
		system(r.FormValue("q"))
	})
}
"#,
            ),
            (
                "other.go",
                r#"package main
func helper() {
	walk(func(w ResponseWriter, r *Request) {
		render(w)
	})
}
"#,
            ),
        ]);
        let mined2 = crate::entries::mine_registration_entries(&cpg2);
        assert_eq!(mined2.len(), 1, "{mined2:?}");
        assert!(mined2[0].contains("@srv.go:"), "{mined2:?}");
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods_registered = mined.iter().cloned().collect();
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        // Census: the two closures must land as DISTINCT rows keyed by the
        // positional spelling, not collapse into one `<anon>` row.
        let authz = std::collections::HashSet::new();
        let census = crate::authz_census(&cpg, &authz, &mined, &[]);
        assert_eq!(census.rows.len(), 2, "{census:?}");
        assert!(
            census.rows.iter().all(|(k, _)| k.contains("@srv.go:")),
            "{census:?}"
        );
    }

    #[test]
    fn mines_decorator_registration() {
        // FastAPI/Flask idiom: the registration call is a DECORATOR — the
        // handler is the decorated function below it, not an argument.
        let cpg = build_python(&[(
            "srv.py",
            r#"app = FastAPI()

@app.post("/score")
def score(req):
    run_udf(req.code)

@app.get("/health")
def health(req):
    return "ok"

def helper(x):
    return x
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert!(
            mined.iter().any(|e| e.contains("score")),
            "decorated handler mined: {mined:?}"
        );
        assert!(
            mined.iter().any(|e| e.contains("health")),
            "second decorated handler mined: {mined:?}"
        );
        assert!(
            !mined.iter().any(|e| e.contains("helper")),
            "unrelated function must not be mined: {mined:?}"
        );
    }

    #[test]
    fn registered_entry_bypasses_handler_shape_gate() {
        // A queue consumer's parameter type (*Message) fails the IDL
        // handler-shape gate — but a Subscribe registration is direct
        // evidence, so the registered tier must not require the shape.
        let cpg = build_go(&[(
            "consumer.go",
            r#"package main
func onMsg(m *Message) {
	system(m.Body)
}
func main() {
	Subscribe("topic", onMsg)
}
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert_eq!(mined, vec!["onMsg".to_string()]);
        let store = summarise(&cpg);
        let mut as_idl = TaintSpec::new(&[], &["system"]);
        as_idl.source_methods_guarded = mined.iter().cloned().collect();
        assert_eq!(
            find_flows(&cpg, &store, &as_idl).len(),
            0,
            "IDL tier's shape gate rejects *Message"
        );
        let mut as_registered = TaintSpec::new(&[], &["system"]);
        as_registered.source_methods_registered = mined.into_iter().collect();
        let findings = find_flows(&cpg, &store, &as_registered);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains("onMsg"), "{findings:?}");
    }

    #[test]
    fn census_idl_entries_respect_handler_shape_gate() {
        // The ml_service shape: catalog.proto declares `rpc Get(GetFeaturesReq)`.
        // The IDL name `Get` must census the real handler (param typed *Req)
        // but NOT the same-named internal utility (`counter.Get(key string)`).
        // A curated entry with the same name stays trusted verbatim.
        let cpg = build_go(&[(
            "svc.go",
            r#"package main
func (fs *featureStore) Get(ctx Context, req *GetFeaturesReq) (*GetFeaturesReply, error) {
	return fs.fetch(req.TableName)
}
func (c *counter) Get(key string) (int, error) {
	return c.db[key], nil
}
func (cs *crawlService) GetIssue(ctx Context, in *GetIssueRequestV2) (*GetIssueReplyV2, error) {
	return cs.handle(in)
}
"#,
        )]);
        let authz = std::collections::HashSet::new();
        let idl = vec!["Get".to_string(), "GetIssue".to_string()];
        let census = crate::authz_census(&cpg, &authz, &[], &idl);
        let names: Vec<&str> = census.rows.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("featureStore")),
            "handler-shaped Get must census: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("counter")),
            "utility Get must be shape-gated out: {names:?}"
        );
        assert!(
            names.iter().any(|n| n.contains("crawlService")),
            "versioned *RequestV2 handler must census: {names:?}"
        );
        // Same names as TRUSTED (curated/registered) entries: no shape gate,
        // all three methods census.
        let census2 = crate::authz_census(&cpg, &authz, &idl, &[]);
        assert_eq!(census2.rows.len(), 3, "{census2:?}");
    }

    #[test]
    fn len_sanitizer_kills_length_only_taint() {
        // The length-only-Sprintf FP family (`queries.Raw(qmarks(len(ids)))`
        // — placeholder string built from the SIZE of the tainted slice; an
        // int cannot smuggle SQL). Modeled as a pack-level sanitizer on
        // injection rules, NOT an engine hardcode: for memory-safety packs
        // an attacker-controlled size IS the attack, so those packs must
        // not list `len`.
        let cpg = build_go(&[(
            "q.go",
            r#"package main
func lookup() {
	ids := Getenv("IDS")
	q := qmarks(len(ids))
	Raw(q)
	Raw(ids)
}
"#,
        )]);
        let store = summarise(&cpg);
        let without = TaintSpec::new(&["Getenv"], &["Raw"]);
        assert_eq!(find_flows(&cpg, &store, &without).len(), 2, "both flows without sanitizer");
        let with_len = TaintSpec::with_sanitizers(&["Getenv"], &["Raw"], &["len"]);
        let findings = find_flows(&cpg, &store, &with_len);
        assert_eq!(findings.len(), 1, "length-only flow killed, direct flow kept: {findings:?}");
        assert!(
            findings[0].path.iter().any(|s| s.code.contains("Raw(ids)")),
            "the surviving flow is the direct one: {findings:?}"
        );
    }

    #[test]
    fn census_mines_framework_server_constructor_as_gate() {
        // A `service.NewGRPCServer(...)` call is module-local evidence of
        // the framework authn tier: reported as a `framework`-scope,
        // NON-enforcing gate — entry verdicts must not change.
        let cpg = build_go(&[(
            "run.go",
            r#"package main
func handleThing(ctx Context, req *ThingReq) (*ThingReply, error) {
	return doThing(req)
}
func main() {
	grpcServer, err := service.NewGRPCServer(cfg, opts)
	grpcServer.Serve()
}
"#,
        )]);
        let authz = std::collections::HashSet::new();
        let census = crate::authz_census(&cpg, &authz, &["handleThing".into()], &[]);
        assert!(
            census
                .gates
                .iter()
                .any(|g| g.scope == "framework" && g.name == "NewGRPCServer" && !g.enforcing),
            "{census:?}"
        );
        // Non-enforcing: the entry verdict stays none, not middleware@.
        assert_eq!(census.rows.len(), 1, "{census:?}");
        assert_eq!(census.rows[0].1, "none", "{census:?}");
    }

    #[test]
    fn verb_registration_requires_route_literal() {
        // `r.GET("/path", h)` is a router; `qb.Delete(table, cols)` is a
        // query builder. Same names — only the route-literal call mines.
        let cpg = build_go(&[(
            "verbs.go",
            r#"package main
func handleUsers(w ResponseWriter, r *Request) {
	system(r.Path)
}
func columnsOf(t *Table) []string { return t.cols }
func setup(r *Router, qb *Builder, tbl *Table) {
	r.GET("/users", handleUsers)
	qb.Delete(tbl, columnsOf)
}
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert_eq!(
            mined,
            vec!["handleUsers".to_string()],
            "route-literal GET mines, builder Delete does not: {mined:?}"
        );
    }

    #[test]
    fn miner_skips_ambiguous_names_and_test_files() {
        // Four same-named `Run` methods: a BARE-name reference is too
        // ambiguous to mine, but a member-value whose base is locally TYPED
        // (`a.Run`, `a *A`) names its method outright and resolves to
        // exactly that one. A handler defined in a mock file is never mined
        // at all.
        let cpg = build_go(&[
            (
                "tasks.go",
                r#"package main
func (a *A) Run(x *Job) { system(x.Cmd) }
func (b *B) Run(x *Job) { system(x.Cmd) }
func (c *C) Run(x *Job) { system(x.Cmd) }
func (d *D) Run(x *Job) { system(x.Cmd) }
func setup(s *Sched, a *A) {
	s.RegisterHandler(a.Run)
}
func wireAll(q *Queue) {
	q.Subscribe("bare", Run)
}
"#,
            ),
            (
                "mock_handler.go",
                r#"package main
func mockHandle(m *Message) { system(m.Body) }
func wire(q *Queue) {
	q.Subscribe("topic", mockHandle)
}
"#,
            ),
        ]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert_eq!(
            mined,
            vec!["A.Run".to_string()],
            "typed member-value resolves; bare ambiguous + mock-file skipped: {mined:?}"
        );
    }

    #[test]
    fn mines_method_value_and_middleware_wrap() {
        let cpg = build_go(&[(
            "routes.go",
            r#"package main
func (s *Server) handleFoo(w ResponseWriter, r *Request) {
	system(r.Path)
}
func handleBar(w ResponseWriter, r *Request) {
	system(r.Path)
}
func setup(s *Server, mux *Mux) {
	mux.Handle("/f", s.handleFoo)
	mux.HandleFunc("/b", wrap(handleBar))
}
"#,
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert!(
            mined.iter().any(|m| m.ends_with("handleFoo")),
            "method value s.handleFoo mined: {mined:?}"
        );
        assert!(
            mined.iter().any(|m| m.ends_with("handleBar")),
            "one-level middleware wrap mined: {mined:?}"
        );
        let store = summarise(&cpg);
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods_registered = mined.into_iter().collect();
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 2, "both handlers flow: {findings:?}");
    }

    /// Run the standard getenv→system query and return the single finding.
    fn one_finding(cpg: &Cpg, spec: &TaintSpec) -> Finding {
        let store = summarise(cpg);
        let findings = find_flows(cpg, &store, spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        findings.into_iter().next().unwrap()
    }

    #[test]
    fn authz_dominating_check_annotates_finding() {
        // The check runs on EVERY path to the sink (the condition is always
        // evaluated) — dominated, triage last.
        let cpg = build(&[(
            "a.c",
            "void f() {\n    char* q = getenv(\"Q\");\n    if (!check_permission(q)) { return; }\n    system(q);\n}\n",
        )]);
        let f = one_finding(&cpg, &TaintSpec::new(&["getenv"], &["system"]));
        assert!(
            f.authz.as_deref().is_some_and(|a| a.starts_with("authz-dominated@3")),
            "{f:?}"
        );
    }

    #[test]
    fn branch_only_authz_check_is_partial() {
        // The check exists but only on the admin branch: the other path
        // reaches the sink unchecked — the authz-bypass shape.
        let cpg = build(&[(
            "a.c",
            "void f(int admin) {\n    char* q = getenv(\"Q\");\n    if (admin) { check_permission(q); }\n    system(q);\n}\n",
        )]);
        let f = one_finding(&cpg, &TaintSpec::new(&["getenv"], &["system"]));
        assert!(
            f.authz.as_deref().is_some_and(|a| a.starts_with("authz-partial@")),
            "{f:?}"
        );
    }

    #[test]
    fn post_sink_authz_check_is_partial() {
        // Check placed AFTER the sink: present but cannot dominate.
        let cpg = build(&[(
            "a.c",
            "void f() {\n    char* q = getenv(\"Q\");\n    system(q);\n    check_permission(q);\n}\n",
        )]);
        let f = one_finding(&cpg, &TaintSpec::new(&["getenv"], &["system"]));
        assert!(
            f.authz.as_deref().is_some_and(|a| a.starts_with("authz-partial@")),
            "{f:?}"
        );
    }

    #[test]
    fn no_authz_check_leaves_annotation_empty() {
        let cpg = build(&[(
            "a.c",
            "void f() {\n    char* q = getenv(\"Q\");\n    system(q);\n}\n",
        )]);
        let f = one_finding(&cpg, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(f.authz, None, "{f:?}");
    }

    #[test]
    fn entry_method_gate_dominates_interprocedural_sink() {
        // The real-world handler shape: the gate lives in the entry method,
        // the sink several hops down in a helper. The sink's own method has
        // no check — the ENTRY anchor must find the dominance.
        let cpg = build(&[(
            "a.c",
            "void handle_req(char* q) {\n    if (!check_acl(q)) { return; }\n    helper(q);\n}\nvoid helper(char* p) {\n    system(p);\n}\n",
        )]);
        let mut spec = TaintSpec::new(&[], &["system"]);
        spec.source_methods.insert("handle_req".into());
        let f = one_finding(&cpg, &spec);
        assert!(f.method.contains("handle_req"), "{f:?}");
        assert!(
            f.authz.as_deref().is_some_and(|a| a.starts_with("authz-dominated@2")),
            "{f:?}"
        );
    }

    #[test]
    fn spec_authz_names_extend_the_heuristic() {
        // `gate_keeper` matches no built-in vocabulary — invisible without
        // the rule pack's authz list, dominated with it.
        let src = "void f() {\n    char* q = getenv(\"Q\");\n    if (!gate_keeper(q)) { return; }\n    system(q);\n}\n";
        let cpg = build(&[("a.c", src)]);
        let mut spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_eq!(one_finding(&cpg, &spec).authz, None, "not authz-shaped by name");
        spec.authz_methods.insert("gate_keeper".into());
        let f = one_finding(&cpg, &spec);
        assert!(
            f.authz.as_deref().is_some_and(|a| a.starts_with("authz-dominated@")),
            "{f:?}"
        );
    }

    #[test]
    fn confiner_call_on_path_annotates_confined() {
        // Taint reaches the sink only through a pack-declared confiner call
        // (the SSRF query-escape shape) — annotated, never suppressed.
        let src = "void f() {\n    char* q = getenv(\"Q\");\n    char* e = query_escape(q);\n    system(e);\n}\n";
        let cpg = build(&[("a.c", src)]);
        let mut spec = TaintSpec::new(&["getenv"], &["system"]);
        assert_eq!(one_finding(&cpg, &spec).confined, None, "no confiners declared");
        spec.confiners.insert("query_escape".into());
        let f = one_finding(&cpg, &spec);
        assert_eq!(f.confined.as_deref(), Some("confined@3:query_escape"), "{f:?}");
    }

    #[test]
    fn confiner_member_store_on_path_annotates_confined() {
        // The `u.rawquery = q` member store puts the taint into a confined
        // field of the object that reaches the sink (the
        // `parsedURL.RawQuery = query.Encode()` shape).
        let src = "void f(struct url* u) {\n    char* q = getenv(\"Q\");\n    u->rawquery = q;\n    system(u);\n}\n";
        let cpg = build(&[("a.c", src)]);
        let mut spec = TaintSpec::new(&["getenv"], &["system"]);
        spec.confiners.insert("rawquery".into());
        let f = one_finding(&cpg, &spec);
        assert_eq!(f.confined.as_deref(), Some("confined@3:rawquery"), "{f:?}");
    }

    #[test]
    fn confiner_matches_whole_call_names_not_substrings() {
        assert!(super::contains_call("query.Encode()", "Encode"));
        assert!(super::contains_call("Encode(v)", "Encode"));
        assert!(super::contains_call("url.QueryEscape (v)", "QueryEscape"));
        assert!(!super::contains_call("ReEncode(v)", "Encode"), "substring is not a call");
        assert!(!super::contains_call("EncodeBase64(v)", "Encode"), "prefix is not a call");
        assert!(!super::contains_call("x.Encode", "Encode"), "field read is not a call");
    }

    #[test]
    fn alias_after_member_store_carries_object_taint() {
        // The store happens FIRST, the alias second: plain rhs propagation
        // must hand cfg's object taint to y, and the field read through y
        // must fire.
        let cpg = build(&[(
            "f.c",
            "void h() {\n    struct C cfg;\n    cfg.key = getenv(\"X\");\n    struct C *y = &cfg;\n    system(y->key);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].sink_line, Some(5), "{findings:?}");
    }

    #[test]
    fn field_read_accessor_summary_carries_param_flow() {
        // `get` returns a FIELD READ of its parameter — the member-read
        // lowering makes that a Call named "key" which has no summary, so
        // without field-read pass-through in summary computation the
        // param0→return flow is silently lost and the caller-side taint
        // dies at get() (empty summaries suppress the conservative
        // fallback by design).
        let cpg = build(&[(
            "g.c",
            "char* get(struct C *c) { return c->key; }\nvoid h() {\n    struct C cfg;\n    cfg.key = getenv(\"X\");\n    system(get(&cfg));\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].method.contains('h'), "{findings:?}");
    }

    #[test]
    fn serve_http_struct_handler_is_mined_as_entry() {
        // `mux.Handle("/x", &H{})` registers the TYPE — the mined entry
        // must be its ServeHTTP method (the http.Handler contract), both
        // for the composite-literal form and a locally-typed variable.
        // A struct without a ServeHTTP must mine nothing.
        let cpg = build_go(&[(
            "h.go",
            "package main\n\ntype H struct{}\n\nfunc (h *H) ServeHTTP(w http.ResponseWriter, r *http.Request) {\n\tprocess(r)\n}\n\ntype NoHandler struct{}\n\nfunc wire(mux *http.ServeMux) {\n\tmux.Handle(\"/x\", &H{})\n\tmux.Handle(\"/y\", &NoHandler{})\n\tvar h2 *H\n\tmux.Handle(\"/z\", h2)\n}\n",
        )]);
        let mined = crate::entries::mine_registration_entries(&cpg);
        assert!(
            mined.iter().any(|e| e == "H.ServeHTTP"),
            "H.ServeHTTP must be mined: {mined:?}"
        );
        assert!(
            !mined.iter().any(|e| e.contains("NoHandler")),
            "a type without ServeHTTP mines nothing: {mined:?}"
        );
    }

    #[test]
    fn wrapper_returning_source_result_originates_taint_at_call_site() {
        // readcfg() has NO param→return flow — it manufactures taint from a
        // source inside its body. Calling it must originate taint at the
        // call site; the unrelated wrapper must not fire for this spec.
        let cpg = build(&[(
            "w.c",
            "char* readcfg() { return getenv(\"X\"); }\nchar* readver() { return version(); }\nvoid handler() {\n    char* v = readcfg();\n    char* w = readver();\n    system(v);\n    printf(w);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system", "printf"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "only system(v) may report: {findings:?}");
        assert_eq!(findings[0].origin, "getenv");
        assert_eq!(findings[0].sink, "system");
        assert!(findings[0].method.contains("handler"), "{findings:?}");
        // Witness must include the buried source call spliced from readcfg.
        assert!(
            findings[0].path.iter().any(|s| s.code.contains("getenv") && s.depth > 0),
            "{:?}",
            findings[0].path
        );
    }

    #[test]
    fn wrapper_source_return_lifts_transitively() {
        // outer() returns inner()'s result which returns getenv's — two
        // summary levels between the source and the calling method.
        let cpg = build(&[(
            "t.c",
            "char* inner() { return getenv(\"X\"); }\nchar* outer() {\n    char* t = inner();\n    return t;\n}\nvoid handler() { system(outer()); }\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].origin, "getenv");
        assert!(findings[0].method.contains("handler"), "{findings:?}");
    }

    #[test]
    fn query_time_sanitizer_kills_wrapper_source_return() {
        // The summary store was computed WITHOUT sanitizers, so readcfg's
        // call-returns carry getenv raw — but the QUERY declares `clean` a
        // sanitizer, and the witness reconstruction must honour it: every
        // source→return path inside readcfg is laundered, so no lift.
        let cpg = build(&[(
            "q.c",
            "char* clean(char* s) { return s; }\nchar* readcfg() { return clean(getenv(\"X\")); }\nvoid handler() { system(readcfg()); }\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 0, "laundered wrapper must not lift: {findings:?}");
    }

    #[test]
    fn external_summary_call_return_originates_taint() {
        // vendorRead has no body anywhere — an external JSON declaration
        // says it returns getenv's result. The hop must be summary-only
        // with external provenance.
        let cpg = build(&[("e.c", "void handler() { system(vendorRead()); }\n")]);
        let mut store = summarise(&cpg);
        store
            .load_external_json(
                r#"[{"functionDeclaration": {"methodName": "vendorRead"},
                     "callReturns": [{"call": "getenv"}]}]"#,
            )
            .unwrap();
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].origin, "getenv");
        assert!(
            findings[0]
                .path
                .iter()
                .any(|s| matches!(&s.provenance, Provenance::ExternalSummary { callee_fqn } if callee_fqn == "vendorRead")),
            "{:?}",
            findings[0].path
        );
    }
}
