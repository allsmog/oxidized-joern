//! Reaching-definitions / DDG pass (reads Ast + Cfg, writes Ddg).
//!
//! A port of the byte-parity-validated `reaching_def_flows` from
//! `joern-parity` (itself a verbatim reconstruction of Joern v4.0.555's
//! ReachingDefPass + DdgGenerator, validated FLOWS-byte-identical against the
//! oracle) onto the `cpg-core` graph. The algorithm, carried over verbatim:
//!
//! - **GEN**: parameters define themselves at method entry; a non-field-access
//!   CALL defines {itself} ∪ {its Call/Identifier arguments}
//!   (`ReachingDefTransferFunction.initGen`). The `isFieldAccess` exclusion
//!   uses Joern's *broad* set (member accesses + indirection + getElementPtr +
//!   sizeOf).
//! - **Lone identifiers** (`OptimizedReachingDefTransferFunction`): an
//!   identifier argument that is not a param/local, not used in any return,
//!   and unique by name across all call arguments, is removed from gen and
//!   later edged directly to the method exit.
//! - **KILL**: a call's gen kills other defs of the same variables; calls
//!   matching `isGenericMemberAccessName` (member accesses + addressOf +
//!   pointerShift) kill nothing.
//! - **Fixpoint** over the CFG, with the `ReachingDefFlowGraph` quirk that the
//!   first body node's predecessors are *replaced* by the method entry.
//! - **Edges** are then added by the DdgGenerator routines (entry-node edges,
//!   call sites with the DefaultSemantics operator flow table, expression
//!   blocks, returns, exit), each candidate gated through the
//!   `EdgeValidator.isValidEdge` logic and the `UsageAnalyzer.isUsing`
//!   access-path matching (sameVariable ∥ isContainer ∥ isPart ∥ isAlias, all
//!   exact string equality).
//!
//! Divergences forced by the simpler cpg-core schema / frontends:
//!
//! 1. No `METHOD_PARAMETER_OUT` nodes exist, so `addEdgesToMethodParameterOut`
//!    (param-out routing, paramIn -> paramOut chains) is dropped; defs live at
//!    exit still flow to `MethodReturn` via the exit routine.
//! 2. No `TYPE_REF` / `JUMP_TARGET` node kinds (never produced).
//! 3. `addEdgesToCapturedIdentifiersAndParameters` (the `<global>`
//!    method-ref capture linking) is dropped: the frontends build no
//!    `<global>` method and no captured closures.
//! 4. The frontends name operator calls by token (`"="`, `"+"`, `"&&"`, ...),
//!    not `<operator>.assignment`; `operator_semantics` therefore normalises
//!    tokens onto the Joern operator names before consulting the verbatim
//!    DefaultSemantics table. Unknown names stay pass-through, exactly like
//!    Joern's default for calls without semantics.
//! 5. `DISPATCH_TYPE=INLINED` macro calls don't exist, so the entry-node
//!    routine skips *all* calls (in Joern only non-INLINED calls are skipped).
//!
//! Edges are written with [`EdgeKind::ReachingDef`].

use crate::pass::{Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};

pub struct ReachingDefPass;

impl Pass for ReachingDefPass {
    fn name(&self) -> &'static str {
        "ReachingDefPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast, cpg_core::Layer::Cfg]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ddg]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        // The pass manager clears these per file before a re-run, keeping
        // incremental re-analysis idempotent.
        Some(EdgeKind::ReachingDef)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, _ctx: &PassContext) {
        self.run_batch(cpg, &[file], _ctx);
    }

    /// Flow computation is a pure per-method query; only the edge writes
    /// mutate the graph. Compute all methods' flows in parallel, then apply
    /// serially — same edges, same order within a method, as the serial loop.
    fn run_batch(&self, cpg: &mut Cpg, files: &[FileId], _ctx: &PassContext) {
        use rayon::prelude::*;
        let methods: Vec<NodeId> = files
            .iter()
            .flat_map(|&file| {
                cpg.nodes_in_file(file)
                    .iter()
                    .copied()
                    .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
                    .collect::<Vec<_>>()
            })
            .collect();
        let slow_log = std::env::var_os("CPG_SLOW_METHODS").is_some();
        let per_method: Vec<Vec<ReachingDefFlow>> = methods
            .par_iter()
            .map(|&m| {
                let t = std::time::Instant::now();
                let flows = reaching_def_flows(cpg, m);
                if slow_log && t.elapsed().as_millis() > 200 {
                    eprintln!(
                        "slow method {:?} ({:?})",
                        cpg.name_of(m).unwrap_or("?"),
                        t.elapsed()
                    );
                }
                flows
            })
            .collect();
        for flows in per_method {
            let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();
            for f in flows {
                // The generator can propose one edge via several routines
                // (e.g. return-use and exit); the graph keeps one copy.
                if seen.insert((f.src, f.dst)) {
                    cpg.add_edge(f.src, f.dst, EdgeKind::ReachingDef);
                }
            }
        }
    }
}

/// One reaching-def flow: `var` is the variable string the definition carries
/// (Joern's edge VARIABLE property; empty for entry edges).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReachingDefFlow {
    pub var: String,
    pub src: NodeId,
    pub dst: NodeId,
}

// ---- Joern v4.0.555 MemberAccess predicates (verbatim) -------------------

