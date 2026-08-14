//! Library surface of the `cpg` binary: project construction helpers, the
//! JSON request handler behind `cpg serve`, and the scan/rule/SARIF layer
//! (Gap 5). The binary in `main.rs` is a thin arg-parsing shell over this so
//! integration tests can exercise the exact production code paths.

pub mod apis;
pub mod coverage;
pub mod export;
pub mod flatgraph;
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
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

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

/// A supported source language. Parsing this closed set prevents misspelled
/// language names from silently selecting an unrelated frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    C,
    Python,
    Java,
    Go,
    JavaScript,
    Ruby,
    Rust,
    Scala,
    TypeScript,
    Cpp,
}

impl Language {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "c" => Ok(Self::C),
            "python" => Ok(Self::Python),
            "java" => Ok(Self::Java),
            "go" => Ok(Self::Go),
            "javascript" | "js" => Ok(Self::JavaScript),
            "ruby" | "rb" => Ok(Self::Ruby),
            "rust" | "rs" => Ok(Self::Rust),
            "scala" => Ok(Self::Scala),
            "typescript" | "ts" => Ok(Self::TypeScript),
            "cpp" | "c++" | "cxx" => Ok(Self::Cpp),
            _ => Err(format!("unsupported language '{value}'")),
        }
    }

    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Python => "python",
            Self::Java => "java",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::Ruby => "ruby",
            Self::Rust => "rust",
            Self::Scala => "scala",
            Self::TypeScript => "typescript",
            Self::Cpp => "cpp",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::C => &["c", "h"],
            Self::Python => &["py"],
            Self::Java => &["java"],
            Self::Go => &["go"],
            Self::JavaScript => &["js", "mjs", "cjs"],
            Self::Ruby => &["rb"],
            Self::Rust => &["rs"],
            Self::Scala => &["scala", "sc"],
            Self::TypeScript => &["ts", "tsx", "mts", "cts"],
            Self::Cpp => &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"],
        }
    }

    fn project(self) -> Project {
        self.project_with_c_preprocessor(cpg_lang_c::exact::PreprocessorConfig::default())
    }

    fn project_with_c_preprocessor(
        self,
        c_preprocessor: cpg_lang_c::exact::PreprocessorConfig,
    ) -> Project {
        use cpg_lang_ts::TsFrontend;
        match self {
            Self::C => Project::new(
                move || {
                    Box::new(cpg_lang_c::CFrontend::with_preprocessor(
                        c_preprocessor.clone(),
                    ))
                },
                standard_pipeline(),
            ),
            Self::Python => Project::new(|| Box::new(TsFrontend::python()), standard_pipeline()),
            Self::Java => Project::new(|| Box::new(TsFrontend::java()), standard_pipeline()),
            Self::Go => Project::new(|| Box::new(TsFrontend::go()), standard_pipeline()),
            Self::JavaScript => {
                Project::new(|| Box::new(TsFrontend::javascript()), standard_pipeline())
            }
            Self::Ruby => Project::new(|| Box::new(TsFrontend::ruby()), standard_pipeline()),
            Self::Rust => Project::new(|| Box::new(TsFrontend::rust()), standard_pipeline()),
            Self::Scala => Project::new(|| Box::new(TsFrontend::scala()), standard_pipeline()),
            Self::TypeScript => {
                Project::new(|| Box::new(TsFrontend::typescript()), standard_pipeline())
            }
            Self::Cpp => Project::new(|| Box::new(TsFrontend::cpp()), standard_pipeline()),
        }
    }
}

/// An empty project for `lang` plus the source-file extensions it owns.
pub fn make_project(lang: &str) -> Result<(Project, &'static [&'static str]), String> {
    let lang = Language::parse(lang)?;
    Ok((lang.project(), lang.extensions()))
}

/// Construct a project with deterministic C compiler inputs. Supplying these
/// options for another language is rejected instead of silently ignored.
pub fn make_project_with_c_preprocessor(
    lang: &str,
    config: cpg_lang_c::exact::PreprocessorConfig,
) -> Result<(Project, &'static [&'static str]), String> {
    let lang = Language::parse(lang)?;
    if lang != Language::C && config != cpg_lang_c::exact::PreprocessorConfig::default() {
        return Err("C preprocessor options require --lang c".to_string());
    }
    Ok((lang.project_with_c_preprocessor(config), lang.extensions()))
}

