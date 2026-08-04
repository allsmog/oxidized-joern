//! Thrift parsing + stitching, against builder-constructed CPGs — the same
//! `cpg_cli::thrift` code paths `cpg merge --thrifts` runs in production.

use cpg_cli::thrift::{
    link_thrift, parse_thrift, resolve_extends, thrift_entries, ThriftService,
};
use cpg_core::{Cpg, CpgBuilder, NodeId, Query};

#[test]
fn parses_services_methods_and_oneway() {
    let src = r#"
// FUSE-style filesystem service.
namespace cpp filesvc

service FileService {
  common.Status mkdir(1: MkdirRequest request),
  GetAttrResponse getattr(1: GetAttrRequest request)
      throws (1: NotFound nf);
  oneway void ping();
  ReadResponse read(
      1: ReadRequest request,
      2: i64 offset,
  ) throws (1: IOError e),
}
"#;
    let mut out = Vec::new();
    parse_thrift(src, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "FileService");
    assert_eq!(out[0].extends, None);
    assert_eq!(out[0].methods, vec!["mkdir", "getattr", "ping", "read"]);
}

#[test]
fn strips_comments_including_hash_and_block() {
    let src = "
# service Fake1 { void nope(); }
/* service Fake2 {
   void alsoNope(); } */
service Real { void yes(); } // service Fake3 { void no(); }
";
    let mut out = Vec::new();
    parse_thrift(src, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "Real");
    assert_eq!(out[0].methods, vec!["yes"]);
}

#[test]
fn extends_on_following_line_and_chain() {
    let src = "
service Base { void ancestral(); }
service Mid extends other.Base { void middle(); }
service Leaf
    extends Mid {
  void own();
}
";
    let mut out = Vec::new();
    parse_thrift(src, &mut out);
    assert_eq!(out.len(), 3);
    let leaf = out.iter().find(|s| s.name == "Leaf").unwrap();
    assert_eq!(leaf.extends.as_deref(), Some("Mid"));
    resolve_extends(&mut out);
    let leaf = out.iter().find(|s| s.name == "Leaf").unwrap();
    // Transitive: own + middle + ancestral (the `other.` prefix is stripped).
    assert_eq!(leaf.methods, vec!["own", "middle", "ancestral"]);
}

/// CPG mirroring a generated C++ Thrift service shape: one handler subclassing
/// the generated If, a Mock and a Client subclass that must not stitch, a
/// hinted client call, and an unhinted libc-style call that must stay untouched.
fn stitch_graph() -> (Cpg, NodeId, NodeId, NodeId) {
    let mut cpg = Cpg::new();
    let f = cpg.file_id("all.cpp");
    let (handler_mkdir, hinted_call, bare_call);
    {
        let mut b = CpgBuilder::new(&mut cpg, f);
        let file = b.file_node("all.cpp");
        for (class, bases) in [
            ("FileServiceHandler", vec!["FileServiceIf".to_string()]),
            ("MockFileServiceHandler", vec!["FileServiceIf".to_string()]),
            ("GatewayFileServiceClient", vec!["FileServiceIf".to_string()]),
        ] {
            let td = b.type_decl(class, class, &bases, Some(1));
            b.contains(file, td);
        }
        handler_mkdir = b.method("mkdir", "FileServiceHandler::mkdir", "mkdir()", Some(10));
        b.contains(file, handler_mkdir);
        let p = b.parameter("request", "MkdirRequest", 1);
        b.ast_child(handler_mkdir, p);
        // Same-named methods on the excluded classes.
        for cls in ["MockFileServiceHandler", "GatewayFileServiceClient"] {
            let m = b.method("mkdir", &format!("{cls}::mkdir"), "mkdir()", Some(20));
            b.contains(file, m);
            let sym = b.cpg.intern(cls);
            b.cpg.set_type_full_name(m, sym);
        }
        let caller = b.method("HandleMkdir", "GatewayServer::HandleMkdir", "HandleMkdir()", Some(30));
        b.contains(file, caller);
        hinted_call = b.call("mkdir", "file_service_client_->mkdir(response, request)", Some(31));
        b.ast_child(caller, hinted_call);
        bare_call = b.call("mkdir", "mkdir(path, 0755)", Some(32));
        b.ast_child(caller, bare_call);
    }
    let sym = cpg.intern("FileServiceHandler");
    cpg.set_type_full_name(handler_mkdir, sym);
    let sym = cpg.intern("FileServiceIf");
    cpg.set_type_full_name(hinted_call, sym);
    (cpg, handler_mkdir, hinted_call, bare_call)
}