/// `MemberAccess.isFieldAccess` — the broad GEN exclusion set.
fn is_field_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.memberAccess"
            | "<operator>.indirectComputedMemberAccess"
            | "<operator>.indirectMemberAccess"
            | "<operator>.computedMemberAccess"
            | "<operator>.indirection"
            | "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
            | "<operator>.getElementPtr"
            | "<operator>.sizeOf"
    )
}

/// The UsageAnalyzer `containerSet`.
fn is_container_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
    )
}

/// The UsageAnalyzer `indirectionAccessSet`.
fn is_indirection_access(name: &str) -> bool {
    matches!(name, "<operator>.addressOf" | "<operator>.indirection")
}

/// `MemberAccess.isGenericMemberAccessName` — the KILL skip set (addressOf +
/// pointerShift instead of sizeOf).
fn is_generic_member_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.memberAccess"
            | "<operator>.indirectComputedMemberAccess"
            | "<operator>.indirectMemberAccess"
            | "<operator>.computedMemberAccess"
            | "<operator>.indirection"
            | "<operator>.addressOf"
            | "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
            | "<operator>.pointerShift"
            | "<operator>.getElementPtr"
    )
}

/// Access-path operators (`toTrackedBaseAndAccessPathSimple` candidates).
fn is_access_path_call(name: &str) -> bool {
    is_container_access(name)
        || is_indirection_access(name)
        || matches!(name, "<operator>.pointerShift" | "<operator>.getElementPtr")
}

/// The engine's frontends name operator calls by their source token; map
/// those onto the Joern operator names the semantics table is keyed by
/// (divergence 4 in the module docs). Non-operator names map to themselves.
fn normalize_operator(name: &str) -> &str {
    match name {
        "=" => "<operator>.assignment",
        "+=" => "<operator>.assignmentPlus",
        "-=" => "<operator>.assignmentMinus",
        "*=" => "<operator>.assignmentMultiplication",
        "/=" => "<operator>.assignmentDivision",
        "%=" => "<operators>.assignmentModulo",
        "&=" => "<operators>.assignmentAnd",
        "|=" => "<operators>.assignmentOr",
        "^=" => "<operators>.assignmentXor",
        "<<=" => "<operators>.assignmentShiftLeft",
        ">>=" => "<operators>.assignmentArithmeticShiftRight",
        "+" => "<operator>.addition",
        "&&" => "<operator>.logicalAnd",
        "||" => "<operator>.logicalOr",
        "?:" => "<operator>.conditional",
        "++" => "<operator>.postIncrement",
        "--" => "<operator>.postDecrement",
        other => other,
    }
}

/// Joern v4.0.555 DefaultSemantics operator flow mappings: (srcArgIdx,
/// dstArgIdx), dst -1 = return value. `None` = no explicit semantics
/// (pass-through: all flows valid). `Some(vec![])` = sizeOf (no flows).
/// Verbatim from the decompiled DefaultSemantics.operatorFlows().
pub fn operator_semantics(name: &str) -> Option<Vec<(i64, i64)>> {
    let name = normalize_operator(name);
    let compound = vec![(2, 1), (1, 1), (2, -1)];
    let access1 = vec![(1, -1)];
    let incdec = vec![(1, 1), (1, -1)];
    let v: Vec<(i64, i64)> = match name {
        "<operator>.addition" => vec![(1, -1), (2, -1)],
        "<operator>.addressOf" => access1,
        "<operator>.assignment" => vec![(2, 1), (2, -1)],
        "<operators>.assignmentAnd"
        | "<operators>.assignmentArithmeticShiftRight"
        | "<operator>.assignmentDivision"
        | "<operators>.assignmentExponentiation"
        | "<operators>.assignmentLogicalShiftRight"
        | "<operator>.assignmentMinus"
        | "<operators>.assignmentModulo"
        | "<operator>.assignmentMultiplication"
        | "<operators>.assignmentOr"
        | "<operator>.assignmentPlus"
        | "<operators>.assignmentShiftLeft"
        | "<operators>.assignmentXor" => compound,
        "<operator>.cast" => vec![(1, -1), (2, -1)],
        "<operator>.computedMemberAccess" => access1,
        "<operator>.conditional" => vec![(2, -1), (3, -1)],
        "<operator>.elvis" => vec![(1, -1), (2, -1)],
        "<operator>.notNullAssert" => access1,
        "<operator>.fieldAccess" => access1,
        "<operator>.getElementPtr" => access1,
        "<operator>.incBy" => vec![(1, 1), (2, 1), (3, 1), (4, 1)],
        "<operator>.indexAccess" => access1,
        "<operator>.indirectComputedMemberAccess" => access1,
        "<operator>.indirectFieldAccess" => access1,
        "<operator>.indirectIndexAccess" => vec![(1, -1), (2, 1)],
        "<operator>.indirectMemberAccess" => access1,
        "<operator>.indirection" => access1,
        "<operator>.memberAccess" => access1,
        "<operator>.pointerShift" => access1,
        "<operator>.postDecrement"
        | "<operator>.postIncrement"
        | "<operator>.preDecrement"
        | "<operator>.preIncrement" => incdec,
        "<operator>.sizeOf" => vec![],
        // Everything else (named calls, subtraction, comparisons, ...):
        // no explicit semantics = pass-through.
        _ => return None,
    };
    Some(v)
}

// ---- graph-shaped helpers over cpg-core -----------------------------------

