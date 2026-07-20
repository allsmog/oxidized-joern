//! Call-graph pass (reads Ast, writes CallGraph).
//!
//! Resolves call sites to method declarations by name and emits `Call` edges.
//! This is the one inherently cross-file pass: a call in `a.c` may target a
//! method defined in `b.c`. When `a.c` changes we re-resolve its calls against
//! the project-maintained name index from the `PassContext` (falling back to a
//! one-shot scan when no context index is provided); the incremental driver
//! additionally re-runs this pass for callers when a *callee's* file changes
//! (see `cpg-incremental`).

use crate::pass::{ast_descendants, method_name_index, Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};

pub struct CallGraphPass;

impl Pass for CallGraphPass {
    fn name(&self) -> &'static str {
        "CallGraphPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::CallGraph]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Call)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, ctx: &PassContext) {
        let aux = ResolveAux::build(cpg);
        match ctx.methods_by_name {
            Some(index) => self.resolve_file(cpg, file, index, &aux),
            None => {
                let index = method_name_index(cpg);
                self.resolve_file(cpg, file, &index, &aux);
            }
        }
    }

    /// Without a context index, build the global method-name index once and
    /// resolve every file against it — O(methods + calls), not O(files × methods).
    fn run_batch(&self, cpg: &mut Cpg, files: &[FileId], ctx: &PassContext) {
        let aux = ResolveAux::build(cpg);
        match ctx.methods_by_name {
            Some(index) => {
                for &f in files {
                    self.resolve_file(cpg, f, index, &aux);
                }
            }
            None => {
                let index = method_name_index(cpg);
                for &f in files {
                    self.resolve_file(cpg, f, &index, &aux);
                }
            }
        }
    }
}

/// Whole-graph lookup tables for receiver-type resolution, built once per run.
struct ResolveAux {
    /// (owner class name, member name) -> declared member type. Keys whose
    /// type differs across same-named classes are dropped as ambiguous.
    member_types: std::collections::HashMap<(String, String), String>,
    /// Direct base class name -> subclass names, from TypeDecl base lists.
    /// Powers virtual dispatch: a call hinted at an interface fans out to
    /// the same-named methods of its implementing classes.
    subclasses: std::collections::HashMap<String, Vec<String>>,
    /// The inverse: class name -> its direct base names. Powers inherited-
    /// method resolution (`derived->m()` where `m` is defined on a base).
    bases: std::collections::HashMap<String, Vec<String>>,
    /// Files holding test/mock/fake code, by path convention. A production
    /// call site never dispatches into test doubles at runtime, so their
    /// methods are demoted as resolution candidates (empty when the
    /// CPG_RESOLVE_TEST_CODE escape hatch is set).
    test_files: std::collections::HashSet<FileId>,
}

/// Path-convention test-code detector, cross-language: test/mock directory
/// segments, `mock_`/`fake_`/`test_` basename prefixes, and `_test`/`.spec`
/// stem suffixes. Deliberately NOT camel `*Test`/`*Spec`/`*Mock` class-file
/// suffixes: JVM tests are directory-segregated (`src/test/` — the segment
/// rule catches them) and production domain classes legitimately use those
/// names (`DomainRecoverySpec.scala`, `SelfTest`-style product features).
pub(crate) fn is_test_path(path: &str) -> bool {
    const SEGMENTS: &[&str] = &[
        "test", "tests", "testing", "testdata", "mocks", "fakes", "__tests__", "__mocks__",
    ];
    let mut segs = path.split(['/', '\\']).filter(|s| !s.is_empty()).peekable();
    let mut base = "";
    while let Some(s) = segs.next() {
        let low = s.to_ascii_lowercase();
        if segs.peek().is_none() {
            base = s;
        } else if SEGMENTS.contains(&low.as_str())
            // Test-double servers (`k8s_simulator` faking the K8s API) —
            // same demotion class as mocks; mirrors the C++ excludes.
            || low.ends_with("simulator") || low.ends_with("simulators")
        {
            return true;
        }
    }
    let lower = base.to_ascii_lowercase();
    if lower.starts_with("mock_") || lower.starts_with("fake_") || lower.starts_with("test_") {
        return true;
    }
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.ends_with("_mock")
        || stem.ends_with("_fake")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
}

