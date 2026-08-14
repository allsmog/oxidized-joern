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

use cpg_cli::{build_project_filtered, flag, flags, handle, open_project, rules::RulePack, scan};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const USAGE: &str = "usage (langs: c|cpp|go|java|javascript|typescript|python|ruby|rust|scala):
  cpg build <dir> -o <graph.cpg> [--lang L] [--exclude S]...    build and persist a CPG
  cpg serve <dir> [--lang L]                                    build then serve queries
  cpg serve --load <graph.cpg>                                  reopen a saved CPG and serve
  cpg scan <dir> [--rules <rules.json>] [--lang L] [-o out.sarif] rule-pack scan, emit SARIF
  cpg scan --load <graph.cpg> [--rules <rules.json>] [--lang L]  scan a saved CPG
  cpg slice --load <graph.cpg> --call <name> [--file S] [--line N] [--method M] [--depth D] [-o out.json]
  cpg slice --load <graph.cpg> --method <name> --line <N>        backward slice from a location
  cpg merge -o <merged.cpg> [--protos <dir>]... [--thrifts <dir>]... <a.cpg> [b.cpg ...]
  cpg apis <dir>|--load <graph.cpg> [--lang L] [--top N] [-o out.json]
  cpg export <dir>|--load <graph.cpg> [--lang L] [--repr ast|cfg|ddg|cpg14|all] [--format dot|graphml|json] -o <outdir>
  cpg flow <src-glob> <sink-glob> <dir>|--load <graph.cpg> [--lang L] [--sanitizer S]... [-o out.json]
  cpg vectors <dir>|--load <graph.cpg> [--lang L] [--features] [-o out.json]
  cpg rules                                                     list compiled-in rule packs
  cpg x [-C <root>] <build|scan|apis|slice|flow|serve|taint|merge|list> <path> ...
  cpg mcp [--root <repo>]                                       Model Context Protocol server over stdio
  cpg shape-version                                             print the saved-graph shape version";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--version" | "-V" => println!("cpg {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" => println!("{USAGE}"),
        "serve" => serve(&args),
        "build" => build_and_save(&args),
        "scan" => scan_cmd(&args),
        "slice" => slice_cmd(&args),
        "merge" => merge_cmd(&args),
        "apis" => apis_cmd(&args),
        "export" => export_cmd(&args),
        "flow" => flow_cmd(&args),
        "vectors" => vectors_cmd(&args),
        "rules" => rules_cmd(),
        // Machine-readable graph-shape version: cache wrappers (cpgx) key
        // cache filenames on it so engine shape changes invalidate caches
        // without a manual wipe.
        "shape-version" => println!("{}", cpg_cli::workspace::GRAPH_SHAPE_VERSION),
        "x" => x_cmd(&args),
        "mcp" => {
            if let Err(e) = cpg_cli::mcp::run(flag(&args, "--root")) {
                eprintln!("mcp server error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

/// `cpg rules`: list every rule pack compiled into this binary — the
/// zero-config per-language defaults and the named IRIS methodology packs.
fn rules_cmd() {
    // Buffered + error-tolerant so `cpg rules | head` doesn't panic on the
    // broken pipe when the reader closes early.
    let mut out = String::new();
    out.push_str("built-in language packs (used when scan has no --rules):\n");
    for lang in ["c", "cpp", "go", "java", "javascript", "python", "scala"] {
        if let Some(p) = cpg_cli::rules::builtin_pack(lang) {
            out.push_str(&format!("  {lang}: {} rules\n", p.rules.len()));
        }
    }
    out.push_str("IRIS packs (select with --rules iris:<name>):\n");
    for (name, json) in cpg_cli::rules::IRIS_PACKS {
        let p = RulePack::from_json(json).expect("compiled-in pack parses");
        let ids: Vec<&str> = p.rules.iter().map(|r| r.id.as_str()).collect();
        out.push_str(&format!(
            "  iris:{name}: {} rules [{}]\n",
            p.rules.len(),
            ids.join(", ")
        ));
    }
    let _ = std::io::stdout().write_all(out.as_bytes());
}

/// `cpg x`: root-relative front over the ordinary subcommands — cached
/// builds keyed by path/lang/graph-shape-version, language auto-detection,
/// per-language excludes, and gRPC/thrift IDL auto-discovery. Runs on any
/// repository: the root is `-C <root>`, `$CPGX_ROOT`, or the cwd.
fn x_cmd(args: &[String]) {
    use cpg_cli::workspace::Workspace;
    let usage = "usage: cpg x [-C <root>] build <path> [lang]\n       \
                 cpg x [-C <root>] scan <path> [rules.json|iris:<pack>] [scan flags...]\n       \
                 cpg x [-C <root>] apis|slice|export|vectors|serve <path> [flags...]\n       \
                 cpg x [-C <root>] flow <path> <src-glob> <sink-glob> [flags...]\n       \
                 cpg x [-C <root>] taint <path> <sources,csv> <sinks,csv>\n       \
                 cpg x [-C <root>] merge <out-name> <path1> [path2...]\n       \
                 cpg x list\n       \
                 (<path> is relative to the root; '.' = whole root; lang auto-detected)";
    let mut rest: Vec<String> = args[2..].to_vec();
    let mut root: Option<String> = None;
    if rest.first().map(String::as_str) == Some("-C") {
        if rest.len() < 2 {
            eprintln!("{usage}");
            std::process::exit(2);
        }
        root = Some(rest[1].clone());
        rest.drain(0..2);
    }
    let ws = match Workspace::open(root.as_deref()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let Some(sub) = rest.first().cloned() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let rest = &rest[1..];
    let die = |e: String| -> ! {
        eprintln!("{e}");
        std::process::exit(1);
    };
    let need_path = |rest: &[String]| -> String {
        match rest.first().filter(|p| !p.starts_with("--")) {
            Some(p) => p.clone(),
            None => {
                eprintln!("missing <path>\n{usage}");
                std::process::exit(2);
            }
        }
    };
    let synth = |cmd: &str, cpg: &std::path::Path, lang: &str| -> Vec<String> {
        vec![
            "cpg".to_string(),
            cmd.to_string(),
            "--load".to_string(),
            cpg.to_string_lossy().into_owned(),
            "--lang".to_string(),
            lang.to_string(),
        ]
    };
    match sub.as_str() {
        "list" => {
            for p in ws.cached() {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                println!("{:>12}  {}", size, p.display());
            }
        }
        "build" => {
            let path = need_path(rest);
            let lang = rest
                .get(1)
                .filter(|l| !l.starts_with("--"))
                .map(String::as_str);
            let (cpg, _) = ws.ensure_cpg(&path, lang).unwrap_or_else(|e| die(e));
            println!("{}", cpg.display());
        }
        "scan" => {
            let path = need_path(rest);
            let mut pass: Vec<String> = rest[1..].to_vec();
            let lang_opt = flag(&pass, "--lang").map(String::from);
            let (cpg, lang) = ws
                .ensure_cpg(&path, lang_opt.as_deref())
                .unwrap_or_else(|e| die(e));
            let mut argv = synth("scan", &cpg, &lang);
            // positional rules argument (a file or iris:<pack>), cpgx-style
            if pass.first().is_some_and(|a| !a.starts_with("--")) {
                argv.push("--rules".to_string());
                argv.push(pass.remove(0));
            }
            // gRPC-aware by default: protos under the module, thrift root-wide
            for d in ws.proto_dirs(&path) {
                argv.push("--rpc-sources".to_string());
                argv.push(d);
            }
            for d in ws.thrift_dirs() {
                argv.push("--thrift-sources".to_string());
                argv.push(d);
            }
            // Play routes under the module: the miner no-ops when none exist.
            if let Ok(d) = ws.module_dir(&path) {
                argv.push("--play-routes".to_string());
                argv.push(d.to_string_lossy().into_owned());
            }
            argv.extend(pass);
            scan_cmd(&argv);
        }
        "apis" | "slice" | "export" | "vectors" | "serve" => {
            let path = need_path(rest);
            let pass: Vec<String> = rest[1..].to_vec();
            let lang_opt = flag(&pass, "--lang").map(String::from);
            let (cpg, lang) = ws
                .ensure_cpg(&path, lang_opt.as_deref())
                .unwrap_or_else(|e| die(e));
            let mut argv = synth(&sub, &cpg, &lang);
            argv.extend(pass);
            match sub.as_str() {
                "apis" => apis_cmd(&argv),
                "slice" => slice_cmd(&argv),
                "export" => export_cmd(&argv),
                "vectors" => vectors_cmd(&argv),
                _ => serve(&argv),
            }
        }
        "flow" => {
            let path = need_path(rest);
            let (Some(src), Some(sink)) = (rest.get(1), rest.get(2)) else {
                eprintln!("flow needs <path> <src-glob> <sink-glob>\n{usage}");
                std::process::exit(2);
            };
            let pass: Vec<String> = rest[3..].to_vec();
            let lang_opt = flag(&pass, "--lang").map(String::from);
            let (cpg, lang) = ws
                .ensure_cpg(&path, lang_opt.as_deref())
                .unwrap_or_else(|e| die(e));
            let mut argv = vec![
                "cpg".to_string(),
                "flow".to_string(),
                src.clone(),
                sink.clone(),
            ];
            argv.push("--load".to_string());
            argv.push(cpg.to_string_lossy().into_owned());
            argv.push("--lang".to_string());
            argv.push(lang);
            argv.extend(pass);
            flow_cmd(&argv);
        }
        "taint" => {
            let path = need_path(rest);
            let (Some(sources), Some(sinks)) = (rest.get(1), rest.get(2)) else {
                eprintln!("taint needs <path> <sources,csv> <sinks,csv>\n{usage}");
                std::process::exit(2);
            };
            let (cpg, lang) = ws.ensure_cpg(&path, None).unwrap_or_else(|e| die(e));
            let argv = synth("taint", &cpg, &lang);
            let mut project = open_project(&argv).unwrap_or_else(|e| die(e));
            let req = json!({
                "cmd": "taint",
                "sources": sources.split(',').collect::<Vec<&str>>(),
                "sinks": sinks.split(',').collect::<Vec<&str>>(),
            });
            println!("{}", handle(&mut project, &req));
        }
        "merge" => {
            let out_name = need_path(rest);
            let modules: Vec<String> = rest[1..]
                .iter()
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .collect();
            if modules.is_empty() {
                eprintln!("merge needs <out-name> <path1> [path2...]\n{usage}");
                std::process::exit(2);
            }
            let out = ws.merge_output_path(&out_name).unwrap_or_else(|e| die(e));
            let mut argv = vec![
                "cpg".to_string(),
                "merge".to_string(),
                "-o".to_string(),
                out.to_string_lossy().into_owned(),
            ];
            for m in &modules {
                for d in ws.proto_dirs(m) {
                    argv.push("--protos".to_string());
                    argv.push(d);
                }
            }
            for d in ws.thrift_dirs() {
                argv.push("--thrifts".to_string());
                argv.push(d);
            }
            for m in &modules {
                let (cpg, _) = ws.ensure_cpg(m, None).unwrap_or_else(|e| die(e));
                argv.push(cpg.to_string_lossy().into_owned());
            }
            merge_cmd(&argv);
            println!("{}", out.display());
        }
        other => {
            eprintln!("unknown x subcommand '{other}'\n{usage}");
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
    let project = match build_project_filtered(dir, lang, &flags(args, "--exclude")) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    cpg_cli::dump_edges_if_requested(&project.cpg);
    match project.cpg.save(out) {
        Ok(()) => {
            let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            eprintln!(
                "saved {} nodes to {out} ({size} bytes)",
                project.cpg.live_count()
            );
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            std::process::exit(1);
        }
    }
}

/// `cpg slice`: backward slice over the ReachingDef layer from a sink call
/// or a method:line location.
fn slice_cmd(args: &[String]) {
    use cpg_cli::slice::{backward_slice, criterion_calls, criterion_location, slice_json};
    let usage = "usage: cpg slice --load <graph.cpg> [--lang L] --call <name> [--file S] [--line N] [--method M] [--depth D] [--max N] [-o out.json]\n       \
                 cpg slice --load <graph.cpg> [--lang L] --method <name> --line <N> [--depth D] [-o out.json]";
    let project = match open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    let cpg = &project.cpg;
    let line = flag(args, "--line").and_then(|l| l.parse::<u32>().ok());
    let depth = flag(args, "--depth")
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(3);
    let max_nodes = flag(args, "--max")
        .and_then(|m| m.parse::<usize>().ok())
        .unwrap_or(5000);
    let criteria = if let Some(call) = flag(args, "--call") {
        criterion_calls(
            cpg,
            call,
            flag(args, "--file"),
            line,
            flag(args, "--method"),
        )
    } else if let (Some(m), Some(l)) = (flag(args, "--method"), line) {
        criterion_location(cpg, m, l)
    } else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    if criteria.is_empty() {
        eprintln!("no criterion nodes matched");
        std::process::exit(1);
    }
    let (entries, truncated) = backward_slice(cpg, &criteria, depth, max_nodes);
    let files: std::collections::HashSet<&str> = entries.iter().map(|e| e.file.as_str()).collect();
    eprintln!(
        "slice: {} criterion nodes -> {} nodes across {} files{}",
        criteria.len(),
        entries.len(),
        files.len(),
        if truncated {
            " (TRUNCATED at --max)"
        } else {
            ""
        }
    );
    let out =
        serde_json::to_string_pretty(&slice_json(cpg, &criteria, &entries, truncated)).unwrap();
    match flag(args, "-o") {
        Some(path) => {
            if let Err(e) = std::fs::write(path, out) {
                eprintln!("cannot write {path}: {e}");
                std::process::exit(1);
            }
            eprintln!("wrote {path}");
        }
        None => println!("{out}"),
    }
}

/// `cpg merge`: absorb one or more CPGs into one, re-resolve calls globally,
/// and stitch RPC boundaries — gRPC via .proto rpc declarations (`--protos`),
/// thrift via .thrift service declarations (`--thrifts`). A single input is
/// legal: the client and server of an in-process or same-language RPC live in
/// one CPG, and only the stitch is wanted.
fn merge_cmd(args: &[String]) {
    use cpg_cli::merge::{link_rpcs, relink_calls, rpc_names};
    let usage =
        "usage: cpg merge -o <merged.cpg> [--protos <dir>]... [--thrifts <dir>]... <a.cpg> [b.cpg ...]";
    let Some(out) = flag(args, "-o") else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    // Positional .cpg inputs: everything after the subcommand that isn't a
    // flag or a flag's value.
    let mut inputs: Vec<&str> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        let a = args[i].as_str();
        if a.starts_with("--") || a == "-o" {
            i += 2;
            continue;
        }
        inputs.push(a);
        i += 1;
    }
    if inputs.is_empty() {
        eprintln!("need at least one CPG\n{usage}");
        std::process::exit(2);
    }
    let mut merged = cpg_core::Cpg::new();
    for path in &inputs {
        match cpg_core::Cpg::load(path) {
            Ok(donor) => {
                use cpg_core::Query;
                eprintln!("absorbing {path} ({} nodes)", donor.live_count());
                let _ = donor.methods();
                merged.absorb(donor);
            }
            Err(e) => {
                eprintln!("cannot load {path}: {e}");
                std::process::exit(1);
            }
        }
    }
    relink_calls(&mut merged);
    let mut rpcs: Vec<String> = Vec::new();
    for dir in cpg_cli::flags(args, "--protos") {
        rpc_names(std::path::Path::new(dir), &mut rpcs);
    }
    rpcs.sort();
    rpcs.dedup();
    if !rpcs.is_empty() {
        let (added, skipped) = link_rpcs(&mut merged, &rpcs);
        eprintln!(
            "rpc stitch: {} declared rpcs, {added} client->handler edges",
            rpcs.len()
        );
        for s in &skipped {
            eprintln!("  skipped over-generic rpc name: {s}");
        }
    }
    let mut services: Vec<cpg_cli::thrift::ThriftService> = Vec::new();
    for dir in cpg_cli::flags(args, "--thrifts") {
        cpg_cli::thrift::thrift_services(std::path::Path::new(dir), &mut services);
    }
    if !services.is_empty() {
        cpg_cli::thrift::resolve_extends(&mut services);
        let (added, skipped) = cpg_cli::thrift::link_thrift(&mut merged, &services);
        eprintln!(
            "thrift stitch: {} services, {added} client->handler edges",
            services.len()
        );
        for s in &skipped {
            eprintln!("  skipped over-generic thrift method: {s}");
        }
    }
    cpg_cli::dump_edges_if_requested(&merged);
    use cpg_core::Query;
    match merged.save(out) {
        Ok(()) => eprintln!(
            "merged {} CPGs -> {out} ({} nodes, {} methods)",
            inputs.len(),
            merged.live_count(),
            merged.methods().len()
        ),
        Err(e) => {
            eprintln!("save failed: {e}");
            std::process::exit(1);
        }
    }
}

/// `cpg apis`: inventory of external APIs (called names with no defining
/// body in the CPG) as JSON — the input to IRIS-style LLM spec inference.
fn apis_cmd(args: &[String]) {
    let usage = "usage: cpg apis <dir>|--load <graph.cpg> [--lang L] [--top N] [--min-count N] [--examples K] [-o out.json]";
    let project = match cpg_cli::open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    let examples = flag(args, "--examples")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let top: usize = flag(args, "--top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let min_count: usize = flag(args, "--min-count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let mut inv = cpg_cli::apis::inventory(&project.cpg, examples);
    inv.retain(|e| e.count >= min_count);
    inv.truncate(top);
    eprintln!("{} external APIs (after filters)", inv.len());
    let json = serde_json::to_string_pretty(&inv).expect("serialize");
    match flag(args, "-o") {
        Some(out) => {
            if let Err(e) = std::fs::write(out, json) {
                eprintln!("cannot write {out}: {e}");
                std::process::exit(1);
            }
            eprintln!("-> {out}");
        }
        None => println!("{json}"),
    }
}

/// `cpg export`: dump the graph (whole or split per method) as
/// dot / graphml / json. JoernExport parity.
fn export_cmd(args: &[String]) {
    use cpg_cli::export::{export, Format, Repr};
    let usage = "usage: cpg export <dir>|--load <graph.cpg> [--lang L] [--repr ast|cfg|ddg|cpg14|all] [--format dot|graphml|json] -o <outdir>";
    let repr = match flag(args, "--repr") {
        None => Repr::Cpg14,
        Some(s) => match Repr::parse(s) {
            Some(r) => r,
            None => {
                eprintln!("unknown --repr {s}\n{usage}");
                std::process::exit(2);
            }
        },
    };
    let format = match flag(args, "--format") {
        None => Format::Dot,
        Some(s) => match Format::parse(s) {
            Some(f) => f,
            None => {
                eprintln!("unknown --format {s}\n{usage}");
                std::process::exit(2);
            }
        },
    };
    let Some(out) = flag(args, "-o") else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let project = match open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    match export(&project.cpg, repr, format, std::path::Path::new(out)) {
        Ok(stats) => eprintln!(
            "exported {} nodes, {} edges into {} files under {out}",
            stats.nodes, stats.edges, stats.files
        ),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
}

/// `cpg flow`: one-off source→sink taint query without writing a rules file.
/// JoernFlow parity, with globs instead of regexes (same matcher as
/// --entry-glob): call names matching <src-glob> are sources, methods
/// matching it donate their parameters as entry points, and call names
/// matching <sink-glob> are sinks.
fn flow_cmd(args: &[String]) {
    use std::collections::BTreeSet;
    let usage = "usage: cpg flow <src-glob> <sink-glob> <dir>|--load <graph.cpg> [--lang L] [--sanitizer S]... [-o out.json]";
    // Positional args after the subcommand, skipping flags and their values.
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        let a = args[i].as_str();
        if a.starts_with('-') {
            i += 2;
            continue;
        }
        positional.push(a);
        i += 1;
    }
    let (Some(src_glob), Some(sink_glob)) = (positional.first(), positional.get(1)) else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    // Rebuild an argv whose position 2 is the directory (if any), so
    // open_project's dir/--load convention applies unchanged.
    let mut open_args: Vec<String> = vec![
        args[0].clone(),
        "flow".to_string(),
        positional.get(2).copied().unwrap_or("--").to_string(),
    ];
    open_args.extend(args[2..].iter().cloned());
    let project = match open_project(&open_args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    use cpg_core::Query;
    let cpg = &project.cpg;
    let sources: BTreeSet<String> = cpg
        .calls()
        .into_iter()
        .filter_map(|c| cpg.name_of(c))
        .filter(|n| cpg_cli::glob_match(src_glob, n))
        .map(str::to_string)
        .collect();
    let sinks: BTreeSet<String> = cpg
        .calls()
        .into_iter()
        .filter_map(|c| cpg.name_of(c))
        .filter(|n| cpg_cli::glob_match(sink_glob, n))
        .map(str::to_string)
        .collect();
    // Methods matching the source glob are entry points: their parameters
    // count as attacker-controlled (JoernFlow's param-to-param mode).
    let entries: Vec<String> = cpg
        .methods()
        .into_iter()
        .filter_map(|m| cpg.name_of(m))
        .filter(|n| cpg_cli::glob_match(src_glob, n))
        .map(str::to_string)
        .collect();
    eprintln!(
        "{} source call names, {} entry methods, {} sink call names",
        sources.len(),
        entries.len(),
        sinks.len()
    );
    if sinks.is_empty() {
        eprintln!("no call names match sink glob '{sink_glob}'");
        std::process::exit(1);
    }
    let rule_json = json!({"rules": [{
        "id": "FLOW",
        "description": format!("{src_glob} -> {sink_glob}"),
        "sources": sources,
        "sinks": sinks,
        "sanitizers": flags(args, "--sanitizer"),
    }]});
    let pack = match RulePack::from_json(&rule_json.to_string()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("internal rule construction failed: {e}");
            std::process::exit(1);
        }
    };
    let per_rule = scan::run_pack_entry(&project, &pack, &entries, &[], &[]);
    let findings: Vec<&cpg_analysis::Finding> =
        per_rule.iter().flat_map(|rf| rf.findings.iter()).collect();
    for f in &findings {
        eprintln!(
            "flow: {} -> {} @ {}:{}",
            f.origin,
            f.sink,
            f.sink_file.as_deref().unwrap_or("?"),
            f.sink_line
                .map(|l| l.to_string())
                .unwrap_or_else(|| "?".to_string())
        );
    }
    eprintln!("{} flows", findings.len());
    let out = serde_json::to_string_pretty(&findings).expect("serialize");
    match flag(args, "-o") {
        Some(path) => {
            if let Err(e) = std::fs::write(path, out) {
                eprintln!("cannot write {path}: {e}");
                std::process::exit(1);
            }
            eprintln!("wrote {path}");
        }
        None => println!("{out}"),
    }
}

/// `cpg vectors`: bag-of-properties embedding of every node plus the edge
/// list, as one JSON document. JoernVectors parity.
fn vectors_cmd(args: &[String]) {
    let usage = "usage: cpg vectors <dir>|--load <graph.cpg> [--lang L] [--features] [-o out.json]";
    let project = match open_project(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\n{usage}");
            std::process::exit(2);
        }
    };
    let with_features = args.iter().any(|a| a == "--features");
    let result = match flag(args, "-o") {
        Some(path) => {
            let file = match std::fs::File::create(path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("cannot create {path}: {e}");
                    std::process::exit(1);
                }
            };
            let mut w = std::io::BufWriter::new(file);
            let r = cpg_cli::vectors::write_vectors(&project.cpg, with_features, &mut w);
            if r.is_ok() {
                eprintln!("wrote {path}");
            }
            r
        }
        None => {
            let stdout = std::io::stdout();
            let mut w = std::io::BufWriter::new(stdout.lock());
            cpg_cli::vectors::write_vectors(&project.cpg, with_features, &mut w)
        }
    };
    if let Err(e) = result {
        eprintln!("vectors failed: {e}");
        std::process::exit(1);
    }
}

/// `cpg scan`: run a declarative rule pack over the project and emit SARIF.
fn scan_cmd(args: &[String]) {
    let usage = "usage: cpg scan <dir> [--rules <rules.json>|iris:<pack>] [--lang L] [-o findings.sarif]\n       \
                 cpg scan --load <graph.cpg> [--rules <rules.json>|iris:<pack>] [--lang L] [-o findings.sarif]\n       \
                 (without --rules, the built-in security pack for --lang is used;\n       \
                  iris:<pack> selects a compiled-in IRIS pack — see cpg rules)";
    let pack = match flag(args, "--rules") {
        Some(rules_path) => match RulePack::resolve(rules_path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => {
            let lang = flag(args, "--lang").unwrap_or("c");
            match cpg_cli::rules::builtin_pack(lang) {
                Some(p) => {
                    eprintln!("using built-in {lang} rule pack ({} rules)", p.rules.len());
                    p
                }
                None => {
                    eprintln!("no built-in rule pack for lang {lang}; pass --rules\n{usage}");
                    std::process::exit(2);
                }
            }
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
    // --rpc-sources <protodir>: every rpc declared there names an entry-point
    // method whose parameters are attacker-controlled for every rule.
    // --entry <name>: add a single entry-point method by name (repeatable) —
    // the hook for handler lists mined outside protobuf (thrift, HTTP routers).
    // IDL-mined entries (guarded: must look like handlers) vs curated
    // entries (--entry, thrift-derived qualified names: trusted verbatim).
    let mut idl_entries: Vec<String> = Vec::new();
    for dir in flags(args, "--rpc-sources") {
        cpg_cli::merge::rpc_names(std::path::Path::new(dir), &mut idl_entries);
    }
    // Scala/Java gRPC handlers are lowerCamel while proto rpcs are
    // PascalCase — admit both spellings (the handler-shape guard keeps
    // collisions out).
    let camel: Vec<String> = idl_entries
        .iter()
        .map(|e| cpg_cli::merge::lc_first(e))
        .collect();
    idl_entries.extend(camel);
    idl_entries.sort();
    idl_entries.dedup();
    let mut entry_methods: Vec<String> = Vec::new();
    entry_methods.extend(flags(args, "--entry").into_iter().map(String::from));
    // --thrift-sources <dir>: thrift service methods, resolved against the
    // graph's handler classes (TypeDecls subclassing the generated If), added
    // as qualified `Handler::method` entries.
    let mut services: Vec<cpg_cli::thrift::ThriftService> = Vec::new();
    for dir in flags(args, "--thrift-sources") {
        cpg_cli::thrift::thrift_services(std::path::Path::new(dir), &mut services);
    }
    if !services.is_empty() {
        cpg_cli::thrift::resolve_extends(&mut services);
        let entries = cpg_cli::thrift::thrift_entries(&project.cpg, &services);
        eprintln!(
            "{} thrift services -> {} handler entry methods",
            services.len(),
            entries.len()
        );
        entry_methods.extend(entries);
    }
    // --play-routes <dir-or-file>: Play Framework `conf/routes` bindings,
    // resolved against the graph's controller classes, added as qualified
    // full-name entries. The unresolved count keeps zeros honest.
    let mut routes: Vec<cpg_cli::play::PlayRoute> = Vec::new();
    for p in flags(args, "--play-routes") {
        cpg_cli::play::play_routes(std::path::Path::new(p), &mut routes);
    }
    if !routes.is_empty() {
        let (entries, unresolved) = cpg_cli::play::play_entries(&project.cpg, &routes);
        eprintln!(
            "{} play routes -> {} controller entry methods ({unresolved} routes unresolved)",
            routes.len(),
            entries.len()
        );
        entry_methods.extend(entries);
    }
    // --entry-glob 'NAMEPAT[@FILEPAT]': every method whose full name matches
    // NAMEPAT (and whose file matches FILEPAT, if given) is a curated entry.
    // The convention hook for code-first frameworks with no IDL to mine —
    // Sangria GraphQL resolvers ('Queries.*@*/schema/resolvers/*'), route
    // classes, controller suffixes.
    for pat in flags(args, "--entry-glob") {
        let entries = cpg_cli::entries_from_glob(&project.cpg, pat);
        eprintln!("--entry-glob {pat}: {} entry methods", entries.len());
        entry_methods.extend(entries);
    }
    // Pack-carried entry conventions: same mechanics as --entry-glob, but the
    // pattern ships with the rule pack (GraphQL resolver directories, wire
    // protocol connection handlers) so scans need no per-target flags.
    for pat in &pack.entry_globs {
        let entries = cpg_cli::entries_from_glob(&project.cpg, pat);
        eprintln!("pack entry-glob {pat}: {} entry methods", entries.len());
        entry_methods.extend(entries);
    }
    entry_methods.sort();
    entry_methods.dedup();
    if !entry_methods.is_empty() || !idl_entries.is_empty() {
        eprintln!(
            "{} curated + {} IDL-mined entry-point methods",
            entry_methods.len(),
            idl_entries.len()
        );
    }
    // Registration-mined entries: methods passed by value to router /
    // consumer registration APIs (HandleFunc, Subscribe, ...). Set
    // CPG_NO_REG_ENTRIES=1 to disable.
    let registered_entries = if std::env::var("CPG_NO_REG_ENTRIES").is_ok() {
        Vec::new()
    } else {
        cpg_analysis::mine_registration_entries(&project.cpg)
    };
    if !registered_entries.is_empty() {
        let shown: Vec<&str> = registered_entries
            .iter()
            .take(20)
            .map(String::as_str)
            .collect();
        let more = registered_entries.len().saturating_sub(shown.len());
        eprintln!(
            "{} registration-mined entry-point methods: {}{}",
            registered_entries.len(),
            shown.join(", "),
            if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            }
        );
    }
    // One pack run feeds flows-json, SARIF, and the coverage report alike —
    // the taint query is the expensive part and must not repeat.
    let per_rule = scan::run_pack_entry(
        &project,
        &pack,
        &entry_methods,
        &idl_entries,
        &registered_entries,
    );
    // --flows-json <path>: full witness paths + rule metadata, the input to
    // an LLM triage pass (IRIS's contextual filtering stage).
    if let Some(flows_out) = flag(args, "--flows-json") {
        let flows: Vec<serde_json::Value> = per_rule
            .iter()
            .flat_map(|rf| {
                rf.findings.iter().map(|f| {
                    serde_json::json!({
                        "rule": {"id": rf.rule.id, "cwe": rf.rule.cwe,
                                 "severity": rf.rule.severity,
                                 "description": rf.rule.description},
                        // Sink file when the finding carries one; the
                        // name-based method lookup is only a fallback (it
                        // picks the first same-named method — worthless for
                        // `<anon>` entries, which collide graph-wide).
                        "file": f.sink_file.clone()
                            .or_else(|| scan::file_of_method(&project.cpg, &f.method)),
                        "finding": f,
                    })
                })
            })
            .collect();
        let js = serde_json::to_string_pretty(&flows).expect("serialize");
        if let Err(e) = std::fs::write(flows_out, js) {
            eprintln!("cannot write {flows_out}: {e}");
            std::process::exit(1);
        }
        eprintln!("{} flows -> {flows_out}", flows.len());
    }
    // --authz-census: router-level authorization census over every entry
    // (curated + IDL + registration-mined) with middleware/interceptor
    // mining — resolves the annotate_authz `None` ambiguity between
    // "no check" and "check lives in middleware". Advisory, stderr;
    // --authz-census-json <path> additionally writes rows + gates.
    if args.iter().any(|a| a == "--authz-census") || flag(args, "--authz-census-json").is_some() {
        let authz_names: std::collections::HashSet<String> = pack
            .rules
            .iter()
            .flat_map(|r| r.authz.iter().cloned())
            .collect();
        // Curated + registration-mined entries are trusted verbatim; IDL
        // names go in separately so the census can apply the same
        // handler-shape gate the taint matcher uses (an rpc named `Get`
        // must not census every same-named utility method).
        let mut census_entries: Vec<String> = entry_methods.clone();
        census_entries.extend(registered_entries.iter().cloned());
        census_entries.sort();
        census_entries.dedup();
        let mut census_idl: Vec<String> = idl_entries.clone();
        census_idl.sort();
        census_idl.dedup();
        let census_config = pack.authz_census_config();
        let census = cpg_analysis::authz_census_with_config(
            &project.cpg,
            &authz_names,
            &census_entries,
            &census_idl,
            &census_config,
        );
        let (inline, wrapped, mw, subject_gated, partial, none) = census.counts();
        eprintln!(
            "authz census: {} entries -> {inline} inline / {wrapped} wrapped / {mw} middleware / {subject_gated} subject-gated / {partial} inline-partial / {none} none ({} routes)",
            census.rows.len(),
            census.routes.len()
        );
        for g in &census.gates {
            eprintln!(
                "  gate: {}({}) @{} {}",
                g.scope,
                g.name,
                g.line,
                if g.enforcing {
                    "ENFORCING"
                } else {
                    "not-enforcing"
                }
            );
        }
        let nones: Vec<&str> = census
            .rows
            .iter()
            .filter(|(_, v)| v == "none")
            .map(|(e, _)| e.as_str())
            .collect();
        if !nones.is_empty() {
            let shown = &nones[..nones.len().min(25)];
            let more = nones.len() - shown.len();
            eprintln!(
                "  none (triage first): {}{}",
                shown.join(", "),
                if more > 0 {
                    format!(" (+{more} more)")
                } else {
                    String::new()
                }
            );
        }
        if let Some(out) = flag(args, "--authz-census-json") {
            let js = serde_json::to_string_pretty(&census).expect("serialize");
            if let Err(e) = std::fs::write(out, js) {
                eprintln!("cannot write {out}: {e}");
                std::process::exit(1);
            }
            eprintln!("authz census -> {out}");
        }
    }
    // Coverage report: certifies what a zero-finding scan actually covered.
    let finding_methods: std::collections::HashSet<String> = per_rule
        .iter()
        .flat_map(|rf| rf.findings.iter().map(|f| f.method.clone()))
        .collect();
    eprint!(
        "{}",
        cpg_cli::coverage::coverage_report(
            &project.cpg,
            &entry_methods,
            &idl_entries,
            &pack,
            &finding_methods,
        )
    );
    let log = cpg_cli::sarif::build_log(
        &pack,
        &per_rule,
        &|method| scan::file_of_method(&project.cpg, method),
        &fallback,
    );
    let n_results: usize = log.runs.iter().map(|r| r.results.len()).sum();
    let sarif = log.to_json_pretty();
    match flag(args, "-o") {
        Some(out) => {
            if let Err(e) = std::fs::write(out, sarif) {
                eprintln!("cannot write {out}: {e}");
                std::process::exit(1);
            }
            eprintln!(
                "{} rules, {} findings -> {out}",
                pack.rules.len(),
                n_results
            );
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
            eprintln!(
                "{e}\nusage: cpg serve <dir> [--lang c|python]  |  cpg serve --load <graph.cpg>"
            );
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