/// A per-method view: the method's own nodes plus the lookups the generator
/// keeps asking for. Built once per method.
struct MethodView<'a> {
    cpg: &'a Cpg,
    /// AST descendants of the method, excluding nested Method subtrees.
    own: HashSet<NodeId>,
    /// Sorted argument lists of every Call in `own`, computed once — the
    /// usage/validity predicates ask for these O(uses x defs) times.
    call_args: HashMap<NodeId, Vec<NodeId>>,
}

impl<'a> MethodView<'a> {
    fn new(cpg: &'a Cpg, method: NodeId) -> Self {
        let mut own = HashSet::new();
        let mut stack = vec![method];
        while let Some(i) = stack.pop() {
            if !own.insert(i) {
                continue;
            }
            for c in cpg.out_kind(i, EdgeKind::Ast) {
                // A nested method is a separate reaching-def unit.
                if cpg.kind_of(c) == NodeKind::Method {
                    continue;
                }
                stack.push(c);
            }
        }
        let mut call_args = HashMap::new();
        for &i in &own {
            if cpg.kind_of(i) == NodeKind::Call {
                let mut v: Vec<NodeId> = cpg
                    .out_kind(i, EdgeKind::Argument)
                    .filter(|&k| cpg.kind_of(k) != NodeKind::FieldIdentifier)
                    .collect();
                v.sort_by_key(|&k| cpg.argument_index_of(k));
                call_args.insert(i, v);
            }
        }
        MethodView {
            cpg,
            own,
            call_args,
        }
    }

    fn kind(&self, n: NodeId) -> NodeKind {
        self.cpg.kind_of(n)
    }
    fn name(&self, n: NodeId) -> &str {
        self.cpg.name_of(n).unwrap_or("")
    }
    fn code(&self, n: NodeId) -> &str {
        self.cpg.code_of(n).unwrap_or("")
    }

    /// The enclosing call, when the node is directly a call's AST child.
    fn in_call(&self, n: NodeId) -> Option<NodeId> {
        self.cpg
            .in_kind(n, EdgeKind::Ast)
            .find(|&p| self.kind(p) == NodeKind::Call)
    }

    /// call.argument: Argument-edge children minus FieldIdentifier, sorted by
    /// argument index. (Receivers carry no Argument edge and are excluded —
    /// same as Joern's `call.argument` excluding the receiver-only edge.)
    /// Served from the per-method cache; a call outside `own` (never happens
    /// via the generator's paths) would return empty.
    fn args_of(&self, c: NodeId) -> &[NodeId] {
        self.call_args.get(&c).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn arg_index(&self, n: NodeId) -> i64 {
        self.cpg.argument_index_of(n) as i64
    }

    /// call.argument at a given index (`argumentOption(idx)`).
    fn arg_at(&self, c: NodeId, idx: i64) -> Option<NodeId> {
        self.args_of(c)
            .iter()
            .copied()
            .find(|&k| self.arg_index(k) == idx)
    }

    /// call.argument headOption (lowest index: the base of an access).
    fn head_arg(&self, c: NodeId) -> Option<NodeId> {
        // The cached list is sorted by argument index.
        self.args_of(c).first().copied()
    }

    /// DdgGenerator.nodeToEdgeLabel: parameters use their name, everything
    /// else its code; an empty expression block renders `<empty>`.
    fn node_var(&self, n: NodeId) -> String {
        match self.kind(n) {
            NodeKind::MethodParameterIn => self.name(n).to_string(),
            NodeKind::Block if self.code(n).is_empty() => "<empty>".to_string(),
            _ => self.code(n).to_string(),
        }
    }

    // ---- UsageAnalyzer.isUsing (v4.0.555) ----
    // nodeToString: Identifier/ParamIn -> NAME; Expression -> CODE; else None.
    // Borrows from the graph — this runs O(uses x defs) times per method, so
    // it must not allocate.
    fn node_str(&self, n: NodeId) -> Option<&'a str> {
        match self.kind(n) {
            NodeKind::Identifier | NodeKind::MethodParameterIn => {
                Some(self.cpg.name_of(n).unwrap_or(""))
            }
            NodeKind::Method
            | NodeKind::MethodReturn
            | NodeKind::ControlStructure
            | NodeKind::File
            | NodeKind::Namespace
            | NodeKind::TypeDecl
            | NodeKind::Member
            | NodeKind::Local => None,
            _ => Some(self.cpg.code_of(n).unwrap_or("")),
        }
    }

    /// sameVariable(use, inElement).
    fn same_var(&self, use_i: NodeId, in_i: NodeId) -> bool {
        let us = self.node_str(use_i);
        match self.kind(in_i) {
            NodeKind::MethodParameterIn | NodeKind::Identifier => us == Some(self.name(in_i)),
            NodeKind::Call => {
                if is_indirection_access(self.name(in_i)) {
                    match self.arg_at(in_i, 1) {
                        Some(op) => us == Some(self.code(op)),
                        None => false,
                    }
                } else {
                    us == Some(self.code(in_i))
                }
            }
            _ => false,
        }
    }

    /// isContainer(use, inElement): inElement is a container access (q.x) and
    /// its base equals the use (q).
    fn is_container(&self, use_i: NodeId, in_i: NodeId) -> bool {
        if self.kind(in_i) == NodeKind::Call && is_container_access(self.name(in_i)) {
            if let Some(base) = self.head_arg(in_i) {
                return self.node_str(use_i) == self.node_str(base);
            }
        }
        false
    }

