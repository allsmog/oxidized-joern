//! `cpg` — build a project, serve queries over a line-oriented JSON
//! protocol, and scan with declarative rule packs (roadmap items #4 and
//! Gap 5: a query/rule surface decoupled from the host language).
//!
//! Usage:
//!     cpg build <dir> -o <graph.cpg> [--lang L]
//!     cpg serve <dir> [--lang L]  |  cpg serve --load <graph.cpg>
//!     cpg scan <dir> --rules <rules.json> [--lang L] [-o findings.sarif]
//!     cpg scan --load <graph.cpg> --rules <rules.json> [-o findings.sarif]
//!
//! `serve` reads one JSON request per line on stdin and writes one JSON
//! response per line on stdout. Requests:
//!     {"cmd":"stats"}
//!     {"cmd":"methods","name":"main"}            (name optional)
//!     {"cmd":"calls","name":"strcpy"}            (name optional)
//!     {"cmd":"summary","fqn":"wrap"}
//!     {"cmd":"taint","sources":["getenv"],"sinks":["system"]}
//!     {"cmd":"scan","rules":[{"id":"CPG-001","sources":["getenv"],"sinks":["system"]}]}
//!     {"cmd":"update","path":"a.c","source":"int f(){}"}   (incremental!)
//!     {"cmd":"quit"}
//!
//! `scan` runs each rule of the pack as a taint query and emits SARIF 2.1.0
//! (to stdout, or to the `-o` file). See `examples/rules/default.json` for
//! the rule format.

use cpg_cli::{build_project, flag, handle, open_project, rules::RulePack, scan};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "serve" => serve(&args),
        "build" => build_and_save(&args),
        "scan" => scan_cmd(&args),
        _ => {
            eprintln!(
                "usage (langs: c|python|java|go|javascript|ruby|rust):\n  \
                 cpg build <dir> -o <graph.cpg> [--lang L]                     build and persist a CPG\n  \
                 cpg serve <dir> [--lang L]                                    build then serve queries\n  \
                 cpg serve --load <graph.cpg>                                  reopen a saved CPG and serve\n  \
                 cpg scan <dir> --rules <rules.json> [--lang L] [-o out.sarif] run a rule pack, emit SARIF\n  \
                 cpg scan --load <graph.cpg> --rules <rules.json> [-o out]     scan a saved CPG"
            );
            std::process::exit(2);
        }
    }
}

/// `cpg build <dir> -o <out>`: build a CPG and persist it to disk.
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

/// `cpg scan`: run a declarative rule pack over the project and emit SARIF.
fn scan_cmd(args: &[String]) {
    let usage = "usage: cpg scan <dir> --rules <rules.json> [--lang L] [-o findings.sarif]\n       \
                 cpg scan --load <graph.cpg> --rules <rules.json> [-o findings.sarif]";
    let Some(rules_path) = flag(args, "--rules") else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let pack = match RulePack::from_file(rules_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let project = match open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    let fallback = args
        .get(2)
        .filter(|d| !d.starts_with("--"))
        .cloned()
        .or_else(|| flag(args, "--load").map(String::from))
        .unwrap_or_else(|| ".".to_string());
    let log = scan::scan_to_sarif(&project, &pack, &fallback);
    let n_results: usize = log.runs.iter().map(|r| r.results.len()).sum();
    let sarif = log.to_json_pretty();
    match flag(args, "-o") {
        Some(out) => {
            if let Err(e) = std::fs::write(out, sarif) {
                eprintln!("cannot write {out}: {e}");
                std::process::exit(1);
            }
            eprintln!("{} rules, {} findings -> {out}", pack.rules.len(), n_results);
        }
        None => {
            println!("{sarif}");
            eprintln!("{} rules, {} findings", pack.rules.len(), n_results);
        }
    }
}

/// `cpg serve`: either build from a directory or reopen a saved graph, then
/// answer JSON queries on stdin. A reopened graph skips parsing entirely —
/// the persistence payoff for a long-lived analysis service.
fn serve(args: &[String]) {
    let mut project = match open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\nusage: cpg serve <dir> [--lang c|python]  |  cpg serve --load <graph.cpg>");
            std::process::exit(2);
        }
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
