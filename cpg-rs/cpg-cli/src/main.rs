//! `cpg` — build a project and serve queries over a line-oriented JSON
//! protocol (roadmap item #4: a query surface decoupled from the host
//! language).
//!
//! Usage:
//!     cpg serve <dir> [--lang c|python]
//!
//! Builds a CPG for every matching source file under `<dir>`, then reads one
//! JSON request per line on stdin and writes one JSON response per line on
//! stdout. Any client language with a JSON library can drive it; wrapping the
//! same loop in a TCP/HTTP listener is transport plumbing, not architecture.
//!
//! Requests:
//!     {"cmd":"stats"}
//!     {"cmd":"methods","name":"main"}            (name optional)
//!     {"cmd":"calls","name":"strcpy"}            (name optional)
//!     {"cmd":"summary","fqn":"wrap"}
//!     {"cmd":"taint","sources":["getenv"],"sinks":["system"]}
//!     {"cmd":"update","path":"a.c","source":"int f(){}"}   (incremental!)
//!     {"cmd":"quit"}

use cpg_core::Query;
use cpg_analysis::standard_pipeline;
use cpg_incremental::{Project, UpdateOutcome};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "serve" {
        eprintln!("usage: cpg serve <dir> [--lang c|python]");
        std::process::exit(2);
    }
    let dir = &args[2];
    let lang = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("c");

    let (mut project, exts): (Project, &[&str]) = match lang {
        "python" => (
            Project::new(
                || Box::new(cpg_lang_python::PythonFrontend::new()),
                standard_pipeline(),
            ),
            &["py"],
        ),
        _ => (
            Project::new(
                || Box::new(cpg_lang_c::CFrontend::new()),
                standard_pipeline(),
            ),
            &["c", "h"],
        ),
    };

    // Collect sources.
    let mut sources: Vec<(String, String)> = Vec::new();
    collect_sources(std::path::Path::new(dir), exts, &mut sources);
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let stats = project.build(&refs);
    eprintln!(
        "built {} files in {:?} (parallel {:?}, merge {:?}); serving on stdin",
        refs.len(),
        stats.parse_build + stats.passes + stats.summaries,
        stats.parallel_frontend,
        stats.merge
    );

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&mut project, &req),
            Err(e) => json!({"error": format!("bad request: {e}")}),
        };
        if response.get("quit").is_some() {
            break;
        }
        let _ = writeln!(out, "{response}");
        let _ = out.flush();
    }
}

fn handle(p: &mut Project, req: &Value) -> Value {
    match req.get("cmd").and_then(|c| c.as_str()) {
        Some("stats") => json!({
            "nodes": p.cpg.live_count(),
            "methods": p.cpg.methods().len(),
            "calls": p.cpg.calls().len(),
            "summaries": p.summaries.len(),
        }),
        Some("methods") => {
            let methods = match req.get("name").and_then(|n| n.as_str()) {
                Some(name) => p.cpg.method_named(name),
                None => p.cpg.methods(),
            };
            let items: Vec<Value> = methods
                .iter()
                .map(|&m| {
                    json!({
                        "name": p.cpg.name_of(m),
                        "fullName": p.cpg.full_name_of(m),
                        "signature": p.cpg.signature_of(m),
                        "file": p.cpg.path_of(p.cpg.file_of(m)),
                        "line": p.cpg.line_of(m),
                        "parameters": p.cpg.parameters_of(m).len(),
                    })
                })
                .collect();
            json!({"methods": items})
        }
        Some("calls") => {
            let calls = match req.get("name").and_then(|n| n.as_str()) {
                Some(name) => p.cpg.calls_named(name),
                None => p.cpg.calls(),
            };
            let items: Vec<Value> = calls
                .iter()
                .map(|&c| {
                    json!({
                        "name": p.cpg.name_of(c),
                        "code": p.cpg.code_of(c),
                        "file": p.cpg.path_of(p.cpg.file_of(c)),
                        "line": p.cpg.line_of(c),
                        "resolved": p.cpg.call_target(c).is_some(),
                    })
                })
                .collect();
            json!({"calls": items})
        }
        Some("summary") => {
            let Some(fqn) = req.get("fqn").and_then(|n| n.as_str()) else {
                return json!({"error": "summary requires fqn"});
            };
            match p.summary_of(fqn) {
                Some(s) => {
                    let flows: Vec<String> = s
                        .flows
                        .iter()
                        .map(|f| format!("{:?} -> {:?}", f.from, f.to))
                        .collect();
                    json!({"fqn": fqn, "flows": flows})
                }
                None => json!({"error": format!("no summary for {fqn}")}),
            }
        }
        Some("taint") => {
            let parse = |key: &str| -> Vec<String> {
                req.get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let sources = parse("sources");
            let sinks = parse("sinks");
            let src_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
            let sink_refs: Vec<&str> = sinks.iter().map(|s| s.as_str()).collect();
            let findings: Vec<Value> = p
                .find_taint(&src_refs, &sink_refs)
                .iter()
                .map(|f| {
                    let path: Vec<Value> = f
                        .path
                        .iter()
                        .map(|s| json!({"code": s.code, "line": s.line}))
                        .collect();
                    json!({
                        "method": f.method,
                        "sink": f.sink,
                        "line": f.sink_line,
                        "origin": f.origin,
                        "path": path,
                    })
                })
                .collect();
            json!({"findings": findings})
        }
        Some("update") => {
            let (Some(path), Some(source)) = (
                req.get("path").and_then(|v| v.as_str()),
                req.get("source").and_then(|v| v.as_str()),
            ) else {
                return json!({"error": "update requires path and source"});
            };
            match p.update_file(path, source) {
                UpdateOutcome::Unchanged => json!({"updated": false}),
                UpdateOutcome::Rebuilt { files_reanalysed, summaries_recomputed } => json!({
                    "updated": true,
                    "filesReanalysed": files_reanalysed,
                    "summariesRecomputed": summaries_recomputed,
                }),
            }
        }
        Some("quit") => json!({"quit": true}),
        _ => json!({"error": "unknown cmd; one of stats|methods|calls|summary|taint|update|quit"}),
    }
}

fn collect_sources(dir: &std::path::Path, exts: &[&str], out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, exts, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if exts.contains(&ext) {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    out.push((path.to_string_lossy().into_owned(), src));
                }
            }
        }
    }
}