    /// isPart(use, inElement): use is a container access and its base equals
    /// inElement (a param or identifier).
    fn is_part(&self, use_i: NodeId, in_i: NodeId) -> bool {
        if self.kind(use_i) == NodeKind::Call && is_container_access(self.name(use_i)) {
            if let Some(base) = self.head_arg(use_i) {
                let bs = self.node_str(base);
                return match self.kind(in_i) {
                    NodeKind::MethodParameterIn | NodeKind::Identifier => {
                        bs == Some(self.name(in_i))
                    }
                    _ => false,
                };
            }
        }
        false
    }

    /// isAlias(use, inElement): both are access-path calls with the same
    /// tracked base and exact-matching access path (equal code).
    fn is_alias(&self, use_i: NodeId, in_i: NodeId) -> bool {
        if self.kind(use_i) == NodeKind::Call
            && self.kind(in_i) == NodeKind::Call
            && is_access_path_call(self.name(use_i))
            && is_access_path_call(self.name(in_i))
        {
            return self.node_str(use_i) == self.node_str(in_i);
        }
        false
    }

    fn is_using(&self, use_i: NodeId, in_i: NodeId) -> bool {
        self.same_var(use_i, in_i)
            || self.is_container(use_i, in_i)
            || self.is_part(use_i, in_i)
            || self.is_alias(use_i, in_i)
    }

    // ---- EdgeValidator.isValidEdge (v4.0.555) ----
    fn is_expr(&self, n: NodeId) -> bool {
        matches!(
            self.kind(n),
            NodeKind::Call
                | NodeKind::Identifier
                | NodeKind::Literal
                | NodeKind::Block
                | NodeKind::FieldIdentifier
                | NodeKind::MethodRef
        )
    }
    fn sem(&self, c: NodeId) -> Option<Vec<(i64, i64)>> {
        if self.kind(c) == NodeKind::Call {
            operator_semantics(self.name(c))
        } else {
            None
        }
    }
    /// A call whose semantics never flow to the return value.
    fn is_call_retval(&self, n: NodeId) -> bool {
        if self.kind(n) != NodeKind::Call {
            return false;
        }
        match self.sem(n) {
            Some(m) => !m.iter().any(|&(_, d)| d == -1),
            None => false,
        }
    }
    fn is_used(&self, e: NodeId) -> bool {
        match self.in_call(e).and_then(|c| self.sem(c)) {
            Some(m) => m.iter().any(|&(s, _)| s == self.arg_index(e)),
            None => true,
        }
    }
    fn is_defined(&self, e: NodeId) -> bool {
        match self.in_call(e).and_then(|c| self.sem(c)) {
            Some(m) => m.iter().any(|&(_, d)| d == self.arg_index(e)),
            None => true,
        }
    }
    fn has_flow(&self, parent: NodeId, child: NodeId) -> bool {
        match self.in_call(parent).and_then(|c| self.sem(c)) {
            Some(m) => m.contains(&(self.arg_index(parent), self.arg_index(child))),
            None => true,
        }
    }
    fn same_call(&self, a: NodeId, b: NodeId) -> bool {
        let ca = self.in_call(a);
        ca.is_some() && ca == self.in_call(b)
    }
    fn valid_to_expr(&self, par: NodeId, cur: NodeId) -> bool {
        if self.is_expr(par) {
            let same = self.same_call(par, cur);
            (same && self.is_used(par) && self.is_defined(cur)) || (!same && self.is_used(cur))
        } else {
            self.is_used(cur)
        }
    }
    fn valid_edge(&self, child: NodeId, parent: NodeId) -> bool {
        if self.is_expr(child)
            && (self.is_call_retval(parent) || !self.valid_to_expr(parent, child))
        {
            return false;
        }
        if self.kind(child) == NodeKind::Call
            && self.is_expr(parent)
            && self.is_call_retval(child)
            && self.args_of(child).contains(&parent)
        {
            return false;
        }
        if self.is_expr(child) {
            if self.is_expr(parent) {
                if self.same_call(parent, child) && self.is_defined(child) && self.is_used(parent) {
                    return self.has_flow(parent, child);
                }
                return true;
            }
            return self.is_used(child);
        }
        !self.is_call_retval(parent)
    }
}

