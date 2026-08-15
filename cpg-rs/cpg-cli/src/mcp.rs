//! `cpg mcp`: a Model Context Protocol server over stdio, so AI agents can
//! drive the whole IRIS loop (build → apis → scan → coverage → slice/flow →
//! merge) through one self-contained binary on any repository.
//!
//! Transport: MCP stdio — newline-delimited JSON-RPC 2.0 on stdin/stdout,
//! logging on stderr. Hand-rolled on serde_json (the same shape as the
//! `cpg serve` loop); no SDK, no async runtime, no new dependencies.
//!
//! Register with an MCP client as: `cpg mcp --root <repo>`. Tools take
//! module paths relative to that root; CPGs build lazily into the versioned
//! cache and reload incrementally across calls.

use crate::rules::{RulePack, IRIS_PACKS};
use crate::workspace::{graph_content_digest, language_for_cpg, Workspace};
use cpg_core::Query;
use cpg_incremental::Project;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// The methodology document, embedded so the binary is the whole toolkit.
pub const METHODOLOGY: &str = include_str!("../../iris/METHODOLOGY.md");

const PROTOCOL_VERSION: &str = "2025-03-26";

struct Server {
    ws: Workspace,
    /// Loaded projects keyed by cpg path, invalidated when the file changes.
    projects: HashMap<PathBuf, (String, String, Project)>,
}

/// Run the server until stdin closes. `root` as in `Workspace::open`.
pub fn run(root: Option<&str>) -> std::io::Result<()> {
    let ws = Workspace::open(root).map_err(std::io::Error::other)?;
    eprintln!(
        "cpg mcp: root {}, cache {}",
        ws.root.display(),
        ws.cache.display()
    );
    let mut srv = Server {
        ws,
        projects: HashMap::new(),
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                respond(&json!({"jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("parse error: {e}")}}))?;
                continue;
            }
        };
        let Some(id) = msg.get("id").filter(|i| !i.is_null()).cloned() else {
            continue; // notification (e.g. notifications/initialized): no response
        };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = match srv.dispatch(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        respond(&reply)?;
    }
    Ok(())
}

fn respond(v: &Value) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, v).map_err(std::io::Error::other)?;
    out.write_all(b"\n")?;
    out.flush()
}