/// Build a project by parsing every matching source file under `dir`.
pub fn build_project(dir: &str, lang: &str) -> Result<Project, String> {
    build_project_filtered(dir, lang, &[])
}

/// Build a project, skipping any file whose path contains one of the
/// `excludes` substrings (vendored, generated, and test code have no place
/// in a security CPG and often dominate the file count).
pub fn build_project_filtered(dir: &str, lang: &str, excludes: &[&str]) -> Result<Project, String> {
    build_project_ext(dir, lang, excludes, None)
}

/// [`build_project_filtered`] plus optional external summaries JSON — loaded
/// BEFORE the build so computed summaries compose with the declared ones.
pub fn build_project_ext(
    dir: &str,
    lang: &str,
    excludes: &[&str],
    external_summaries: Option<&str>,
) -> Result<Project, String> {
    build_project_ext_with_c_preprocessor(
        dir,
        lang,
        excludes,
        external_summaries,
        cpg_lang_c::exact::PreprocessorConfig::default(),
    )
}

pub fn build_project_ext_with_c_preprocessor(
    dir: &str,
    lang: &str,
    excludes: &[&str],
    external_summaries: Option<&str>,
    config: cpg_lang_c::exact::PreprocessorConfig,
) -> Result<Project, String> {
    let (_, exts) = make_project(lang)?;
    let sources = collect_sources_filtered(Path::new(dir), exts, excludes)?;
    for forced in &config.forced_includes {
        if !sources.iter().any(|(path, _)| path == forced) {
            return Err(format!(
                "forced include is not part of the source snapshot: {forced}"
            ));
        }
    }
    build_project_from_sources_with_c_preprocessor(lang, &sources, external_summaries, config)
}

/// Build from an already-collected source snapshot. Workspace caching uses
/// this path so the exact strings hashed into its manifest are the strings the
/// frontend parses, with no second filesystem read between identity and build.
pub fn build_project_from_sources(
    lang: &str,
    sources: &[(String, String)],
    external_summaries: Option<&str>,
) -> Result<Project, String> {
    build_project_from_sources_with_c_preprocessor(
        lang,
        sources,
        external_summaries,
        cpg_lang_c::exact::PreprocessorConfig::default(),
    )
}

pub fn build_project_from_sources_with_c_preprocessor(
    lang: &str,
    sources: &[(String, String)],
    external_summaries: Option<&str>,
    config: cpg_lang_c::exact::PreprocessorConfig,
) -> Result<Project, String> {
    let (mut project, _) = make_project_with_c_preprocessor(lang, config)?;
    load_externals(&mut project, external_summaries)?;
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
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
    Ok(project)
}

/// Load `--summaries <file>` external-summary JSON into a project (no-op
/// when None). Must run before build/reopen so the summary fixpoint
/// composes with the declared entries.
fn load_externals(project: &mut Project, json: Option<&str>) -> Result<(), String> {
    if let Some(json) = json {
        let n = project
            .load_external_summaries(json)
            .map_err(|e| format!("--summaries: {e}"))?;
        eprintln!("loaded {n} external function summaries");
    }
    Ok(())
}