/// REACHING_DEF flows for one method, computed over the Cfg edges the CFG
/// pass produced. Pure query (no mutation) so tests and downstream analyses
/// can inspect the labelled flows; the pass materialises them as
/// [`EdgeKind::ReachingDef`] edges.
pub fn reaching_def_flows(cpg: &Cpg, method: NodeId) -> Vec<ReachingDefFlow> {
    if cpg.kind_of(method) != NodeKind::Method {
        return Vec::new();
    }
    let v = MethodView::new(cpg, method);
    let mut nodes: Vec<NodeId> = v.own.iter().copied().collect();
    nodes.sort(); // deterministic iteration/output order

    // --- uses helpers ---
    let uses_of = |i: NodeId| -> Vec<NodeId> {
        match v.kind(i) {
            NodeKind::Call => v.args_of(i).to_vec(),
            NodeKind::Return => cpg.out_kind(i, EdgeKind::Ast).collect(),
            _ => Vec::new(),
        }
    };
    let is_gen_arg = |k: NodeId| matches!(v.kind(k), NodeKind::Call | NodeKind::Identifier);

    // --- GEN / KILL ---
    // A def is identified by its node. Each def carries a variable string.
    let mut def_var: HashMap<NodeId, String> = HashMap::new();
    let mut gen: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let params: Vec<NodeId> = cpg
        .out_kind(method, EdgeKind::Ast)
        .filter(|&k| v.kind(k) == NodeKind::MethodParameterIn)
        .collect();
    let mut entry_gen: Vec<NodeId> = Vec::new();
    for &p in &params {
        def_var.insert(p, v.node_var(p));
        entry_gen.push(p);
    }
    let calls: Vec<NodeId> = nodes
        .iter()
        .copied()
        .filter(|&i| v.kind(i) == NodeKind::Call)
        .collect();
    // GEN excludes field-access calls (defsForCalls.filterNot(isFieldAccess)).
    let gen_calls: Vec<NodeId> = calls
        .iter()
        .copied()
        .filter(|&c| !is_field_access(v.name(c)))
        .collect();
    for &c in &gen_calls {
        // super.initGen: gen(call) = {call} ++ {Call|Identifier arguments}.
        let mut g = vec![c];
        def_var.insert(c, v.node_var(c));
        for &a in v.args_of(c) {
            if is_gen_arg(a) {
                def_var.insert(a, v.node_var(a));
                g.push(a);
            }
        }
        gen.insert(c, g);
    }
    // OptimizedReachingDefTransferFunction.withoutLoneIdentifiers.
    let mut lone_idents: Vec<NodeId> = Vec::new();
    {
        let mut name_excluded: HashSet<String> =
            params.iter().map(|&p| v.name(p).to_string()).collect();
        for &i in &nodes {
            if v.kind(i) == NodeKind::Local {
                name_excluded.insert(v.name(i).to_string());
            }
        }
        for &i in &nodes {
            if v.kind(i) == NodeKind::Return {
                let mut stack: Vec<NodeId> = cpg.out_kind(i, EdgeKind::Ast).collect();
                while let Some(x) = stack.pop() {
                    if v.kind(x) == NodeKind::Identifier {
                        name_excluded.insert(v.name(x).to_string());
                    }
                    stack.extend(cpg.out_kind(x, EdgeKind::Ast));
                }
            }
        }
        let mut by_name: HashMap<String, Vec<(NodeId, NodeId)>> = HashMap::new();
        for &c in &calls {
            for &a in v.args_of(c) {
                if v.kind(a) == NodeKind::Identifier && !name_excluded.contains(v.name(a)) {
                    by_name
                        .entry(v.name(a).to_string())
                        .or_default()
                        .push((c, a));
                }
            }
        }
        let mut lone: Vec<(NodeId, NodeId)> = by_name
            .values()
            .filter(|occ| occ.len() == 1)
            .map(|occ| occ[0])
            .collect();
        lone.sort();
        for (c, a) in lone {
            lone_idents.push(a);
            if let Some(g) = gen.get_mut(&c) {
                g.retain(|&x| x != a);
            }
        }
    }
    // kill(call) = other defs of the variables in gen(call); skipped for
    // generic-member-access calls (`&v` kills no prior def of v).
    // Served by a var -> defs index so kill construction is O(defs killed),
    // not O(calls x defs).
    let mut defs_of_var: HashMap<&String, Vec<NodeId>> = HashMap::new();
    for (d, var) in &def_var {
        defs_of_var.entry(var).or_default().push(*d);
    }
    let mut kill: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
    for &c in &gen_calls {
        if is_generic_member_access(v.name(c)) {
            continue;
        }
        let g: HashSet<NodeId> = gen[&c].iter().copied().collect();
        let k: HashSet<NodeId> = gen[&c]
            .iter()
            .flat_map(|d| defs_of_var[&def_var[d]].iter().copied())
            .filter(|d| !g.contains(d))
            .collect();
        kill.insert(c, k);
    }

    // --- dataflow fixpoint over the CFG ---
    // CFG edges within this method (plus the method entry node itself).
    let mut in_scope: HashSet<NodeId> = v.own.clone();
    in_scope.insert(method);
    let cfg: Vec<(NodeId, NodeId)> = in_scope
        .iter()
        .flat_map(|&s| {
            cpg.out_kind(s, EdgeKind::Cfg)
                .filter(|d| in_scope.contains(d))
                .map(move |d| (s, d))
        })
        .collect();
    // Nodes that are part of the CFG: used to tell an EXPRESSION block (comma
    // operator, a CFG node) from a statement block (not in the CFG).
    let cfg_nodes: HashSet<NodeId> = cfg.iter().flat_map(|&(s, d)| [s, d]).collect();
    let mut preds: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for &(s, d) in &cfg {
        preds.entry(d).or_default().push(s);
    }
    // ReachingDefFlowGraph quirk (decompiled initPred): the first body node's
    // predecessor is the method entry, REPLACING its CFG preds — this drops
    // loop back-edges into a condition that is also the first body node.
    for &(s, d) in &cfg {
        if s == method {
            preds.insert(d, vec![method]);
        }
    }
    let empty_p: Vec<NodeId> = Vec::new();
    // The def universe is exactly def_var's keys (params + gen members).
    // Dense def indices -> per-node bitsets, so the fixpoint is word-wise
    // OR/AND-NOT instead of per-element hashing — same sets, orders of
    // magnitude cheaper on methods with thousands of defs.
    let mut defs: Vec<NodeId> = def_var.keys().copied().collect();
    defs.sort();
    let def_idx: HashMap<NodeId, usize> = defs.iter().enumerate().map(|(i, &d)| (d, i)).collect();
    let words = defs.len().div_ceil(64);
    let mk_bits = |ids: &mut dyn Iterator<Item = NodeId>| -> Vec<u64> {
        let mut b = vec![0u64; words];
        for id in ids {
            let i = def_idx[&id];
            b[i / 64] |= 1u64 << (i % 64);
        }
        b
    };
    let entry_bits = mk_bits(&mut entry_gen.iter().copied());
    let gen_bits: HashMap<NodeId, Vec<u64>> = gen
        .iter()
        .map(|(&n, g)| (n, mk_bits(&mut g.iter().copied())))
        .collect();
    let kill_bits: HashMap<NodeId, Vec<u64>> = kill
        .iter()
        .map(|(&n, k)| (n, mk_bits(&mut k.iter().copied())))
        .collect();
    /// Decode a bitset back to node ids, ascending (defs is sorted).
    fn to_ids(bits: &[u64], defs: &[NodeId]) -> Vec<NodeId> {
        let mut v = Vec::new();
        for (wi, &word) in bits.iter().enumerate() {
            let mut w = word;
            while w != 0 {
                v.push(defs[wi * 64 + w.trailing_zeros() as usize]);
                w &= w - 1;
            }
        }
        v
    }
    let mut out: HashMap<NodeId, Vec<u64>> = HashMap::new();
    out.insert(method, entry_bits.clone());
    // Only nodes whose out-set is ever read matter: CFG participants (preds
    // values are CFG sources) plus the method entry. Iterating the rest just
    // recomputes sets nobody reads — on big methods that's most of the AST.
    let fixpoint_nodes: Vec<NodeId> = nodes
        .iter()
        .copied()
        .filter(|i| *i == method || cfg_nodes.contains(i))
        .collect();
    let mut changed = true;
    let mut in_bits = vec![0u64; words];
    while changed {
        changed = false;
        for &i in &fixpoint_nodes {
            in_bits.iter_mut().for_each(|w| *w = 0);
            for p in preds.get(&i).unwrap_or(&empty_p) {
                if let Some(po) = out.get(p) {
                    for (a, b) in in_bits.iter_mut().zip(po) {
                        *a |= b;
                    }
                }
            }
            let mut new_out = in_bits.clone();
            if let Some(k) = kill_bits.get(&i) {
                for (a, b) in new_out.iter_mut().zip(k) {
                    *a &= !b;
                }
            }
            if let Some(g) = gen_bits.get(&i) {
                for (a, b) in new_out.iter_mut().zip(g) {
                    *a |= b;
                }
            }
            if i == method {
                for (a, b) in new_out.iter_mut().zip(&entry_bits) {
                    *a |= b;
                }
            }
            if out.get(&i) != Some(&new_out) {
                out.insert(i, new_out);
                changed = true;
            }
        }
    }
    let in_of = |i: NodeId| -> Vec<NodeId> {
        let mut bits = vec![0u64; words];
        for p in preds.get(&i).unwrap_or(&empty_p) {
            if let Some(po) = out.get(p) {
                for (a, b) in bits.iter_mut().zip(po) {
                    *a |= b;
                }
            }
        }
        to_ids(&bits, &defs)
    };

    // method exit node.
    let exit = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&c| v.kind(c) == NodeKind::MethodReturn);

    // Defs live at exit come from the single `lastActualCfgNode` (the earliest
    // cfg-predecessor of METHOD_RETURN), not the union of all returns — the
    // ReachingDefFlowGraph param-out chain (see joern-parity).
    let exit_in: Vec<NodeId> = exit
        .and_then(|e| preds.get(&e).and_then(|ps| ps.iter().copied().min()))
        .and_then(|la| out.get(&la).map(|b| to_ids(b, &defs)))
        .unwrap_or_default();

    let mut flows: Vec<ReachingDefFlow> = Vec::new();
    // Gate every candidate addEdge(from=s, to=d) through isValidEdge(child=d,
    // parent=s), exactly as DdgGenerator.addEdge does.
    let push = |var: String, s: NodeId, d: NodeId, flows: &mut Vec<ReachingDefFlow>| {
        if v.valid_edge(d, s) {
            flows.push(ReachingDefFlow {
                var,
                src: s,
                dst: d,
            });
        }
    };

    // isDdgNode: everything except Method, ControlStructure, FieldIdentifier,
    // MethodReturn (no JumpTarget kind exists). A BLOCK counts only when it is
    // itself a CFG node (an expression block used as call argument).
    let is_ddg = |i: NodeId| match v.kind(i) {
        NodeKind::Call
        | NodeKind::Identifier
        | NodeKind::Literal
        | NodeKind::Return
        | NodeKind::MethodParameterIn
        | NodeKind::MethodRef => true,
        NodeKind::Block => cfg_nodes.contains(&i),
        _ => false,
    };

    // usedIncomingDefs(node): use -> the defs in in(node) it isUsing.
    let used_incoming = |i: NodeId| -> Vec<(NodeId, Vec<NodeId>)> {
        let ins = in_of(i);
        uses_of(i)
            .into_iter()
            .map(|u| {
                let mut ds: Vec<NodeId> =
                    ins.iter().copied().filter(|&d| v.is_using(u, d)).collect();
                ds.sort();
                (u, ds)
            })
            .collect()
    };

    // Write-only args: definition targets that are not reads under their
    // call's semantics (the LHS of plain `=`). No entry edge, not a use.
    let mut assign_lhs: HashSet<NodeId> = HashSet::new();
    for &c in &calls {
        if let Some(maps) = operator_semantics(v.name(c)) {
            for &a in v.args_of(c) {
                let idx = v.arg_index(a);
                let used = maps.iter().any(|&(s, _)| s == idx);
                let defined = maps.iter().any(|&(_, d)| d == idx);
                if defined && !used {
                    assign_lhs.insert(a);
                }
            }
        }
    }

    // 1. addEdgesFromEntryNode: a ddg node none of whose uses has a reaching
    // def gets method -> node. Calls never do (no INLINED calls exist here —
    // divergence 5); write-only targets are dropped by the validity gate.
    for &i in &nodes {
        if i == method || !is_ddg(i) || assign_lhs.contains(&i) {
            continue;
        }
        if v.kind(i) == NodeKind::Call {
            continue;
        }
        if used_incoming(i).iter().all(|(_, ds)| ds.is_empty()) {
            push(String::new(), method, i, &mut flows);
        }
    }

    // 2. call sites.
    for &c in &calls {
        let g_set: Vec<NodeId> = gen.get(&c).cloned().unwrap_or_default();
        // first loop: reaching defs into each arg use (assignment LHS is a
        // pure write target, not a use).
        for (u, ds) in used_incoming(c) {
            if assign_lhs.contains(&u) {
                continue;
            }
            for d in ds {
                if d != u {
                    push(v.node_var(d), d, u, &mut flows);
                }
            }
        }
        // second loop: every arg use -> every gen member, gated by the call's
        // flow semantics (arg -> call output is always valid; arg -> sibling
        // arg needs a (src,dst) mapping; pass-through when no semantics).
        let sem = operator_semantics(v.name(c));
        for &u in v.args_of(c) {
            if !is_ddg(u) {
                continue;
            }
            let u_idx = v.arg_index(u);
            for &gnode in &g_set {
                if u == gnode {
                    continue;
                }
                let valid = gnode == c
                    || match &sem {
                        None => true,
                        Some(maps) => maps.contains(&(u_idx, v.arg_index(gnode))),
                    };
                if valid {
                    push(v.node_var(u), u, gnode, &mut flows);
                }
            }
        }
        // addEdgeForBlock: an expression-block argument (comma operator)
        // routes its value — its last AST child — into the enclosing call.
        for &b in v.args_of(c) {
            if v.kind(b) != NodeKind::Block || !cfg_nodes.contains(&b) {
                continue;
            }
            let Some(last) = cpg.out_kind(b, EdgeKind::Ast).last() else {
                continue;
            };
            match v.kind(last) {
                NodeKind::Identifier => {
                    let ins = in_of(b);
                    let mut ds: Vec<NodeId> = ins
                        .iter()
                        .copied()
                        .filter(|&d| v.is_using(last, d))
                        .filter(|&d| matches!(v.kind(d), NodeKind::Identifier | NodeKind::Call))
                        .collect();
                    ds.sort();
                    let any = !ds.is_empty();
                    for d in ds {
                        push(v.node_var(d), d, b, &mut flows);
                    }
                    if any {
                        push(String::new(), b, c, &mut flows);
                    }
                }
                NodeKind::Call => {
                    push(v.node_var(last), last, b, &mut flows);
                    push(String::new(), b, c, &mut flows);
                }
                _ => {}
            }
        }
    }

    // 3. returns: defs -> return uses, uses -> RETURN, RETURN -> exit.
    for &i in &nodes {
        if v.kind(i) != NodeKind::Return {
            continue;
        }
        for (u, ins) in used_incoming(i) {
            push(v.code(u).to_string(), u, i, &mut flows);
            for d in ins {
                if d != u {
                    push(v.node_var(d), d, u, &mut flows);
                }
            }
        }
        if let Some(e) = exit {
            push("<RET>".into(), i, e, &mut flows);
        }
    }

    // (Joern routine 4, addEdgesToMethodParameterOut, is dropped: no
    // METHOD_PARAMETER_OUT nodes exist in this schema — divergence 1.)

    // 5. exit node: every def live at exit -> exit.
    if let Some(e) = exit {
        let mut ds: Vec<NodeId> = exit_in.to_vec();
        ds.sort();
        for d in ds {
            push(v.node_var(d), d, e, &mut flows);
        }
    }

    // 6. addEdgesFromLoneIdentifiersToExit.
    if let Some(e) = exit {
        let mut ds = lone_idents;
        ds.sort();
        for d in ds {
            push(v.node_var(d), d, e, &mut flows);
        }
    }

    flows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::CfgPass;
    use crate::pass::PassContext;
    use cpg_core::Cpg;
    use cpg_frontend::Frontend;

    /// Build a single C file, run CFG + reaching-def passes.
    fn build(src: &str) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_c::CFrontend::new();
        let r = fe.build_file(&mut cpg, "t.c", src);
        CfgPass.run_file(&mut cpg, r.file, &PassContext::empty());
        ReachingDefPass.run_file(&mut cpg, r.file, &PassContext::empty());
        cpg
    }

    /// Identifiers named `name`, in creation (== source) order.
    fn idents(cpg: &Cpg, name: &str) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = cpg
            .nodes()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some(name))
            .collect();
        v.sort();
        v
    }

    fn call_named(cpg: &Cpg, name: &str) -> NodeId {
        cpg.nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some(name))
            .unwrap_or_else(|| panic!("no call named {name:?}"))
    }

    fn has_rd_edge(cpg: &Cpg, src: NodeId, dst: NodeId) -> bool {
        cpg.out_kind(src, EdgeKind::ReachingDef).any(|d| d == dst)
    }

    #[test]
    fn def_reaches_use() {
        // x is defined by the assignment and used by sink(x).
        let cpg = build("void f() { x = source(); sink(x); }");
        let xs = idents(&cpg, "x");
        assert_eq!(xs.len(), 2, "lhs x and sink-arg x");
        let (x_def, x_use) = (xs[0], xs[1]);
        assert!(has_rd_edge(&cpg, x_def, x_use), "def x -> use x");
        // The use flows into its call's output node (arg -> call gen member).
        let sink = call_named(&cpg, "sink");
        assert!(has_rd_edge(&cpg, x_use, sink), "arg x -> sink call");
    }

    #[test]
    fn reassignment_kills_earlier_def() {
        let cpg = build("void f() { x = a(); x = b(); sink(x); }");
        let xs = idents(&cpg, "x");
        assert_eq!(xs.len(), 3);
        let (x1, x2, x_use) = (xs[0], xs[1], xs[2]);
        assert!(
            !has_rd_edge(&cpg, x1, x_use),
            "the first def of x is killed by the second assignment"
        );
        assert!(has_rd_edge(&cpg, x2, x_use), "the live def reaches the use");
    }

    #[test]
    fn def_flows_into_call_argument() {
        // A parameter definition reaches its use as a call argument, and the
        // argument taints the call's output.
        let cpg = build("void f(int p) { g(p); }");
        let param = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::MethodParameterIn && cpg.name_of(n) == Some("p"))
            .unwrap();
        let p_use = idents(&cpg, "p")[0];
        let g = call_named(&cpg, "g");
        assert!(has_rd_edge(&cpg, param, p_use), "param def -> argument use");
        assert!(has_rd_edge(&cpg, p_use, g), "argument -> call output");
    }

    #[test]
    fn assignment_semantics_gate_sibling_edges() {
        // For `y = x`, semantics are (2,1),(2,-1): rhs -> lhs and rhs -> call,
        // but never lhs -> rhs.
        let cpg = build("void f(int x) { y = x; sink(y); }");
        let assign = call_named(&cpg, "=");
        let y_lhs = idents(&cpg, "y")[0];
        let x_rhs = idents(&cpg, "x")[0];
        assert!(
            has_rd_edge(&cpg, x_rhs, y_lhs),
            "rhs -> lhs under assignment semantics"
        );
        assert!(!has_rd_edge(&cpg, y_lhs, x_rhs), "no lhs -> rhs flow");
        let _ = assign;
    }

    #[test]
    fn def_survives_branch_join() {
        // A def before an if reaches a use after it through both arms.
        let cpg = build("void f(int c) { x = source(); if (c) { a(); } else { b(); } sink(x); }");
        let xs = idents(&cpg, "x");
        assert_eq!(xs.len(), 2);
        assert!(
            has_rd_edge(&cpg, xs[0], xs[1]),
            "def flows across the branch join"
        );
    }

    #[test]
    fn branch_defs_both_reach_join_use() {
        // A def in each arm: both reach the use after the join.
        let cpg = build("void f(int c) { if (c) { x = a(); } else { x = b(); } sink(x); }");
        let xs = idents(&cpg, "x");
        assert_eq!(xs.len(), 3);
        let (x_then, x_else, x_use) = (xs[0], xs[1], xs[2]);
        assert!(
            has_rd_edge(&cpg, x_then, x_use),
            "then-arm def reaches the join use"
        );
        assert!(
            has_rd_edge(&cpg, x_else, x_use),
            "else-arm def reaches the join use"
        );
    }

    #[test]
    fn return_use_and_exit_edges() {
        let cpg = build("int f(int p) { return p; }");
        let method = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Method)
            .unwrap();
        let mret = cpg
            .out_kind(method, EdgeKind::Ast)
            .find(|&c| cpg.kind_of(c) == NodeKind::MethodReturn)
            .unwrap();
        let ret = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Return)
            .unwrap();
        let param = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::MethodParameterIn)
            .unwrap();
        let p_use = idents(&cpg, "p")[0];
        assert!(
            has_rd_edge(&cpg, param, p_use),
            "param def -> return operand"
        );
        assert!(has_rd_edge(&cpg, p_use, ret), "return operand -> RETURN");
        assert!(has_rd_edge(&cpg, ret, mret), "RETURN -> METHOD_RETURN");
    }

    #[test]
    fn rerun_is_idempotent() {
        let src = "void f() { x = source(); sink(x); }";
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_c::CFrontend::new();
        let r = fe.build_file(&mut cpg, "t.c", src);
        CfgPass.run_file(&mut cpg, r.file, &PassContext::empty());
        ReachingDefPass.run_file(&mut cpg, r.file, &PassContext::empty());
        let count = |cpg: &Cpg| -> usize {
            cpg.nodes()
                .map(|n| cpg.out_kind(n, EdgeKind::ReachingDef).count())
                .sum()
        };
        let first = count(&cpg);
        assert!(first > 0);
        // Clearing + re-running (what the pass manager does per file) must
        // reproduce exactly the same edge set.
        let live: Vec<NodeId> = cpg.nodes().collect();
        for n in live {
            cpg.remove_out_edges_of_kind(n, EdgeKind::ReachingDef);
        }
        ReachingDefPass.run_file(&mut cpg, r.file, &PassContext::empty());
        assert_eq!(count(&cpg), first, "re-run reproduces the same edges");
    }
}