impl Server {
    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion")
                    .and_then(|v| v.as_str()).unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}, "resources": {}},
                "serverInfo": {"name": "cpg", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Code-property-graph security analysis over the workspace \
                    root. Read resource iris://methodology first: it is the loop these \
                    tools implement (build -> apis -> scan entry-driven -> coverage -> \
                    slice/flow triage -> merge). Module paths are relative to the root; \
                    '.' is the whole root.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_descriptors()})),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.call_tool(name, &args) {
                    Ok(v) => Ok(json!({"content": [{"type": "text",
                        "text": serde_json::to_string_pretty(&v).unwrap_or_default()}],
                        "isError": false})),
                    Err(e) => Ok(json!({"content": [{"type": "text", "text": e}],
                        "isError": true})),
                }
            }
            "resources/list" => {
                let mut resources = vec![json!({
                    "uri": "iris://methodology",
                    "name": "IRIS methodology",
                    "description": "The scanning loop these tools implement — read first",
                    "mimeType": "text/markdown",
                })];
                for (name, _) in IRIS_PACKS {
                    resources.push(json!({
                        "uri": format!("iris://packs/{name}"),
                        "name": format!("IRIS pack {name}"),
                        "description": "Curated rule pack; usable as scan `rules`, or as a template",
                        "mimeType": "application/json",
                    }));
                }
                Ok(json!({"resources": resources}))
            }
            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                let (text, mime) = if uri == "iris://methodology" {
                    (METHODOLOGY.to_string(), "text/markdown")
                } else if let Some(name) = uri.strip_prefix("iris://packs/") {
                    match IRIS_PACKS.iter().find(|(n, _)| *n == name) {
                        Some((_, json_text)) => (json_text.to_string(), "application/json"),
                        None => return Err((-32602, format!("no such pack: {name}"))),
                    }
                } else {
                    return Err((-32602, format!("unknown resource uri: {uri}")));
                };
                Ok(json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]}))
            }
            other => Err((-32601, format!("method not found: {other}"))),
        }
    }

    /// Build-or-reuse the CPG for the tool's target and load it as a project.
    /// Targets: `path` (module relative to root, built via the cache) or
    /// `cpg` (absolute path to an already-built .cpg, e.g. a merge output).
    fn project_for(&mut self, args: &Value) -> Result<(PathBuf, String), String> {
        let lang_arg = args.get("lang").and_then(|l| l.as_str());
        let (cpg_path, lang) = match args.get("cpg").and_then(|c| c.as_str()) {
            Some(cpg) => {
                let p = PathBuf::from(cpg);
                if !p.is_file() {
                    return Err(format!("no such cpg: {cpg}"));
                }
                let lang = language_for_cpg(&p, lang_arg)?;
                (p, lang)
            }
            None => {
                let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
                self.ws.ensure_cpg(path, lang_arg)?
            }
        };
        let identity = graph_content_digest(&cpg_path)?;
        let fresh = self
            .projects
            .get(&cpg_path)
            .is_some_and(|(digest, cached_lang, _)| *digest == identity && *cached_lang == lang);
        if !fresh {
            let cpg = cpg_core::Cpg::load(&cpg_path.to_string_lossy())
                .map_err(|e| format!("load failed: {e}"))?;
            let (mut project, _) = crate::make_project(&lang)?;
            project.reopen(cpg);
            self.projects
                .insert(cpg_path.clone(), (identity, lang.clone(), project));
        }
        Ok((cpg_path, lang))
    }

    fn project(&mut self, cpg_path: &PathBuf) -> &mut Project {
        &mut self
            .projects
            .get_mut(cpg_path)
            .expect("loaded by project_for")
            .2
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "build_cpg" => {
                let (cpg_path, lang) = self.project_for(args)?;
                let p = self.project(&cpg_path);
                Ok(json!({
                    "cpg": cpg_path.to_string_lossy(),
                    "lang": lang,
                    "nodes": p.cpg.live_count(),
                    "methods": p.cpg.methods().len(),
                    "calls": p.cpg.calls().len(),
                }))
            }
            "scan" => self.scan_tool(args),
            "taint" | "methods" | "calls" | "summary" => {
                // Direct pass-through to the serve request handler.
                let (cpg_path, _) = self.project_for(args)?;
                let mut req = args.clone();
                req["cmd"] = json!(name);
                Ok(crate::handle(self.project(&cpg_path), &req))
            }
            "flow" => self.flow_tool(args),
            "slice" => self.slice_tool(args),
            "apis" => {
                let (cpg_path, _) = self.project_for(args)?;
                let min_count =
                    args.get("min_count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let top = args.get("top").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
                let p = self.project(&cpg_path);
                let mut inv = crate::apis::inventory(&p.cpg, 3);
                inv.retain(|e| e.count >= min_count);
                inv.truncate(top);
                serde_json::to_value(&inv).map_err(|e| e.to_string())
            }
            "merge" => self.merge_tool(args),
            "list_rules" => {
                let iris: Vec<Value> = IRIS_PACKS
                    .iter()
                    .map(|(n, j)| {
                        let p = RulePack::from_json(j).expect("compiled-in pack parses");
                        let ids: Vec<&str> = p.rules.iter().map(|r| r.id.as_str()).collect();
                        json!({"name": format!("iris:{n}"), "rules": ids})
                    })
                    .collect();
                Ok(json!({
                    "builtinLangs": ["c", "cpp", "go", "java", "javascript", "python", "ruby", "rust", "scala"],
                    "irisPacks": iris,
                }))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    /// The full entry-driven scan: rules from `rules` (iris:<name>, an inline
    /// rules array, or the built-in pack for the language), entries from
    /// `entries`/`entry_glob` plus auto-discovered proto/thrift IDL plus
    /// registration mining. Returns findings grouped by rule and the
    /// coverage report that certifies zeros.
    fn scan_tool(&mut self, args: &Value) -> Result<Value, String> {
        let (cpg_path, lang) = self.project_for(args)?;
        let pack = match args.get("rules") {
            None => crate::rules::builtin_pack(&lang)
                .ok_or_else(|| format!("no built-in pack for {lang}; pass rules"))?,
            Some(Value::String(s)) => RulePack::resolve(s)?,
            Some(Value::Array(rules)) => {
                let parsed: Result<Vec<crate::rules::Rule>, _> =
                    serde_json::from_value(Value::Array(rules.clone()));
                RulePack {
                    rules: parsed.map_err(|e| format!("bad rules: {e}"))?,
                    entry_globs: vec![],
                    caller_context_markers: None,
                    framework_server_calls: None,
                }
            }
            Some(other) => {
                return Err(format!(
                "rules must be \"iris:<name>\", a rules-file path, or an inline array; got {other}"
            ))
            }
        };
        // IDL-mined entries — auto-discovery only when the target is a module
        // path (a bare .cpg has no directory to discover IDL under); explicit
        // `proto_dirs` (root-relative) work for both, covering services whose
        // IDL lives outside the module tree.
        let mut idl_entries: Vec<String> = Vec::new();
        if args.get("cpg").is_none() {
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            for dir in self.ws.proto_dirs(path) {
                crate::merge::rpc_names(std::path::Path::new(&dir), &mut idl_entries);
            }
        }
        if let Some(dirs) = args.get("proto_dirs").and_then(|v| v.as_array()) {
            for d in dirs.iter().filter_map(|x| x.as_str()) {
                let dir = self.ws.module_dir(d)?;
                crate::merge::rpc_names(&dir, &mut idl_entries);
            }
        }
        let camel: Vec<String> = idl_entries
            .iter()
            .map(|e| crate::merge::lc_first(e))
            .collect();
        idl_entries.extend(camel);
        idl_entries.sort();
        idl_entries.dedup();
        let mut services: Vec<crate::thrift::ThriftService> = Vec::new();
        if args.get("cpg").is_none() {
            for dir in self.ws.thrift_dirs() {
                crate::thrift::thrift_services(std::path::Path::new(&dir), &mut services);
            }
        }
        let str_list = |key: &str| -> Vec<String> {
            args.get(key)
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut curated: Vec<String> = str_list("entries");
        let p = self.project(&cpg_path);
        if !services.is_empty() {
            crate::thrift::resolve_extends(&mut services);
            curated.extend(crate::thrift::thrift_entries(&p.cpg, &services));
        }
        if let Some(pat) = args.get("entry_glob").and_then(|v| v.as_str()) {
            curated.extend(crate::entries_from_glob(&p.cpg, pat));
        }
        curated.sort();
        curated.dedup();
        let registered = cpg_analysis::mine_registration_entries(&p.cpg);
        let per_rule = crate::scan::run_pack_entry(p, &pack, &curated, &idl_entries, &registered);
        let mut grouped = serde_json::Map::new();
        let mut finding_methods: HashSet<String> = HashSet::new();
        let mut total = 0usize;
        for rf in &per_rule {
            let items: Vec<Value> = rf.findings.iter().map(crate::finding_json).collect();
            total += items.len();
            for f in &rf.findings {
                finding_methods.insert(f.method.clone());
            }
            grouped.insert(rf.rule.id.clone(), Value::Array(items));
        }
        let coverage = crate::coverage::coverage_report(
            &p.cpg,
            &curated,
            &idl_entries,
            &pack,
            &finding_methods,
        );
        Ok(json!({
            "cpg": cpg_path.to_string_lossy(),
            "rules": pack.rules.len(),
            "totalFindings": total,
            "findings": Value::Object(grouped),
            "entries": {"curated": curated.len(), "idl": idl_entries.len(),
                         "registered": registered.len()},
            "coverage": coverage,
        }))
    }

    /// Ad-hoc glob flow query, same recipe as `cpg flow`: call names matching
    /// `source_glob` are sources, methods matching it donate parameters as
    /// entries, call names matching `sink_glob` are sinks.
    fn flow_tool(&mut self, args: &Value) -> Result<Value, String> {
        let src = args
            .get("source_glob")
            .and_then(|v| v.as_str())
            .ok_or("flow requires source_glob")?;
        let sink = args
            .get("sink_glob")
            .and_then(|v| v.as_str())
            .ok_or("flow requires sink_glob")?;
        let sanitizers: Vec<String> = args
            .get("sanitizers")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let (cpg_path, _) = self.project_for(args)?;
        let p = self.project(&cpg_path);
        let cpg = &p.cpg;
        let sources: BTreeSet<String> = cpg
            .calls()
            .into_iter()
            .filter_map(|c| cpg.name_of(c))
            .filter(|n| crate::glob_match(src, n))
            .map(str::to_string)
            .collect();
        let sinks: BTreeSet<String> = cpg
            .calls()
            .into_iter()
            .filter_map(|c| cpg.name_of(c))
            .filter(|n| crate::glob_match(sink, n))
            .map(str::to_string)
            .collect();
        let entries: Vec<String> = cpg
            .methods()
            .into_iter()
            .filter(|&m| cpg.name_of(m).is_some_and(|n| crate::glob_match(src, n)))
            .filter_map(|m| cpg.full_name_of(m).map(str::to_string))
            .collect();
        let rule_json = json!({"rules": [{
            "id": "FLOW",
            "sources": sources,
            "sinks": sinks,
            "sanitizers": sanitizers,
        }]});
        let pack = RulePack::from_json(&rule_json.to_string())?;
        let per_rule = crate::scan::run_pack_entry(p, &pack, &entries, &[], &[]);
        let findings: Vec<Value> = per_rule
            .iter()
            .flat_map(|rf| rf.findings.iter())
            .map(crate::finding_json)
            .collect();
        Ok(json!({
            "sourceCalls": sources.len(),
            "entryMethods": entries.len(),
            "sinkCalls": sinks.len(),
            "findings": findings,
        }))
    }

    /// Backward slice from a sink call or a method:line location.
    fn slice_tool(&mut self, args: &Value) -> Result<Value, String> {
        use crate::slice::{backward_slice, criterion_calls, criterion_location, slice_json};
        let (cpg_path, _) = self.project_for(args)?;
        let line = args.get("line").and_then(|v| v.as_u64()).map(|l| l as u32);
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let max_nodes = args.get("max").and_then(|v| v.as_u64()).unwrap_or(5000) as usize;
        let p = self.project(&cpg_path);
        let cpg = &p.cpg;
        let criteria = if let Some(call) = args.get("call").and_then(|v| v.as_str()) {
            criterion_calls(
                cpg,
                call,
                args.get("file").and_then(|v| v.as_str()),
                line,
                args.get("method").and_then(|v| v.as_str()),
            )
        } else if let (Some(m), Some(l)) = (args.get("method").and_then(|v| v.as_str()), line) {
            criterion_location(cpg, m, l)
        } else {
            return Err("slice requires call, or method + line".to_string());
        };
        if criteria.is_empty() {
            return Err("no matching slice criterion in the graph".to_string());
        }
        let (entries, truncated) = backward_slice(cpg, &criteria, depth, max_nodes);
        Ok(slice_json(cpg, &criteria, &entries, truncated))
    }

    /// Merge module CPGs (built via the cache) with gRPC/thrift stitching;
    /// returns the merged cpg path, scannable via the scan tool's `cpg` arg.
    fn merge_tool(&mut self, args: &Value) -> Result<Value, String> {
        use crate::merge::{link_rpcs, relink_calls, rpc_names};
        let out_name = args
            .get("out_name")
            .and_then(|v| v.as_str())
            .ok_or("merge requires out_name")?;
        let out = self.ws.merge_output_path(out_name)?;
        let paths: Vec<String> = args
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return Err("merge requires paths (module paths relative to the root)".to_string());
        }
        let mut merged = cpg_core::Cpg::new();
        let mut rpcs: Vec<String> = Vec::new();
        for rel in &paths {
            let (cpg_path, _) = self.ws.ensure_cpg(rel, None)?;
            let donor = cpg_core::Cpg::load(&cpg_path.to_string_lossy())
                .map_err(|e| format!("load {}: {e}", cpg_path.display()))?;
            let _ = donor.methods();
            merged.absorb(donor);
            for dir in self.ws.proto_dirs(rel) {
                rpc_names(std::path::Path::new(&dir), &mut rpcs);
            }
        }
        relink_calls(&mut merged);
        rpcs.sort();
        rpcs.dedup();
        let mut rpc_edges = 0usize;
        if !rpcs.is_empty() {
            let (added, _skipped) = link_rpcs(&mut merged, &rpcs);
            rpc_edges = added;
        }
        let mut services: Vec<crate::thrift::ThriftService> = Vec::new();
        for dir in self.ws.thrift_dirs() {
            crate::thrift::thrift_services(std::path::Path::new(&dir), &mut services);
        }
        let mut thrift_edges = 0usize;
        if !services.is_empty() {
            crate::thrift::resolve_extends(&mut services);
            let (added, _skipped) = crate::thrift::link_thrift(&mut merged, &services);
            thrift_edges = added;
        }
        merged
            .save(&out.to_string_lossy())
            .map_err(|e| format!("save failed: {e}"))?;
        Ok(json!({
            "cpg": out.to_string_lossy(),
            "nodes": merged.live_count(),
            "rpcEdges": rpc_edges,
            "thriftEdges": thrift_edges,
            "inputs": paths.len(),
        }))
    }
}

fn tool_descriptors() -> Vec<Value> {
    let obj = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    let target = json!({
        "path": {"type": "string", "description":
            "module path relative to the workspace root ('.' = whole root); CPG builds/caches automatically"},
        "cpg": {"type": "string", "description":
            "absolute path to an already-built .cpg (e.g. a merge output) — alternative to path"},
        "lang": {"type": "string", "description":
            "c|cpp|go|java|javascript|python|ruby|rust|scala|typescript (auto-detected when omitted)"},
    });
    let with_target = |extra: Value| {
        let mut props = target.as_object().unwrap().clone();
        for (k, v) in extra.as_object().unwrap() {
            props.insert(k.clone(), v.clone());
        }
        Value::Object(props)
    };
    vec![
        json!({"name": "build_cpg",
            "description": "Build (or reuse from cache) the code property graph for a module; returns graph stats. Other tools do this implicitly.",
            "inputSchema": obj(with_target(json!({})), &[])}),
        json!({"name": "scan",
            "description": "Entry-driven taint scan. rules: 'iris:<name>' | rules-file path | inline rule array | omitted for the language's built-in pack. Entries come from `entries`, `entry_glob`, auto-discovered gRPC/thrift IDL, and registration mining. Returns findings grouped by rule plus the coverage report that certifies zeros.",
            "inputSchema": obj(with_target(json!({
                "rules": {"description": "'iris:<name>', a rules-file path, or an inline array of rule objects"},
                "entries": {"type": "array", "items": {"type": "string"},
                    "description": "entry-point method names whose parameters are attacker-controlled"},
                "entry_glob": {"type": "string",
                    "description": "NAMEPAT[@FILEPAT] glob over method full names (and file paths)"},
                "proto_dirs": {"type": "array", "items": {"type": "string"},
                    "description": "extra root-relative dirs to mine .proto RPC names from, for services whose IDL lives outside the module"},
            })), &[])}),
        json!({"name": "flow",
            "description": "Quick ad-hoc source->sink taint query with globs, no rules file: call names matching source_glob are sources (methods matching it donate parameters as entries), call names matching sink_glob are sinks.",
            "inputSchema": obj(with_target(json!({
                "source_glob": {"type": "string"},
                "sink_glob": {"type": "string"},
                "sanitizers": {"type": "array", "items": {"type": "string"}},
            })), &["source_glob", "sink_glob"])}),
        json!({"name": "taint",
            "description": "Taint query with explicit source/sink call-name lists (supports name@argpos, @recv, @out<k> spellings).",
            "inputSchema": obj(with_target(json!({
                "sources": {"type": "array", "items": {"type": "string"}},
                "sinks": {"type": "array", "items": {"type": "string"}},
            })), &["sources", "sinks"])}),
        json!({"name": "slice",
            "description": "Backward slice over reaching definitions from a sink call name (optionally filtered by file/line/method) or from a method+line location.",
            "inputSchema": obj(with_target(json!({
                "call": {"type": "string"}, "method": {"type": "string"},
                "file": {"type": "string"}, "line": {"type": "integer"},
                "depth": {"type": "integer"}, "max": {"type": "integer"},
            })), &[])}),
        json!({"name": "apis",
            "description": "External-API inventory (calls with no in-repo definition), ranked by use — the input for inferring a scan spec on an unfamiliar codebase.",
            "inputSchema": obj(with_target(json!({
                "min_count": {"type": "integer"}, "top": {"type": "integer"},
            })), &[])}),
        json!({"name": "methods",
            "description": "List methods (optionally by name — exact, or a glob when it contains * or ?, matched against simple and full names): full name, signature, file, line, parameter count.",
            "inputSchema": obj(with_target(json!({"name": {"type": "string"}})), &[])}),
        json!({"name": "calls",
            "description": "List call sites (optionally by callee name — exact, or a glob when it contains * or ?): code, file, line, resolved?, receiver/type hints.",
            "inputSchema": obj(with_target(json!({"name": {"type": "string"}})), &[])}),
        json!({"name": "summary",
            "description": "Interprocedural dataflow summary of a method by full name.",
            "inputSchema": obj(with_target(json!({"fqn": {"type": "string"}})), &["fqn"])}),
        json!({"name": "merge",
            "description": "Merge several modules' CPGs with gRPC/thrift boundary stitching (IDL auto-discovered). Returns the merged .cpg path — scan it via the scan tool's `cpg` argument.",
            "inputSchema": obj(json!({
                "out_name": {"type": "string"},
                "paths": {"type": "array", "items": {"type": "string"},
                    "description": "module paths relative to the workspace root"},
            }), &["out_name", "paths"])}),
        json!({"name": "list_rules",
            "description": "List compiled-in rule packs: built-in per-language defaults and the named IRIS packs.",
            "inputSchema": obj(json!({}), &[])}),
    ]
}