/// Open a project the way `serve` and `scan` both do: `--load <graph.cpg>`
/// reopens a persisted CPG (skipping parsing), otherwise the positional
/// directory at `args[2]` is built from source. `--summaries <file>` loads
/// external function summaries (Fraunhofer-style JSON) either way. Reopened
/// graphs require validated adjacent language metadata or an explicit
/// `--lang`; filenames are never interpreted as frontend identity.
pub fn open_project(args: &[String]) -> Result<Project, String> {
    let ext_json: Option<String> = match flag(args, "--summaries") {
        Some(path) => {
            Some(std::fs::read_to_string(path).map_err(|e| format!("--summaries {path}: {e}"))?)
        }
        None => None,
    };
    if let Some(load) = flag(args, "--load") {
        let lang = workspace::language_for_cpg(Path::new(load), flag(args, "--lang"))?;
        let (mut p, _) = make_project(&lang)?;
        let cpg = Cpg::load(load).map_err(|e| format!("load failed: {e}"))?;
        p.reopen(cpg);
        if ext_json.is_some() {
            load_externals(&mut p, ext_json.as_deref())?;
            p.recompute_summaries();
        }
        eprintln!("loaded {} nodes from {load}", p.cpg.live_count());
        Ok(p)
    } else {
        let Some(dir) = args.get(2).filter(|d| !d.starts_with("--")) else {
            return Err("missing <dir> (or --load <graph.cpg>)".to_string());
        };
        let lang = flag(args, "--lang").unwrap_or("c");
        let config = c_preprocessor_config(args, Path::new(dir), lang)?;
        build_project_ext_with_c_preprocessor(
            dir,
            lang,
            &flags(args, "--exclude"),
            ext_json.as_deref(),
            config,
        )
    }
}

fn c_preprocessor_config(
    args: &[String],
    source_root: &Path,
    lang: &str,
) -> Result<cpg_lang_c::exact::PreprocessorConfig, String> {
    let include_values: Vec<&str> = flags(args, "--include-path")
        .into_iter()
        .chain(flags(args, "-I"))
        .collect();
    let forced_values = flags(args, "--force-include");
    let defines: Vec<String> = flags(args, "--define")
        .into_iter()
        .chain(flags(args, "-D"))
        .map(str::to_string)
        .collect();
    if include_values.is_empty() && forced_values.is_empty() && defines.is_empty() {
        return Ok(cpg_lang_c::exact::PreprocessorConfig::default());
    }
    if Language::parse(lang)? != Language::C {
        return Err("--include-path, --force-include, and --define require --lang c".to_string());
    }
    let root = source_root
        .canonicalize()
        .map_err(|error| format!("invalid source root {}: {error}", source_root.display()))?;
    let source_relative = |value: &str, expect_directory: bool| -> Result<String, String> {
        let candidate = Path::new(value);
        let candidate = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("invalid C preprocessor path {value}: {error}"))?;
        if expect_directory && !canonical.is_dir() {
            return Err(format!("C include path is not a directory: {value}"));
        }
        if !expect_directory && !canonical.is_file() {
            return Err(format!("forced include is not a file: {value}"));
        }
        let relative = canonical.strip_prefix(&root).map_err(|_| {
            format!(
                "C preprocessor path must be inside source root {}: {value}",
                root.display()
            )
        })?;
        Ok(relative.to_string_lossy().replace('\\', "/"))
    };
    for definition in &defines {
        let name = definition
            .split_once('=')
            .map_or(definition.as_str(), |(name, _)| name)
            .trim();
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
        {
            return Err(format!("invalid C preprocessor definition: {definition}"));
        }
    }
    Ok(cpg_lang_c::exact::PreprocessorConfig {
        include_paths: include_values
            .into_iter()
            .map(|value| source_relative(value, true))
            .collect::<Result<_, _>>()?,
        forced_includes: forced_values
            .into_iter()
            .map(|value| source_relative(value, false))
            .collect::<Result<_, _>>()?,
        defines,
    })
}

pub fn collect_sources(dir: &Path, exts: &[&str]) -> Result<Vec<(String, String)>, String> {
    collect_sources_filtered(dir, exts, &[])
}

pub fn collect_sources_filtered(
    dir: &Path,
    exts: &[&str],
    excludes: &[&str],
) -> Result<Vec<(String, String)>, String> {
    let root = dir
        .canonicalize()
        .map_err(|e| format!("invalid source root {}: {e}", dir.display()))?;
    let root_meta = std::fs::metadata(&root)
        .map_err(|e| format!("cannot inspect source root {}: {e}", root.display()))?;
    if !root_meta.is_dir() {
        return Err(format!(
            "source root is not a directory: {}",
            root.display()
        ));
    }

    let mut visited = HashSet::new();
    let mut sources = BTreeMap::new();
    collect_sources_dir(&root, &root, exts, excludes, &mut visited, &mut sources)?;
    if sources.is_empty() {
        return Err(format!(
            "no matching readable source files under {}",
            root.display()
        ));
    }
    Ok(sources
        .into_iter()
        .map(|(path, source)| {
            let relative = path
                .strip_prefix(&root)
                .expect("collector confines every canonical path to root");
            (relative.to_string_lossy().replace('\\', "/"), source)
        })
        .collect())
}

