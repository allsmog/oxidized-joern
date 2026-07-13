//! Library surface of the `cpg` binary: project construction helpers, the
//! JSON request handler behind `cpg serve`, and the scan/rule/SARIF layer
//! (Gap 5). The binary in `main.rs` is a thin arg-parsing shell over this so
//! integration tests can exercise the exact production code paths.

pub mod rules;
pub mod sarif;
pub mod scan;

use cpg_analysis::standard_pipeline;
use cpg_core::{Cpg, Query};
use cpg_incremental::{Project, UpdateOutcome};
use serde_json::{json, Value};

/// Look up the value following a `--flag` in an argv slice.
pub fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

/// An empty project for `lang` plus the source-file extensions it owns.
pub fn make_project(lang: &str) -> (Project, &'static [&'static str]) {
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

/// Build a project by parsing every matching source file under `dir`.
pub fn build_project(dir: &str, lang: &str) -> Project {
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

/// Open a project the way `serve` and `scan` both do: `--load <graph.cpg>`
/// reopens a persisted CPG (skipping parsing), otherwise the positional
/// directory at `args[2]` is built from source.
pub fn open_project(args: &[String]) -> Result<Project, String> {
    let lang = flag(args, "--lang").unwrap_or("c");
    if let Some(load) = flag(args, "--load") {
        let (mut p, _) = make_project(lang);
        let cpg = Cpg::load(load).map_err(|e| format!("load failed: {e}"))?;
        p.reopen(cpg);
        eprintln!("loaded {} nodes from {load}", p.cpg.live_count());
        Ok(p)
    } else {
        let Some(dir) = args.get(2).filter(|d| !d.starts_with("--")) else {
            return Err("missing <dir> (or --load <graph.cpg>)".to_string());
        };
        Ok(build_project(dir, lang))
    }
}

pub fn collect_sources(dir: &std::path::Path, exts: &[&str], out: &mut Vec<(String, String)>) {
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

/// A taint finding as JSON — shared by the `taint` and `scan` commands.
fn finding_json(f: &cpg_analysis::Finding) -> Value {
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
}

/// Answer one JSON request against the project (the `cpg serve` loop body).
pub fn handle(p: &mut Project, req: &Value) -> Value {
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
            let findings: Vec<Value> =
                p.find_taint(&src_refs, &sink_refs).iter().map(finding_json).collect();
            json!({"findings": findings})
        }
        Some("scan") => {
            // Inline rule pack: {"cmd":"scan","rules":[{...},{...}]}. Same
            // rule schema as `cpg scan --rules`; findings come back grouped
            // by rule id.
            let Some(rules_val) = req.get("rules") else {
                return json!({"error": "scan requires rules (an inline array of rule objects)"});
            };
            let parsed: Result<Vec<rules::Rule>, _> = serde_json::from_value(rules_val.clone());
            let pack = match parsed {
                Ok(r) => rules::RulePack { rules: r },
                Err(e) => return json!({"error": format!("bad rules: {e}")}),
            };
            let per_rule = scan::run_pack(p, &pack);
            let mut grouped = serde_json::Map::new();
            for rf in &per_rule {
                let items: Vec<Value> = rf.findings.iter().map(finding_json).collect();
                grouped.insert(rf.rule.id.clone(), Value::Array(items));
            }
            json!({"findings": Value::Object(grouped)})
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
        _ => json!({"error": "unknown cmd; one of stats|methods|calls|summary|taint|scan|update|quit"}),
    }
}
