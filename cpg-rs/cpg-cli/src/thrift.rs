//! Thrift service parsing and RPC stitching.
//!
//! The proto stitch (`merge::link_rpcs`) matches stubs by name alone, which
//! is safe for gRPC's generated lowerCamel stubs but unsound for thrift in a
//! codebase where `mkdir`/`read`/`write` each have many same-named methods.
//! Thrift stitching is therefore keyed on the *interface type*: the C++
//! generator emits `<Service>If` (and a `<Service>Null` no-op base) which is
//! not in the parsed tree, so
//!
//!   - a client call `file_service_client_->mkdir(..)` carries the receiver-type hint
//!     `FileServiceIf` (member/local declared type, resolved by
//!     CallGraphPass) and stays unresolved;
//!   - a handler is a TypeDecl whose base list contains `FileServiceIf` or
//!     `FileServiceNull`.
//!
//! Only that hint↔interface match creates a Call edge, so a bare libc
//! `mkdir(path)` never gets stitched into an RPC server.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ThriftService {
    pub name: String,
    pub extends: Option<String>,
    pub methods: Vec<String>,
}

/// Parse `service X [extends parent] { ret name(args) ... }` blocks out of
/// one .thrift source. Line-based with brace/paren depth tracking: method
/// names are the identifier before a `(` at paren depth 0 inside a service
/// body; `oneway`, return types, multi-line argument lists and `throws`
/// clauses all fall out of the depth rule.
pub fn parse_thrift(src: &str, out: &mut Vec<ThriftService>) {
    let src = strip_comments(src);
    // A `service` header may put `extends Parent` and `{` on later lines.
    let mut pending: Option<ThriftService> = None;
    let mut current: Option<ThriftService> = None;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    for line in src.lines() {
        let t = line.trim();
        if current.is_none() {
            if let Some(rest) = t
                .strip_prefix("service ")
                .or_else(|| t.strip_prefix("service\t"))
            {
                let name: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    pending = Some(ThriftService {
                        name,
                        extends: None,
                        methods: Vec::new(),
                    });
                }
            }
            if let Some(p) = &mut pending {
                if let Some(pos) = t.find("extends") {
                    let ext: String = t[pos + "extends".len()..]
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                        .collect();
                    if !ext.is_empty() {
                        p.extends = Some(ext);
                    }
                }
                if t.contains('{') {
                    current = pending.take();
                    brace_depth = 0;
                    paren_depth = 0;
                    // fall through: the `{` line may already hold a method
                }
            }
        }
        if current.is_none() {
            continue;
        }
        let mut ended = false;
        let mut word = String::new();
        let mut last_ident = String::new();
        for ch in t.chars() {
            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
                continue;
            }
            // A delimiter ends the current word — flush BEFORE handling the
            // delimiter, so `name(` sees `name` as the identifier.
            if !word.is_empty() {
                last_ident = std::mem::take(&mut word);
            }
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if brace_depth == 0 {
                        ended = true;
                        break;
                    }
                }
                '(' => {
                    if paren_depth == 0
                        && brace_depth == 1
                        && !last_ident.is_empty()
                        && last_ident != "throws"
                    {
                        if let Some(svc) = current.as_mut() {
                            svc.methods.push(last_ident.clone());
                        }
                    }
                    paren_depth += 1;
                }
                ')' => paren_depth = paren_depth.saturating_sub(1),
                _ => {}
            }
        }
        if ended {
            if let Some(svc) = current.take() {
                out.push(svc);
            }
        }
    }
}

/// `//`, `#`, and (stateful) `/* */` comments removed, strings left alone —
/// good enough for IDL, where string literals never contain `service {`.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    for line in src.lines() {
        let mut rest = line;
        let mut kept = String::new();
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(i) => {
                        in_block = false;
                        rest = &rest[i + 2..];
                    }
                    None => break,
                }
            } else {
                let line_c = match (rest.find("//"), rest.find('#')) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let block_c = rest.find("/*");
                match (line_c, block_c) {
                    (Some(l), Some(bl)) if l < bl => {
                        kept.push_str(&rest[..l]);
                        break;
                    }
                    (_, Some(bl)) => {
                        kept.push_str(&rest[..bl]);
                        in_block = true;
                        rest = &rest[bl + 2..];
                    }
                    (Some(l), None) => {
                        kept.push_str(&rest[..l]);
                        break;
                    }
                    (None, None) => {
                        kept.push_str(rest);
                        break;
                    }
                }
            }
        }
        out.push_str(&kept);
        out.push('\n');
    }
    out
}

/// All services declared under `dir` (recursive .thrift scan).
pub fn thrift_services(dir: &std::path::Path, out: &mut Vec<ThriftService>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            thrift_services(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("thrift") {
            if let Ok(src) = std::fs::read_to_string(&path) {
                parse_thrift(&src, out);
            }
        }
    }
}