fn collect_sources_dir(
    dir: &Path,
    root: &Path,
    exts: &[&str],
    excludes: &[&str],
    visited: &mut HashSet<PathBuf>,
    sources: &mut BTreeMap<PathBuf, String>,
) -> Result<(), String> {
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("cannot resolve directory {}: {e}", dir.display()))?;
    if !dir.starts_with(root) {
        return Err(format!(
            "source path escapes root {}: {}",
            root.display(),
            dir.display()
        ));
    }
    if !visited.insert(dir.clone()) {
        return Ok(());
    }

    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read directory entry in {}: {e}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if source_path_is_excluded(&path, excludes) {
            continue;
        }
        let link_meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?;
        if !canonical.starts_with(root) {
            return Err(format!(
                "source path escapes root {}: {}",
                root.display(),
                path.display()
            ));
        }
        if source_path_is_excluded(&canonical, excludes) {
            continue;
        }
        let meta = if link_meta.file_type().is_symlink() {
            std::fs::metadata(&canonical)
                .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
        } else {
            link_meta
        };
        if meta.is_dir() {
            collect_sources_dir(&canonical, root, exts, excludes, visited, sources)?;
        } else if meta.is_file()
            && canonical
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| exts.contains(&ext))
        {
            let source = std::fs::read_to_string(&canonical)
                .map_err(|e| format!("cannot read source {}: {e}", path.display()))?;
            let ext = canonical.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "go" && is_go_generated(&source) && !go_generated_registers_routes(&source) {
                continue;
            }
            sources.entry(canonical).or_insert(source);
        }
    }
    Ok(())
}

fn source_path_is_excluded(path: &Path, excludes: &[&str]) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    excludes.iter().any(|exclude| {
        let exclude = exclude.replace('\\', "/");
        normalized.contains(&exclude)
            || (exclude.ends_with('/') && normalized.ends_with(exclude.trim_end_matches('/')))
    })
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
                    cpg.path_of(cpg.file_of(m))
                        .is_some_and(|p| glob_match(fp, p))
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
    let provenance: Vec<String> = f
        .path
        .iter()
        .map(|s| format!("{:?}", s.provenance))
        .collect();
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
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
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
                UpdateOutcome::Rebuilt {
                    files_reanalysed,
                    summaries_recomputed,
                } => json!({
                    "updated": true,
                    "filesReanalysed": files_reanalysed,
                    "summariesRecomputed": summaries_recomputed,
                }),
                UpdateOutcome::FullRebuildRequired => json!({
                    "error": "update requires a full source rebuild for this language"
                }),
            }
        }
        Some("quit") => json!({"quit": true}),
        _ => {
            json!({"error": "unknown cmd; one of stats|methods|calls|summary|taint|scan|update|quit"})
        }
    }
}

#[cfg(test)]
mod glob_tests {
    use super::*;

    fn source_tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cpg-source-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn language_parser_accepts_every_alias_and_canonicalizes() {
        let cases = [
            ("c", "c", &["c", "h"][..]),
            ("python", "python", &["py"][..]),
            ("java", "java", &["java"][..]),
            ("go", "go", &["go"][..]),
            ("javascript", "javascript", &["js", "mjs", "cjs"][..]),
            ("js", "javascript", &["js", "mjs", "cjs"][..]),
            ("ruby", "ruby", &["rb"][..]),
            ("rb", "ruby", &["rb"][..]),
            ("rust", "rust", &["rs"][..]),
            ("rs", "rust", &["rs"][..]),
            ("scala", "scala", &["scala", "sc"][..]),
            ("typescript", "typescript", &["ts", "tsx", "mts", "cts"][..]),
            ("ts", "typescript", &["ts", "tsx", "mts", "cts"][..]),
            (
                "cpp",
                "cpp",
                &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"][..],
            ),
            (
                "c++",
                "cpp",
                &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"][..],
            ),
            (
                "cxx",
                "cpp",
                &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"][..],
            ),
        ];
        for (alias, canonical, extensions) in cases {
            let language = Language::parse(alias).unwrap();
            assert_eq!(language.canonical_name(), canonical, "alias {alias}");
            assert_eq!(language.extensions(), extensions, "alias {alias}");
        }
        assert!(make_project("c").is_ok(), "C must be an explicit frontend");
    }

