//! Library surface of the `cpg` binary: project construction helpers, the
//! JSON request handler behind `cpg serve`, and the scan/rule/SARIF layer
//! (Gap 5). The binary in `main.rs` is a thin arg-parsing shell over this so
//! integration tests can exercise the exact production code paths.

pub mod apis;
pub mod coverage;
pub mod export;
pub mod mcp;
pub mod merge;
pub mod play;
pub mod rules;
pub mod sarif;
pub mod scan;
pub mod slice;
pub mod thrift;
pub mod vectors;
pub mod workspace;

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

/// Collect every value of a repeatable `--flag` in an argv slice.
pub fn flags<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1))
        .map(|s| s.as_str())
        .collect()
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
        "scala" => (
            Project::new(|| Box::new(TsFrontend::scala()), standard_pipeline()),
            &["scala", "sc"],
        ),
        "typescript" | "ts" => (
            Project::new(|| Box::new(TsFrontend::typescript()), standard_pipeline()),
            &["ts", "tsx", "mts", "cts"],
        ),
        "cpp" | "c++" | "cxx" => (
            Project::new(|| Box::new(TsFrontend::cpp()), standard_pipeline()),
            &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"],
        ),
        _ => (
            Project::new(|| Box::new(cpg_lang_c::CFrontend::new()), standard_pipeline()),
            &["c", "h"],
        ),
    }
}

/// Build a project by parsing every matching source file under `dir`.
pub fn build_project(dir: &str, lang: &str) -> Project {
    build_project_filtered(dir, lang, &[])
}

/// Build a project, skipping any file whose path contains one of the
/// `excludes` substrings (vendored, generated, and test code have no place
/// in a security CPG and often dominate the file count).
pub fn build_project_filtered(dir: &str, lang: &str, excludes: &[&str]) -> Project {
    build_project_ext(dir, lang, excludes, None)
}

/// [`build_project_filtered`] plus optional external summaries JSON — loaded
/// BEFORE the build so computed summaries compose with the declared ones.
pub fn build_project_ext(
    dir: &str,
    lang: &str,
    excludes: &[&str],
    external_summaries: Option<&str>,
) -> Project {
    let (mut project, exts) = make_project(lang);
    load_externals(&mut project, external_summaries);
    let mut sources: Vec<(String, String)> = Vec::new();
    collect_sources_filtered(std::path::Path::new(dir), exts, excludes, &mut sources);
    let refs: Vec<(&str, &str)> = sources.iter().map(|(p, s)| (p.as_str(), s.as_str())).collect();
    let stats = project.build(&refs);
    eprintln!(
        "built {} files in {:?} (parallel {:?}, merge {:?}, passes {:?}, summaries {:?})",
        refs.len(),
        stats.parse_build + stats.passes + stats.summaries,
        stats.parallel_frontend,
        stats.merge,
        stats.passes,
        stats.summaries
    );
    project
}

/// Load `--summaries <file>` external-summary JSON into a project (no-op
/// when None). Must run before build/reopen so the summary fixpoint
/// composes with the declared entries.
fn load_externals(project: &mut Project, json: Option<&str>) {
    if let Some(json) = json {
        match project.load_external_summaries(json) {
            Ok(n) => eprintln!("loaded {n} external function summaries"),
            Err(e) => {
                eprintln!("--summaries: {e}");
                std::process::exit(2);
            }
        }
    }
}

/// Open a project the way `serve` and `scan` both do: `--load <graph.cpg>`
/// reopens a persisted CPG (skipping parsing), otherwise the positional
/// directory at `args[2]` is built from source. `--summaries <file>` loads
/// external function summaries (Fraunhofer-style JSON) either way.
pub fn open_project(args: &[String]) -> Result<Project, String> {
    let lang = flag(args, "--lang").unwrap_or("c");
    let ext_json: Option<String> = match flag(args, "--summaries") {
        Some(path) => Some(
            std::fs::read_to_string(path).map_err(|e| format!("--summaries {path}: {e}"))?,
        ),
        None => None,
    };
    if let Some(load) = flag(args, "--load") {
        let (mut p, _) = make_project(lang);
        load_externals(&mut p, ext_json.as_deref());
        let cpg = Cpg::load(load).map_err(|e| format!("load failed: {e}"))?;
        p.reopen(cpg);
        eprintln!("loaded {} nodes from {load}", p.cpg.live_count());
        Ok(p)
    } else {
        let Some(dir) = args.get(2).filter(|d| !d.starts_with("--")) else {
            return Err("missing <dir> (or --load <graph.cpg>)".to_string());
        };
        Ok(build_project_ext(dir, lang, &flags(args, "--exclude"), ext_json.as_deref()))
    }
}