impl ResolveAux {
    fn build(cpg: &Cpg) -> Self {
        let mut member_types = std::collections::HashMap::new();
        let mut ambiguous = std::collections::HashSet::new();
        let mut subclasses: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut bases_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for n in cpg.nodes().collect::<Vec<_>>() {
            if !cpg.is_live(n) {
                continue;
            }
            match cpg.kind_of(n) {
                NodeKind::TypeDecl => {
                    if let (Some(name), Some(bases)) = (cpg.name_of(n), cpg.signature_of(n)) {
                        for base in bases.split(',').filter(|b| !b.is_empty()) {
                            subclasses.entry(base.to_string()).or_default().push(name.to_string());
                            bases_map.entry(name.to_string()).or_default().push(base.to_string());
                        }
                    }
                }
                NodeKind::Member => {
                    let Some(owner) = cpg
                        .in_kind(n, EdgeKind::Ast)
                        .find(|&o| cpg.kind_of(o) == NodeKind::TypeDecl)
                    else {
                        continue;
                    };
                    let (Some(cls), Some(name), Some(ty)) =
                        (cpg.name_of(owner), cpg.name_of(n), cpg.type_full_name_of(n))
                    else {
                        continue;
                    };
                    let key = (cls.to_string(), name.to_string());
                    match member_types.get(&key) {
                        Some(prev) if prev != ty => {
                            ambiguous.insert(key);
                        }
                        None => {
                            member_types.insert(key, ty.to_string());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for k in &ambiguous {
            member_types.remove(k);
        }
        let mut test_files = std::collections::HashSet::new();
        if std::env::var("CPG_RESOLVE_TEST_CODE").is_err() {
            for f in cpg.files() {
                if cpg.path_of(f).is_some_and(is_test_path) {
                    test_files.insert(f);
                }
            }
        }
        ResolveAux { member_types, subclasses, bases: bases_map, test_files }
    }

    /// All (transitive) subclasses of `base`, minus mocks. The set a call
    /// hinted at `base` may virtually dispatch into.
    fn implementors(&self, base: &str) -> std::collections::HashSet<String> {
        Self::closure(&self.subclasses, base, true)
    }

    /// All (transitive) base classes of `class`. Where an inherited method
    /// called through a derived-typed receiver may actually be defined.
    fn ancestors(&self, class: &str) -> std::collections::HashSet<String> {
        Self::closure(&self.bases, class, false)
    }

    fn closure(
        map: &std::collections::HashMap<String, Vec<String>>,
        start: &str,
        skip_mocks: bool,
    ) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        let mut stack = vec![start.to_string()];
        while let Some(k) = stack.pop() {
            for next in map.get(&k).into_iter().flatten() {
                if skip_mocks && next.starts_with("Mock") {
                    continue;
                }
                if out.insert(next.clone()) {
                    stack.push(next.clone());
                }
            }
        }
        out
    }
}

impl CallGraphPass {
    fn resolve_file(
        &self,
        cpg: &mut Cpg,
        file: FileId,
        index: &std::collections::HashMap<String, Vec<NodeId>>,
        aux: &ResolveAux,
    ) {
        let methods: Vec<NodeId> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
            .collect();

        for m in methods {
            let cls = cpg.type_full_name_of(m).map(str::to_string);
            for n in ast_descendants(cpg, m) {
                if cpg.kind_of(n) != NodeKind::Call {
                    continue;
                }
                let mut hint =
                    cpg.type_full_name_of(n).filter(|h| !h.is_empty()).map(str::to_string);
                if hint.is_none() {
                    // The frontend stamped the receiver's variable name into
                    // the signature column; if it names a field of the
                    // enclosing class, the field's declared type is the hint.
                    // Persist it onto the call so later stitch phases (which
                    // may run in a separate process on the saved graph) see it.
                    let member = match (&cls, cpg.signature_of(n)) {
                        (Some(c), Some(r)) => {
                            aux.member_types.get(&(c.clone(), r.to_string())).cloned()
                        }
                        _ => None,
                    };
                    if let Some(t) = member {
                        let sym = cpg.intern(&t);
                        cpg.set_type_full_name(n, sym);
                        hint = Some(t);
                    }
                }
                for t in Self::pick_targets(cpg, n, index, hint.as_deref(), aux) {
                    cpg.add_edge(n, t, EdgeKind::Call);
                }
            }
        }
    }

    /// Disambiguation ladder for a call with several same-named candidates.
    /// Returns the Call edges to add — usually one, several for virtual
    /// dispatch, none when resolving would be a guaranteed mislink.
    ///
    /// 1. receiver-type hint — the frontend stamped the call with its
    ///    receiver's locally known type; a method whose receiver/container
    ///    type matches wins.
    /// 2. inherited method — no method on the hinted type itself, but one on
    ///    a (transitive) base class: `derived->m()` where `m` lives upstream.
    /// 3. virtual dispatch — a method on the hinted type's subclasses:
    ///    calling through an interface/base fans out to the implementors,
    ///    capped at `MAX_VIRTUAL` (an interface with a mob of implementors
    ///    would wire half the graph together).
    /// 4. hint with no match anywhere in its hierarchy — leave unresolved.
    ///    We KNOW the receiver's type; any same-named method elsewhere would
    ///    be a mislink, and an unresolved call stays available for RPC
    ///    stitching (generated thrift/gRPC interfaces are exactly this case).
    /// 5. same file — a unique candidate in the call's own file wins (a
    ///    package-local helper beats a distant namesake).
    /// 6. arity — a unique candidate whose parameter count equals the
    ///    argument count wins (skipped when no candidate matches, so
    ///    variadics don't lose their only target).
    /// 7. fallback — the first candidate (the pre-existing behaviour).
    fn pick_targets(
        cpg: &Cpg,
        call: NodeId,
        index: &std::collections::HashMap<String, Vec<NodeId>>,
        hint: Option<&str>,
        aux: &ResolveAux,
    ) -> Vec<NodeId> {
        use cpg_core::Query;
        const MAX_VIRTUAL: usize = 16;
        let Some(name) = cpg.name_of(call) else { return Vec::new() };
        let Some(targets) = index.get(name) else { return Vec::new() };
        // A call site in production code never dispatches into test doubles
        // (mock_*.go, FakeStorageClient, *Spec.scala) at runtime, so test-file
        // candidates are dropped outright for production callers — even when
        // they are the ONLY candidates (the real implementation is then
        // outside this graph, and unresolved/external-stub is the honest
        // shape; resolving into the double fabricates a witness body). Test
        // call sites keep the full set.
        let demoted: Vec<NodeId>;
        let targets: &[NodeId] = if aux.test_files.contains(&cpg.file_of(call)) {
            targets
        } else {
            demoted = targets
                .iter()
                .copied()
                .filter(|&m| !aux.test_files.contains(&cpg.file_of(m)))
                .collect();
            &demoted
        };
        if targets.is_empty() {
            return Vec::new();
        }
        if targets.len() == 1 {
            return targets.to_vec();
        }
        if let Some(hint) = hint.filter(|h| !h.is_empty()) {
            let typed = targets
                .iter()
                .copied()
                .find(|&m| cpg.type_full_name_of(m) == Some(hint));
            if let Some(t) = typed {
                return vec![t];
            }
            let of_classes = |classes: &std::collections::HashSet<String>| -> Vec<NodeId> {
                targets
                    .iter()
                    .copied()
                    .filter(|&m| cpg.type_full_name_of(m).is_some_and(|t| classes.contains(t)))
                    .collect()
            };
            let inherited = of_classes(&aux.ancestors(hint));
            if !inherited.is_empty() && inherited.len() <= MAX_VIRTUAL {
                return inherited;
            }
            let dispatch = of_classes(&aux.implementors(hint));
            if !dispatch.is_empty() && dispatch.len() <= MAX_VIRTUAL {
                return dispatch;
            }
            return Vec::new();
        }
        let here = cpg.file_of(call);
        let local: Vec<NodeId> =
            targets.iter().copied().filter(|&m| cpg.file_of(m) == here).collect();
        if local.len() == 1 {
            return local;
        }
        let nargs = cpg.out_kind(call, EdgeKind::Argument).count();
        let arity: Vec<NodeId> = targets
            .iter()
            .copied()
            .filter(|&m| cpg.parameters_of(m).len() == nargs)
            .collect();
        if arity.len() == 1 {
            return arity;
        }
        targets.first().copied().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::{CpgBuilder, Cpg, Query};

    /// Two same-named handler methods in one graph, a caller whose class has
    /// a member of a known type, and one call dispatching through that
    /// member. Returns (cpg, call, method-of-matching-class).
    fn graph(member_type: &str) -> (Cpg, NodeId, NodeId) {
        let mut cpg = Cpg::new();
        let f1 = cpg.file_id("gateway_server.cpp");
        let (caller, call);
        {
            let mut b = CpgBuilder::new(&mut cpg, f1);
            let file = b.file_node("gateway_server.cpp");
            caller = b.method("mkdir", "GatewayServer::mkdir", "mkdir()", Some(10));
            b.contains(file, caller);
            let td = b.type_decl("GatewayServer", "GatewayServer", &["GatewayServerIf".into()], Some(1));
            b.contains(file, td);
            b.member(td, "file_service_client_", member_type);
            call = b.call("mkdir", "file_service_client_->mkdir(response, request)", Some(12));
            b.ast_child(caller, call);
        }
        let sym = cpg.intern("GatewayServer");
        cpg.set_type_full_name(caller, sym);
        let sym = cpg.intern("file_service_client_");
        cpg.set_signature(call, sym);

        let f2 = cpg.file_id("file_service_handler.cpp");
        let file_service_mkdir;
        {
            let mut b = CpgBuilder::new(&mut cpg, f2);
            let file = b.file_node("file_service_handler.cpp");
            file_service_mkdir = b.method("mkdir", "FileServiceHandler::mkdir", "mkdir()", Some(5));
            b.contains(file, file_service_mkdir);
            let other = b.method("mkdir", "PosixImpl::mkdir", "mkdir()", Some(50));
            b.contains(file, other);
            let sym = b.cpg.intern("PosixImpl");
            b.cpg.set_type_full_name(other, sym);
        }
        let sym = cpg.intern("FileServiceHandler");
        cpg.set_type_full_name(file_service_mkdir, sym);
        (cpg, call, file_service_mkdir)
    }

    fn resolve(cpg: &mut Cpg) {
        let files = cpg.files();
        CallGraphPass.run_batch(cpg, &files, &PassContext::empty());
    }

    #[test]
    fn member_hint_resolves_and_is_persisted() {
        // Member declared as a class that IS in the graph: hint wins the
        // ladder and gets stamped onto the call.
        let (mut cpg, call, file_service_mkdir) = graph("FileServiceHandler");
        resolve(&mut cpg);
        assert_eq!(cpg.call_target(call), Some(file_service_mkdir));
        assert_eq!(cpg.type_full_name_of(call), Some("FileServiceHandler"));
    }

    #[test]
    fn out_of_graph_interface_hint_stays_unresolved() {
        // Member declared as a generated interface with no methods in the
        // graph: leave the call unresolved (stitchable) instead of guessing
        // among same-named methods — but keep the persisted hint.
        let (mut cpg, call, _) = graph("FileServiceIf");
        resolve(&mut cpg);
        assert_eq!(cpg.call_target(call), None);
        assert_eq!(cpg.type_full_name_of(call), Some("FileServiceIf"));
    }

    #[test]
    fn interface_hint_fans_out_to_implementors() {
        // Member typed as an abstract interface that in-graph classes
        // subclass: virtual dispatch adds edges to every implementor's
        // same-named method (and only those).
        let (mut cpg, call, file_service_mkdir) = graph("FileServiceIf");
        {
            let f = cpg.file_id("file_service_handler.h");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("file_service_handler.h");
            let td = b.type_decl("FileServiceHandler", "FileServiceHandler", &["FileServiceIf".into()], Some(1));
            b.contains(file, td);
        }
        resolve(&mut cpg);
        assert_eq!(cpg.call_targets(call), vec![file_service_mkdir], "dispatch to the implementor only");
    }

    #[test]
    fn hint_with_no_match_in_hierarchy_stays_unresolved() {
        // Hint names an in-graph type (it has an `open`) with no `mkdir`
        // anywhere in its hierarchy: the receiver type is KNOWN, so falling
        // back to an unrelated same-named method would be a mislink.
        let (mut cpg, call, _) = graph("Cache");
        {
            let f = cpg.file_id("third.cpp");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("third.cpp");
            let m = b.method("open", "Cache::open", "open()", Some(3));
            b.contains(file, m);
            let sym = b.cpg.intern("Cache");
            b.cpg.set_type_full_name(m, sym);
        }
        resolve(&mut cpg);
        assert_eq!(cpg.call_target(call), None);
    }

    #[test]
    fn test_path_conventions() {
        for p in [
            "svc/testing/fake_storage_client.go",
            "event/api-handler/mock_event_query.go",
            "a/__tests__/b.ts",
            "pkg/foo_test.go",
            "src/test/scala/com/example/FooSpec.scala",
            "src/test/java/BarTest.java",
            "lib/baz.spec.ts",
        ] {
            assert!(is_test_path(p), "{p} should be test code");
        }
        for p in [
            "pkg/latest.go",
            "src/attestation.rs",
            "svc/protest_handler.go",
            "a/contested/b.go",
            "src/Testament.scala",
            // JVM production domain classes legitimately end in Spec/Test;
            // real JVM tests live under src/test/ (segment rule).
            "src/main/scala/com/example/domain/DomainRecoverySpec.scala",
            "src/main/scala/com/example/selftest/HardwareSelfTest.scala",
        ] {
            assert!(!is_test_path(p), "{p} should NOT be test code");
        }
    }

    /// Same-named method in a mock file and a production file: a production
    /// call site resolves to the production one; a test call site keeps the
    /// mock in play.
    #[test]
    fn production_calls_skip_test_file_candidates() {
        let mut cpg = Cpg::new();
        let (call, prod_m, mock_m, test_call);
        {
            let f = cpg.file_id("svc/service.go");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("svc/service.go");
            let caller = b.method("Run", "Run", "Run()", Some(1));
            b.contains(file, caller);
            call = b.call("Download", "gsClient.Download(x)", Some(2));
            b.ast_child(caller, call);
        }
        {
            let f = cpg.file_id("svc/gcs.go");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("svc/gcs.go");
            prod_m = b.method("Download", "Download", "Download()", Some(5));
            b.contains(file, prod_m);
        }
        {
            let f = cpg.file_id("svc/testing/fake_storage_client.go");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("svc/testing/fake_storage_client.go");
            mock_m = b.method("Download", "Download", "Download()", Some(9));
            b.contains(file, mock_m);
            let tc = b.method("TestDownload", "TestDownload", "TestDownload()", Some(20));
            b.contains(file, tc);
            test_call = b.call("Download", "Download(y)", Some(21));
            b.ast_child(tc, test_call);
        }
        resolve(&mut cpg);
        assert_eq!(
            cpg.call_targets(call),
            vec![prod_m],
            "production call must not resolve into the fake"
        );
        assert!(
            cpg.call_targets(test_call).contains(&mock_m)
                || cpg.call_targets(test_call).contains(&prod_m),
            "test-file call keeps resolving"
        );
    }

    /// The only same-named body in the graph is a test double (the real
    /// implementation lives outside this build): a production call stays
    /// unresolved rather than fabricating a witness body in the fake.
    #[test]
    fn production_call_with_only_test_candidates_stays_unresolved() {
        let mut cpg = Cpg::new();
        let call;
        {
            let f = cpg.file_id("svc/service.go");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("svc/service.go");
            let caller = b.method("Run", "Run", "Run()", Some(1));
            b.contains(file, caller);
            call = b.call("Download", "gsClient.Download(x)", Some(2));
            b.ast_child(caller, call);
        }
        {
            let f = cpg.file_id("testing/testinghelpers/fake_storage_client.go");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("testing/testinghelpers/fake_storage_client.go");
            let m = b.method("Download", "Download", "Download()", Some(9));
            b.contains(file, m);
        }
        resolve(&mut cpg);
        assert_eq!(cpg.call_target(call), None, "external-stub, not the fake");
    }

    #[test]
    fn inherited_method_resolves_through_base() {
        // `derived->mkdir()` where mkdir is defined on the base class: the
        // ancestors walk finds it.
        let (mut cpg, call, file_service_mkdir) = graph("DerivedServer");
        {
            let f = cpg.file_id("derived.h");
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("derived.h");
            let td =
                b.type_decl("DerivedServer", "DerivedServer", &["FileServiceHandler".into()], Some(1));
            b.contains(file, td);
        }
        resolve(&mut cpg);
        assert_eq!(cpg.call_targets(call), vec![file_service_mkdir], "resolved on the base class");
    }
}