    #[test]
    fn language_parser_rejects_unknown_spellings() {
        for invalid in ["pyhton", "C", ""] {
            assert_eq!(
                Language::parse(invalid).unwrap_err(),
                format!("unsupported language '{invalid}'")
            );
        }
    }

    #[test]
    fn source_collection_rejects_missing_and_empty_roots() {
        let root = source_tmpdir("missing-empty");
        let missing = root.join("missing");
        assert!(collect_sources(&missing, &["c"])
            .unwrap_err()
            .contains("invalid source root"));
        assert!(collect_sources(&root, &["c"])
            .unwrap_err()
            .contains("no matching readable source files"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_collection_is_deterministic_and_rejects_invalid_utf8() {
        let root = source_tmpdir("ordering");
        std::fs::write(root.join("z.c"), "int z(void) { return 0; }").unwrap();
        std::fs::write(root.join("a.c"), "int a(void) { return 0; }").unwrap();
        let first = collect_sources(&root, &["c"]).unwrap();
        let second = collect_sources(&root, &["c"]).unwrap();
        assert_eq!(first, second);
        assert_eq!(first[0].0, "a.c");
        assert_eq!(first[1].0, "z.c");

        std::fs::write(root.join("bad.c"), [0xff, 0xfe]).unwrap();
        let error = collect_sources(&root, &["c"]).unwrap_err();
        assert!(error.contains("bad.c"), "{error}");
        assert!(error.contains("cannot read source"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_build_threads_c_preprocessor_inputs_into_the_frontend() {
        let root = source_tmpdir("c-preprocessor-inputs");
        std::fs::create_dir_all(root.join("include")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <feature.h>\n#if HEADER_ON && FORCE_ON && CLI_ON\nint live(void) { return VALUE(2); }\n#else\nint dead(void) { return 0; }\n#endif\n",
        )
        .unwrap();
        std::fs::write(
            root.join("include/feature.h"),
            "#define HEADER_ON 1\n#define VALUE(x) ((x) + 1)\n",
        )
        .unwrap();
        std::fs::write(root.join("config/force.h"), "#define FORCE_ON 1\n").unwrap();
        let args = vec![
            "cpg".to_string(),
            "build".to_string(),
            root.to_string_lossy().into_owned(),
            "--lang".to_string(),
            "c".to_string(),
            "--include-path".to_string(),
            "include".to_string(),
            "--force-include".to_string(),
            "config/force.h".to_string(),
            "--define".to_string(),
            "CLI_ON=1".to_string(),
        ];

        let project = open_project(&args).expect("configured C build");
        assert_eq!(project.cpg.method_named("live").len(), 1);
        assert!(project.cpg.method_named("dead").is_empty());
        assert_eq!(project.cpg.calls_named("VALUE").len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn source_collection_handles_symlinks_without_escape_or_cycles() {
        use std::os::unix::fs::symlink;

        let root = source_tmpdir("symlinks");
        let outside = source_tmpdir("outside");
        std::fs::create_dir_all(root.join("real/nested")).unwrap();
        std::fs::write(root.join("real/nested/a.c"), "int a(void) { return 0; }").unwrap();
        symlink(root.join("real"), root.join("alias")).unwrap();
        symlink(&root, root.join("real/nested/loop")).unwrap();
        let sources = collect_sources(&root, &["c"]).unwrap();
        assert_eq!(sources.len(), 1, "canonical files are deduplicated");

        std::fs::write(outside.join("outside.c"), "int x(void) { return 0; }").unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        let error = collect_sources(&root, &["c"]).unwrap_err();
        assert!(error.contains("escapes root"), "{error}");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn source_collection_applies_excludes_to_symlink_targets() {
        use std::os::unix::fs::symlink;

        let root = source_tmpdir("excluded-symlink");
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(
            root.join("vendor/hidden.c"),
            "int hidden(void) { return 0; }",
        )
        .unwrap();
        std::fs::write(root.join("visible.c"), "int visible(void) { return 0; }").unwrap();
        symlink(root.join("vendor"), root.join("alias")).unwrap();
        symlink(root.join("vendor/hidden.c"), root.join("file-alias.c")).unwrap();

        let sources = collect_sources_filtered(&root, &["c"], &["/vendor/"]).unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].0.ends_with("visible.c"), "{sources:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn source_collection_rejects_unreadable_matching_file_when_enforced() {
        use std::os::unix::fs::PermissionsExt;

        let root = source_tmpdir("unreadable");
        let source = root.join("secret.c");
        std::fs::write(&source, "int secret(void) { return 0; }").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o0)).unwrap();
        if std::fs::read_to_string(&source).is_err() {
            let error = collect_sources(&root, &["c"]).unwrap_err();
            assert!(error.contains("secret.c"), "{error}");
        }
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

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
        assert!(glob_match(
            "*/schema/resolvers/*",
            "app/apps/cluster/schema/resolvers/Queries.scala"
        ));
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
        let (mut project, _) = make_project("c").unwrap();
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
        let (mut project, _) = make_project("c").unwrap();
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

    #[test]
    fn external_summaries_are_equivalent_after_save_and_load() {
        let root = source_tmpdir("external-summaries");
        let source = root.join("flow.c");
        let summaries = root.join("summaries.json");
        let graph = root.join("flow.cpg");
        std::fs::write(&source, "char* wrap(char* s) { return vendor_copy(s); }\n").unwrap();
        std::fs::write(
            &summaries,
            r#"[{"functionDeclaration":{"language":"C","methodName":"vendor_copy"},
                  "dataFlows":[{"from":"param0","to":"return"}]}]"#,
        )
        .unwrap();

        let direct_args = vec![
            "cpg".into(),
            "serve".into(),
            root.to_string_lossy().into_owned(),
            "--lang".into(),
            "c".into(),
            "--summaries".into(),
            summaries.to_string_lossy().into_owned(),
        ];
        let direct = open_project(&direct_args).unwrap();
        let direct_flow: Vec<usize> = direct
            .summaries
            .get("wrap")
            .expect("wrap summary")
            .flows_to_return()
            .collect();
        assert_eq!(direct_flow, vec![0]);
        direct.cpg.save(&graph.to_string_lossy()).unwrap();

        let loaded_args = vec![
            "cpg".into(),
            "serve".into(),
            "--load".into(),
            graph.to_string_lossy().into_owned(),
            "--lang".into(),
            "c".into(),
            "--summaries".into(),
            summaries.to_string_lossy().into_owned(),
        ];
        let loaded = open_project(&loaded_args).unwrap();
        let loaded_flow: Vec<usize> = loaded
            .summaries
            .get("wrap")
            .expect("wrap summary after reopen")
            .flows_to_return()
            .collect();
        assert_eq!(loaded_flow, direct_flow);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generic_graph_load_requires_explicit_language() {
        let root = source_tmpdir("generic-load-language");
        let graph = root.join("generic.cpg");
        let (mut project, _) = make_project("c").unwrap();
        project.build(&[("a.c", "int main(void) { return 0; }")]);
        project.cpg.save(&graph.to_string_lossy()).unwrap();

        let implicit = vec![
            "cpg".into(),
            "serve".into(),
            "--load".into(),
            graph.to_string_lossy().into_owned(),
        ];
        let error = open_project(&implicit)
            .err()
            .expect("implicit load rejected");
        assert!(error.contains("pass --lang"), "{error}");

        let mut explicit = implicit;
        explicit.extend(["--lang".into(), "c".into()]);
        assert!(open_project(&explicit).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
}