pub fn collect_sources(dir: &std::path::Path, exts: &[&str], out: &mut Vec<(String, String)>) {
    collect_sources_filtered(dir, exts, &[], out)
}

pub fn collect_sources_filtered(
    dir: &std::path::Path,
    exts: &[&str],
    excludes: &[&str],
    out: &mut Vec<(String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if excludes.iter().any(|e| path_str.contains(e)) {
            continue;
        }
        if path.is_dir() {
            collect_sources_filtered(&path, exts, excludes, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if exts.contains(&ext) {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    if ext == "go" && is_go_generated(&src) && !go_generated_registers_routes(&src)
                    {
                        continue;
                    }
                    out.push((path.to_string_lossy().into_owned(), src));
                }
            }
        }
    }
}

/// The Go generated-code convention (golang.org/s/generatedcode): a line
/// `// Code generated <tool> DO NOT EDIT.` before the package clause marks
/// the whole file machine-written — sqlboiler, mockery, stringer, and protoc
/// plugins whose output does not carry the `.pb.go` suffix. Generated code
/// has no place in a security CPG: it dominates file counts and its
/// API-shaped wrappers collide with real sinks (e.g. sqlboiler's
/// `Query.ExecContext(ctx, exec)` — an executor argument, not a query —
/// colliding with `database/sql`'s `ExecContext(ctx, query, ...)`).
pub fn is_go_generated(src: &str) -> bool {
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("package ") {
            return false;
        }
        if t.starts_with("// Code generated ") && t.ends_with(" DO NOT EDIT.") {
            return true;
        }
    }
    false
}

/// A generated Go file that REGISTERS HTTP ROUTES is attack-surface
/// definition, not implementation noise: OpenAPI server generators
/// (oapi-codegen gin/echo/chi stubs and kin) emit a service's ENTIRE route
/// table into one generated file (`router.GET(options.BaseURL+"/pets",
/// wrapper.ListPets)`), and excluding it hides the whole surface from entry
/// mining and the census. Keep such a file iff it shows both a router-verb
/// registration marker and a route-shaped argument. Deliberately verb-only:
/// grpc-gateway's `mux.Handle("GET", pattern, closure)` stays excluded (that
/// surface is the IDL-mining lane's job), as do protoc/sqlboiler/mockery
/// output.
pub fn go_generated_registers_routes(src: &str) -> bool {
    const VERB_MARKERS: [&str; 10] = [
        ".GET(", ".POST(", ".PUT(", ".PATCH(", ".DELETE(", ".Get(", ".Post(", ".Put(", ".Patch(",
        ".Delete(",
    ];
    // Route-shaped argument: a leading-slash literal, either concatenated
    // onto a base-URL expression or passed directly.
    (src.contains("+\"/") || src.contains("(\"/")) && VERB_MARKERS.iter().any(|v| src.contains(v))
}

