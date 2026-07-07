//! `cpg` — build a project and serve queries over a line-oriented JSON
//! protocol.
//!
//! The `taint` request accepts `sources`, `sinks`, and optional `sanitizers`.

use cpg_core::{Cpg, Query};
use cpg_analysis::standard_pipeline;
use cpg_incremental::{Project, UpdateOutcome};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn make_project(lang: &str) -> (Project, &'static [&'static str]) {
    use cpg_lang_ts::TsFrontend;
    match lang {
        "python" => (
            Project::new(|| Box::new(TsFrontend::python()), standard_pipeline()),
            &["py"],
        ),
        "java" => (
            Project::new(|| Box::new(TsFrontend::java()), standard_pipeline()),
            &["java"],
        ),
        "go" => (
            Project::new(|| Box::new(TsFrontend::go()), standard_pipeline()),
            &["go"],
        ),
        "javascript" | "js" => (
            Project::new(|| Box::new(TsFrontend::javascript()), standard_pipeline()),
            &["js", "mjs", "cjs"],
        ),
        "ruby" | "rb" => (
            Project::new(|| Box::new(TsFrontend::ruby()), standard_pipeline()),
            &["rb"],
        ),
        "rust" | "rs" => (
            Project::new(|| Box::new(TsFrontend::rust()), standard_pipeline()),
            &["rs"],
        ),
        _ => (
            Project::new(|| Box::new(cpg_lang_c::CFrontend::new()), standard_pipeline()),
            &["c", "h"],
        ),
    }
}

fn build_project(dir: &str, lang: &str) -> Project {
    let (mut project, exts) = make_project(lang);
    let mut sources: Vec<(String, String)> = Vec::new();
    collect_sources(std::path::Path::new(dir), exts, &mut sources);
    let refs: Vec<(&str, &str)> = sources.iter().map(|(p, s)| (p.as_str(), s.as_str())).collect();
    let stats = project.build(&refs);
    eprintln!(
        "built {} files in {:?} (parallel {:?}, merge {:?})",
        refs.len(),
        stats.parse_build + stats.passes + stats.summaries,
        stats.parallel_frontend,
        stats.merge
    );
    project
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "serve" => serve(&args),
        "build" => build_and_save(&args),
        _ => {
            eprintln!(
                "usage (langs: c|python|java|go|javascript|ruby|rust):\n  \
                 cpg build <dir> -o <graph.cpg> [--lang L]   build and persist a CPG\n  \
                 cpg serve <dir> [--lang L]                   build then serve queries\n  \
                 cpg serve --load <graph.cpg>                 reopen a saved CPG and serve"
            );
            std::process::exit(2);
        }
    }
}

fn build_and_save(args: &[String]) {
    let Some(dir) = args.get(2) else {
        eprintln!("usage: cpg build <dir> -o <graph.cpg> [--lang c|python]");
        std::process::exit(2);
    };
    let out = flag(args, "-o").unwrap_or("graph.cpg");
    let lang = flag(args, "--lang").unwrap_or("c");
    let project = build_project(dir, lang);
    match project.cpg.save(out) {
        Ok(()) => {
            let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            eprintln!("saved {} nodes to {out} ({size} bytes)", project.cpg.live_count());
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            std::process::exit(1);
        }
    }
}

fn serve(args: &[String]) {
    let lang = flag(args, "--lang").unwrap_or("c");
    let mut project = if let Some(load) = flag(args, "--load") {
        let (mut p, _) = make_project(lang);
        match Cpg::load(load) {
            Ok(cpg) => {
                p.reopen(cpg);
                eprintln!("loaded {} nodes from {load}", p.cpg.live_count());
            }
            Err(e) => {
                eprintln!("load failed: {e}");
                std::process::exit(1);
            }
        }
        p
    } else {
        let Some(dir) = args.get(2).filter(|d| !d.starts_with("--")) else {
            eprintln!("usage: cpg serve <dir> [--lang c|python]  |  cpg serve --load <graph.cpg>");
            std::process::exit(2);
        };
        build_project(dir, lang)
    };
    eprintln!("serving on stdin");

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
                    let flows: Vec<Value> = s
                        .flows
                        .iter()
                        .map(|f| {
                            json!({
                                "from": format!("{:?}", f.from),
                                "to": format!("{:?}", f.to),
                                "labels": f.label_strings(),
                            })
                        })
                        .collect();
                    json!({"fqn": fqn, "flows": flows, "provenance": s.provenance_notes()})
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
            let sanitizers = parse("sanitizers");
            let src_refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
            let sink_refs: Vec<&str> = sinks.iter().map(|s| s.as_str()).collect();
            let sanitizer_refs: Vec<&str> = sanitizers.iter().map(|s| s.as_str()).collect();
            let spec = cpg_analysis::TaintSpec::with_sanitizers(&src_refs, &sink_refs, &sanitizer_refs);
            let findings: Vec<Value> = cpg_analysis::find_flows(&p.cpg, &p.summaries, &spec)
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
                        "labels": f.labels,
                        "provenance": f.provenance,
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