/// Fold parent-service methods into children (`service A extends common.B`).
/// The `file.` include prefix is stripped: thrift service names are unique
/// enough in practice, and the stitch only needs the method list.
pub fn resolve_extends(services: &mut [ThriftService]) {
    let by_name: HashMap<String, (Option<String>, Vec<String>)> = services
        .iter()
        .map(|s| (s.name.clone(), (s.extends.clone(), s.methods.clone())))
        .collect();
    for svc in services.iter_mut() {
        let mut seen = std::collections::HashSet::new();
        seen.insert(svc.name.clone());
        let mut parent = svc.extends.clone();
        while let Some(p) = parent {
            let simple = p.rsplit('.').next().unwrap_or(&p).to_string();
            if !seen.insert(simple.clone()) {
                break; // cycle guard
            }
            let Some((grandparent, methods)) = by_name.get(&simple) else {
                break;
            };
            svc.methods.extend(methods.iter().cloned());
            parent = grandparent.clone();
        }
    }
}

/// Service name -> handler class names: live TypeDecls whose direct base
/// list contains `{S}If` or `{S}Null`. `Mock*`/`*Client` classes are skipped
/// — client-side wrappers subclass the interface too, but they forward to
/// the RPC stub rather than implement the service.
fn handler_classes<'a>(cpg: &Cpg, services: &'a [ThriftService]) -> HashMap<&'a str, Vec<String>> {
    let mut handlers: HashMap<&str, Vec<String>> = HashMap::new();
    for td in cpg.nodes_of_kind(NodeKind::TypeDecl) {
        if !cpg.is_live(td) {
            continue;
        }
        let Some(name) = cpg.name_of(td) else {
            continue;
        };
        if name.ends_with("Client") || name.starts_with("Mock") {
            continue;
        }
        let Some(bases) = cpg.signature_of(td) else {
            continue;
        };
        for base in bases.split(',') {
            for svc in services {
                if base == format!("{}If", svc.name) || base == format!("{}Null", svc.name) {
                    handlers
                        .entry(svc.name.as_str())
                        .or_default()
                        .push(name.to_string());
                }
            }
        }
    }
    for v in handlers.values_mut() {
        v.sort();
        v.dedup();
    }
    handlers
}

/// Stitch thrift client calls to handler methods. A handler for service `S`
/// is a TypeDecl whose direct base list contains `SIf` or `SNull` (skipping
/// `Mock*`/`*Client` classes — client wrappers subclass the interface too).
/// A client call is a call named `M` (M ∈ S's methods) whose receiver-type
/// hint is `SIf`. Fan-out is capped at `MAX_IMPLS` per (service, method).
pub fn link_thrift(cpg: &mut Cpg, services: &[ThriftService]) -> (usize, Vec<String>) {
    const MAX_IMPLS: usize = 8;
    // (class name, method name) -> method nodes.
    let mut methods_by_class: HashMap<(String, String), Vec<NodeId>> = HashMap::new();
    for m in cpg.methods() {
        if let (Some(cls), Some(name)) = (cpg.type_full_name_of(m), cpg.name_of(m)) {
            methods_by_class
                .entry((cls.to_string(), name.to_string()))
                .or_default()
                .push(m);
        }
    }
    let handlers = handler_classes(cpg, services);

    let mut added = 0;
    let mut skipped: Vec<String> = Vec::new();
    let mut new_edges: Vec<(NodeId, NodeId)> = Vec::new();
    for svc in services {
        let Some(classes) = handlers.get(svc.name.as_str()) else {
            continue;
        };
        let iface = format!("{}If", svc.name);
        let mut methods = svc.methods.clone();
        methods.sort();
        methods.dedup();
        for m in &methods {
            let impls: Vec<NodeId> = classes
                .iter()
                .flat_map(|c| {
                    methods_by_class
                        .get(&(c.clone(), m.clone()))
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect();
            if impls.is_empty() {
                continue;
            }
            if impls.len() > MAX_IMPLS {
                skipped.push(format!("{}.{m} ({} impls)", svc.name, impls.len()));
                continue;
            }
            for call in cpg.calls_named(m) {
                if cpg.type_full_name_of(call) != Some(iface.as_str()) {
                    continue;
                }
                for &target in &impls {
                    // The unique-candidate rung may already have resolved the
                    // call to this handler; don't double-edge it.
                    if !cpg.call_targets(call).contains(&target) {
                        new_edges.push((call, target));
                    }
                }
            }
        }
    }
    for (c, m) in new_edges {
        cpg.add_edge(c, m, EdgeKind::Call);
        added += 1;
    }
    (added, skipped)
}

/// Taint entry points implied by the services: every handler method that
/// exists in the graph, as a qualified `Class::method` name (the form
/// `cpg scan --entry` matches against method full names).
pub fn thrift_entries(cpg: &Cpg, services: &[ThriftService]) -> Vec<String> {
    let mut methods_by_class: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for m in cpg.methods() {
        if let (Some(cls), Some(name)) = (cpg.type_full_name_of(m), cpg.name_of(m)) {
            methods_by_class.insert((cls.to_string(), name.to_string()));
        }
    }
    let handlers = handler_classes(cpg, services);
    let mut out = Vec::new();
    for svc in services {
        let Some(classes) = handlers.get(svc.name.as_str()) else {
            continue;
        };
        for class in classes {
            for m in &svc.methods {
                if methods_by_class.contains(&(class.clone(), m.clone())) {
                    out.push(format!("{class}::{m}"));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