/// Simple `*`-wildcard match (no `?`, no character classes): `*` matches any
/// run of characters including none. Iterative backtracking, O(n·m) worst case.
pub fn glob_match(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Entry mining by convention: `NAMEPAT[@FILEPAT]` — every method whose FULL
/// name matches NAMEPAT (and whose file path matches FILEPAT when given)
/// becomes a curated entry method. The hook for code-first frameworks with no
/// IDL: Sangria GraphQL resolvers (`'Queries.*@*/schema/resolvers/*'`),
/// controller/route classes, handler suffixes.
pub fn entries_from_glob(cpg: &Cpg, pat: &str) -> Vec<String> {
    let (name_pat, file_pat) = match pat.split_once('@') {
        Some((n, f)) => (n, Some(f)),
        None => (pat, None),
    };
    let mut out: Vec<String> = cpg
        .methods()
        .into_iter()
        .filter(|&m| {
            cpg.full_name_of(m).is_some_and(|f| glob_match(name_pat, f))
                && file_pat.is_none_or(|fp| {
                    cpg.path_of(cpg.file_of(m)).is_some_and(|p| glob_match(fp, p))
                })
        })
        .filter_map(|m| cpg.full_name_of(m).map(str::to_string))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// CPG_DUMP_EDGES=<path>: write every live edge as a sorted text dump, so two
/// builds/merges can be compared for semantic (set) equality independent of
/// edge insertion order. (The raw .cpg bytes are NOT stable — the file table
/// serializes from a HashMap — so this dump is the determinism artifact.)
pub fn dump_edges_if_requested(cpg: &Cpg) {
    let Some(dump_path) = std::env::var_os("CPG_DUMP_EDGES") else {
        return;
    };
    use cpg_core::EdgeKind;
    let mut lines: Vec<String> = Vec::new();
    for n in cpg.nodes() {
        for kind in [
            EdgeKind::Ast,
            EdgeKind::Cfg,
            EdgeKind::Call,
            EdgeKind::Ref,
            EdgeKind::Ddg,
            EdgeKind::Argument,
            EdgeKind::Receiver,
            EdgeKind::Contains,
            EdgeKind::ReachingDef,
        ] {
            for d in cpg.out_kind(n, kind) {
                lines.push(format!("{:?} {:?} {:?}", kind, n, d));
            }
        }
    }
    lines.sort();
    std::fs::write(&dump_path, lines.join("\n")).expect("edge dump write failed");
    eprintln!("dumped {} edges to {:?}", lines.len(), dump_path);
}

/// A taint finding as JSON — shared by the `taint`/`scan` commands and the
/// MCP tools.
pub fn finding_json(f: &cpg_analysis::Finding) -> Value {
    let path: Vec<Value> = f
        .path
        .iter()
        .map(|s| {
            json!({
                "code": s.code,
                "line": s.line,
                "provenance": s.provenance,
                "depth": s.depth,
            })
        })
        .collect();
    let provenance: Vec<String> =
        f.path.iter().map(|s| format!("{:?}", s.provenance)).collect();
    json!({
        "method": f.method,
        "sink": f.sink,
        "line": f.sink_line,
        "sinkFile": f.sink_file,
        "origin": f.origin,
        "path": path,
        "labels": Vec::<String>::new(),
        "provenance": provenance,
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
                Some(name) if name.contains(['*', '?']) => p
                    .cpg
                    .methods()
                    .into_iter()
                    .filter(|&m| {
                        p.cpg.name_of(m).is_some_and(|n| glob_match(name, n))
                            || p.cpg.full_name_of(m).is_some_and(|f| glob_match(name, f))
                    })
                    .collect(),
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
                Some(name) if name.contains(['*', '?']) => p
                    .cpg
                    .calls()
                    .into_iter()
                    .filter(|&c| p.cpg.name_of(c).is_some_and(|n| glob_match(name, n)))
                    .collect(),
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
                        "hint": p.cpg.type_full_name_of(c),
                        "recv": p.cpg.signature_of(c),
                    })
                })
                .collect();
            json!({"calls": items})
        }
        Some("summary") => {
            let Some(fqn) = req.get("fqn").and_then(|n| n.as_str()) else {
                return json!({"error": "summary requires fqn"});
            };
            match p.summaries.get_with_origin(fqn) {
                Some((summary, origin)) => {
                    let mut ordered: Vec<_> = summary.flows.iter().collect();
                    ordered.sort();
                    let flows: Vec<Value> = ordered
                        .into_iter()
                        .map(|flow| {
                            json!({
                                "from": flow.from,
                                "to": flow.to,
                                "labels": flow.label_strings(),
                            })
                        })
                        .collect();
                    json!({"fqn": fqn, "flows": flows, "provenance": [origin]})
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
            let findings: Vec<Value> = p
                .find_taint_with_sanitizers(&src_refs, &sink_refs, &sanitizer_refs)
                .iter()
                .map(finding_json)
                .collect();
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
                Ok(r) => rules::RulePack {
                    rules: r,
                    entry_globs: vec![],
                    caller_context_markers: None,
                    framework_server_calls: None,
                },
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

#[cfg(test)]
mod glob_tests {
    use super::*;

    #[test]
    fn go_generated_header_detection() {
        // The canonical convention: header line before the package clause.
        assert!(is_go_generated(
            "// Code generated by SQLBoiler 4.14.2 (https://github.com/volatiletech/sqlboiler). DO NOT EDIT.\n// This file is meant to be re-generated in place and/or deleted at any time.\n\npackage customer\n"
        ));
        assert!(is_go_generated(
            "//go:build !ignore\n\n// Code generated by mockery v2.20.0. DO NOT EDIT.\npackage mocks\n"
        ));
        // A mention AFTER the package clause is just a comment, not a marker.
        assert!(!is_go_generated(
            "package main\n\n// Code generated by hand, honest. DO NOT EDIT.\nfunc main() {}\n"
        ));
        // Handwritten file.
        assert!(!is_go_generated("package main\nfunc main() {}\n"));
    }

    #[test]
    fn generated_route_registration_files_kept() {
        // oapi-codegen gin/echo/chi server stubs: verb registration onto a
        // BaseURL-concatenated route — the whole v2 surface of a service.
        assert!(go_generated_registers_routes(
            "// Code generated by oapi-codegen. DO NOT EDIT.\npackage api\nfunc RegisterHandlersWithOptions(router gin.IRouter, si ServerInterface, options GinServerOptions) {\n\trouter.GET(options.BaseURL+\"/accelerators\", wrapper.ListAccelerators)\n}\n"
        ));
        // Direct-literal route form (chi lowercase verbs).
        assert!(go_generated_registers_routes(
            "package api\nfunc Mount(r chi.Router) {\n\tr.Get(\"/pets\", wrapper.ListPets)\n}\n"
        ));
        // grpc-gateway: Handle(verb-string, pattern, closure) — not a verb
        // marker; that surface belongs to the IDL lane.
        assert!(!go_generated_registers_routes(
            "package gw\nfunc RegisterPetsHandlerClient(ctx context.Context, mux *runtime.ServeMux) {\n\tmux.Handle(\"GET\", pattern_Pets_List_0, func(w http.ResponseWriter, req *http.Request, pathParams map[string]string) {})\n}\n"
        ));
        // sqlboiler-ish: no verb registration marker at all.
        assert!(!go_generated_registers_routes(
            "package models\nfunc (q Query) One(ctx context.Context) (*Customer, error) { return nil, nil }\n"
        ));
    }

    #[test]
    fn glob_match_star_semantics() {
        assert!(glob_match("Queries.*", "Queries.clusterDns"));
        assert!(glob_match("*/schema/resolvers/*", "app/apps/cluster/schema/resolvers/Queries.scala"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("Queries.*", "Mutations.removeVlans"));
        assert!(!glob_match("*.scala", "Queries.rs"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn entries_from_glob_filters_by_name_and_file() {
        use cpg_core::CpgBuilder;
        let mut cpg = Cpg::new();
        let f1 = cpg.file_id("app/x/schema/resolvers/Queries.scala");
        let f2 = cpg.file_id("app/x/util/Helpers.scala");
        {
            let mut b = CpgBuilder::new(&mut cpg, f1);
            b.method("clusterDns", "Queries.clusterDns", "", Some(1));
        }
        {
            let mut b = CpgBuilder::new(&mut cpg, f2);
            b.method("helper", "Helpers.helper", "", Some(1));
        }
        let hits = entries_from_glob(&cpg, "Queries.*@*/schema/resolvers/*");
        assert_eq!(hits, vec!["Queries.clusterDns".to_string()]);
        let all = entries_from_glob(&cpg, "*.helper");
        assert_eq!(all, vec!["Helpers.helper".to_string()]);
    }

    #[test]
    fn json_taint_request_honours_sanitizers() {
        let (mut project, _) = make_project("c");
        project.build(&[(
            "v.c",
            "char* clean(char* s) { return s; }\n\
             char* source(void) { return \"x\"; }\n\
             void sink(char* s) {}\n\
             void run(void) { sink(clean(source())); }\n",
        )]);

        let without = handle(
            &mut project,
            &json!({"cmd":"taint", "sources":["source"], "sinks":["sink"]}),
        );
        assert_eq!(without["findings"].as_array().unwrap().len(), 1);

        let with = handle(
            &mut project,
            &json!({
                "cmd":"taint",
                "sources":["source"],
                "sinks":["sink"],
                "sanitizers":["clean"],
            }),
        );
        assert!(with["findings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn json_summary_exposes_labels_and_provenance() {
        let (mut project, _) = make_project("c");
        project
            .summaries
            .load_external_json(
                r#"[{"functionDeclaration":{"methodName":"escape"},
                     "dataFlows":[{"from":"param0","to":"return",
                                   "labels":["sanitized:escape"]}]}]"#,
            )
            .unwrap();

        let response = handle(&mut project, &json!({"cmd":"summary", "fqn":"escape"}));
        assert_eq!(response["flows"][0]["labels"][0], "sanitized:escape");
        assert_eq!(response["provenance"][0], "External");
    }
}