fn file_service() -> Vec<ThriftService> {
    vec![ThriftService {
        name: "FileService".to_string(),
        extends: None,
        methods: vec!["mkdir".to_string()],
    }]
}

#[test]
fn stitches_only_hinted_calls_to_real_handlers() {
    let (mut cpg, handler_mkdir, hinted_call, bare_call) = stitch_graph();
    let (added, skipped) = link_thrift(&mut cpg, &file_service());
    // Exactly one edge: hinted call -> FileServiceHandler::mkdir. Mock*/…Client
    // classes are not handlers, the unhinted call is not a client.
    assert_eq!(added, 1);
    assert!(skipped.is_empty());
    assert_eq!(cpg.call_targets(hinted_call), vec![handler_mkdir]);
    assert!(cpg.call_targets(bare_call).is_empty());
}

#[test]
fn stitch_is_idempotent_on_existing_edges() {
    let (mut cpg, handler_mkdir, hinted_call, _) = stitch_graph();
    cpg.add_edge(hinted_call, handler_mkdir, cpg_core::EdgeKind::Call);
    let (added, _) = link_thrift(&mut cpg, &file_service());
    assert_eq!(added, 0, "already-resolved edge must not be duplicated");
    assert_eq!(cpg.call_targets(hinted_call).len(), 1);
}

#[test]
fn entries_are_qualified_handler_methods() {
    let (cpg, _, _, _) = stitch_graph();
    let entries = thrift_entries(&cpg, &file_service());
    assert_eq!(entries, vec!["FileServiceHandler::mkdir"]);
}

#[test]
fn slice_walks_all_fanout_targets() {
    use cpg_cli::slice::backward_slice;
    // A call stitched to TWO handlers: the slice from the SECOND handler's
    // parameter must still reach the caller's argument (regression for the
    // first-Call-edge-only traversal).
    let mut cpg = Cpg::new();
    let f = cpg.file_id("fan.cpp");
    let (call, h2_param);
    {
        let mut b = CpgBuilder::new(&mut cpg, f);
        let file = b.file_node("fan.cpp");
        let caller = b.method("go", "go", "go()", Some(1));
        b.contains(file, caller);
        call = b.call("handle", "c->handle(req)", Some(2));
        b.ast_child(caller, call);
        let arg = b.identifier("req", Some(2));
        b.add_argument(call, arg, 0);
        let h1 = b.method("handle", "A::handle", "handle()", Some(10));
        b.contains(file, h1);
        let p1 = b.parameter("r", "Req", 0);
        b.ast_child(h1, p1);
        let h2 = b.method("handle", "B::handle", "handle()", Some(20));
        b.contains(file, h2);
        h2_param = b.parameter("r", "Req", 0);
        b.ast_child(h2, h2_param);
        b.cpg.add_edge(call, h1, cpg_core::EdgeKind::Call);
        b.cpg.add_edge(call, h2, cpg_core::EdgeKind::Call);
    }
    let (entries, truncated) = backward_slice(&cpg, &[h2_param], 2, 100);
    assert!(!truncated);
    let has_caller_arg = entries.iter().any(|e| e.code == "req" && e.line == Some(2));
    assert!(
        has_caller_arg,
        "slice from the second fan-out target must hop to the caller's argument; got {:?}",
        entries.iter().map(|e| (&e.code, e.line)).collect::<Vec<_>>()
    );
}
