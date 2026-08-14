//! Workspace driver: root-relative CPG analysis for ANY repository (`cpg x`).
//!
//! This is the former `cpgx` shell wrapper absorbed into the binary, so a
//! single self-contained executable runs the whole IRIS loop anywhere:
//! module paths relative to a project root, language auto-detection by file
//! census, per-language exclude sets for vendored/generated/test code, an
//! exact content-manifest CPG cache, and gRPC/thrift IDL auto-discovery for
//! entry-point mining.
//!
//! Nothing here is tied to a machine or project: the root comes from
//! `-C`/`$CPGX_ROOT`/cwd, the cache from `$CPG_CACHE`/`$XDG_CACHE_HOME/cpg`/
//! `~/.cache/cpg`.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

/// Bumped whenever an engine change alters the shape of built graphs
/// (lowering, new node/edge kinds, persist layout). Part of every cache file
/// name, so stale CPGs built by an older engine are rebuilt instead of
/// silently reused — the mtime check alone cannot see binary changes.
/// v2: C++ ctor-factory lowering (make_shared<T> named T) + direct-init
/// declaration lowering (`Type var(args)` = var-assignment of a Type call).
/// v6: Go variadic-spread arguments (`f(xs...)`) unwrap to the inner
/// expression instead of vanishing from the call's argument list.
/// v7: Go generated files (`// Code generated ... DO NOT EDIT.` before the
/// package clause) are excluded at collection time — graph CONTENT change,
/// same staleness hazard as a shape change.
/// v8: decorators lowered into the decorated method's body (Python
/// `@app.post("/x")` / `@require_admin` become leading Call nodes) — entry
/// mining and authz-census evidence for decorator-registered handlers.
/// v9: generated Go files that register HTTP routes (oapi-codegen server
/// stubs) are KEPT at collection time (`go_generated_registers_routes`) —
/// file-set change, same staleness hazard as a shape change.
pub const GRAPH_SHAPE_VERSION: u32 = 11;

const CACHE_MANIFEST_VERSION: u32 = 1;
const CACHE_GRAPH_FORMAT: &str = "CPG2";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceManifestEntry {
    path: String,
    digest: String,
}

/// Metadata binding a cached graph to the exact source snapshot and frontend
/// policy that produced it. The graph digest makes graph + manifest a pair:
/// an interrupted publication can never advertise an older or partial graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheManifest {
    manifest_version: u32,
    graph_format: String,
    graph_shape_version: u32,
    workspace_root_digest: String,
    module_path_digest: String,
    language: String,
    excludes: Vec<String>,
    sources: Vec<SourceManifestEntry>,
    graph_digest: String,
}

impl CacheManifest {
    fn source_state_matches(&self, expected: &Self) -> bool {
        self.manifest_version == expected.manifest_version
            && self.graph_format == expected.graph_format
            && self.graph_shape_version == expected.graph_shape_version
            && self.workspace_root_digest == expected.workspace_root_digest
            && self.module_path_digest == expected.module_path_digest
            && self.language == expected.language
            && self.excludes == expected.excludes
            && self.sources == expected.sources
    }
}

struct CacheLock {
    _file: File,
}

/// A project root plus the CPG cache directory.
#[derive(Clone, Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub cache: PathBuf,
}

impl Workspace {
    /// Resolve the workspace: `root` argument, else `$CPGX_ROOT`, else cwd.
    /// Cache: `$CPG_CACHE`, else `$XDG_CACHE_HOME/cpg`, else `~/.cache/cpg`.
    pub fn open(root: Option<&str>) -> Result<Workspace, String> {
        let root = match root
            .map(String::from)
            .or_else(|| std::env::var("CPGX_ROOT").ok())
        {
            Some(r) => PathBuf::from(r),
            None => std::env::current_dir().map_err(|e| format!("no cwd: {e}"))?,
        };
        let root = root
            .canonicalize()
            .map_err(|e| format!("bad root {}: {e}", root.display()))?;
        let cache = match std::env::var("CPG_CACHE").ok() {
            Some(c) => PathBuf::from(c),
            None => match std::env::var("XDG_CACHE_HOME").ok() {
                Some(x) => PathBuf::from(x).join("cpg"),
                None => match std::env::var("HOME").ok() {
                    Some(h) => PathBuf::from(h).join(".cache").join("cpg"),
                    None => PathBuf::from(".cpg-cache"),
                },
            },
        };
        std::fs::create_dir_all(&cache)
            .map_err(|e| format!("cannot create cache {}: {e}", cache.display()))?;
        let cache = cache
            .canonicalize()
            .map_err(|e| format!("bad cache {}: {e}", cache.display()))?;
        Ok(Workspace { root, cache })
    }

    /// Absolute directory for a root-relative module path (`.` = the root).
    pub fn module_dir(&self, rel: &str) -> Result<PathBuf, String> {
        let root = self
            .root
            .canonicalize()
            .map_err(|e| format!("bad root {}: {e}", self.root.display()))?;
        let dir = if rel == "." {
            root.clone()
        } else {
            let path = Path::new(rel);
            if rel.is_empty()
                || path.is_absolute()
                || rel
                    .split(['/', '\\'])
                    .any(|segment| segment.is_empty() || segment == "." || segment == "..")
                || !path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                return Err(format!("module path must be root-relative: {rel}"));
            }
            root.join(path)
        };
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("no such directory {}: {e}", dir.display()))?;
        if dir != root && !dir.starts_with(&root) {
            return Err(format!(
                "module path escapes workspace root {}: {rel}",
                root.display()
            ));
        }
        if !dir.is_dir() {
            return Err(format!("not a directory: {}", dir.display()));
        }
        Ok(dir)
    }

    /// A validated merge target directly inside the canonical cache.
    pub fn merge_output_path(&self, out_name: &str) -> Result<PathBuf, String> {
        if out_name.is_empty()
            || out_name == "."
            || out_name == ".."
            || out_name.contains(['/', '\\', '\0'])
        {
            return Err(format!("invalid merge output name: {out_name:?}"));
        }
        let mut components = Path::new(out_name).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(format!("invalid merge output name: {out_name:?}"));
        }

        std::fs::create_dir_all(&self.cache)
            .map_err(|e| format!("cannot create cache {}: {e}", self.cache.display()))?;
        let cache = self
            .cache
            .canonicalize()
            .map_err(|e| format!("bad cache {}: {e}", self.cache.display()))?;
        let target = cache.join(format!("{out_name}.cpg"));
        if target.parent() != Some(cache.as_path()) {
            return Err(format!("merge output escapes cache: {out_name:?}"));
        }
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(format!(
                    "merge output is not a regular file: {}",
                    target.display()
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot inspect {}: {e}", target.display())),
        }
        Ok(target)
    }

    /// The cache file a module/lang pair maps to. The canonical module path is
    /// hashed rather than rewritten with separators, so distinct paths cannot
    /// alias and absolute paths are not disclosed in cache filenames.
    pub fn cache_path(&self, dir: &Path, lang: &str) -> Result<PathBuf, String> {
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("bad module {}: {e}", dir.display()))?;
        let key = digest_text(&dir.to_string_lossy());
        Ok(self
            .cache
            .join(format!("module-{key}.{lang}.v{GRAPH_SHAPE_VERSION}.cpg")))
    }

    /// Build-or-reuse the CPG for a module: returns `(cpg_path, lang)`.
    /// Rebuilds unless the graph and its adjacent manifest match the exact
    /// path/content/language/exclude/version snapshot of the sources.
    pub fn ensure_cpg(&self, rel: &str, lang: Option<&str>) -> Result<(PathBuf, String), String> {
        let parsed_lang = match lang {
            Some(lang) => crate::Language::parse(lang)?,
            None => {
                let dir = self.module_dir(rel)?;
                crate::Language::parse(detect_lang(&dir))?
            }
        };
        self.ensure_cpg_with_excludes(rel, parsed_lang, excludes_for(parsed_lang.canonical_name()))
    }

    fn ensure_cpg_with_excludes(
        &self,
        rel: &str,
        parsed_lang: crate::Language,
        excludes: &[&str],
    ) -> Result<(PathBuf, String), String> {
        let dir = self.module_dir(rel)?;
        let lang_name = parsed_lang.canonical_name();
        let lang = lang_name.to_string();
        let cpg_path = self.cache_path(&dir, &lang)?;
        let _lock = acquire_cache_lock(&lock_path(&cpg_path))?;

        // Collect exactly once while holding the per-key lock. These strings
        // are both hashed into the manifest and passed to the frontend, so a
        // concurrent edit cannot make the graph and metadata describe
        // different source snapshots.
        let sources = crate::collect_sources_filtered(&dir, parsed_lang.extensions(), excludes)?;
        let mut manifest = self.source_manifest(&dir, &lang, excludes, &sources)?;
        if cached_pair_is_fresh(&cpg_path, &manifest) {
            return Ok((cpg_path, lang));
        }

        eprintln!(
            "building {} ({lang}) -> {}",
            dir.display(),
            cpg_path.display()
        );
        let project = crate::build_project_from_sources(&lang, &sources, None)?;
        project
            .cpg
            .save(&cpg_path.to_string_lossy())
            .map_err(|e| format!("save failed: {e}"))?;

        manifest.graph_digest = graph_content_digest(&cpg_path)?;
        // Publish the graph first and the manifest last. If the process exits
        // between these operations, the old manifest's graph digest cannot
        // match and the next process rebuilds under the same lock.
        write_manifest_atomically(&manifest_path(&cpg_path), &manifest)?;
        Ok((cpg_path, lang))
    }

    fn source_manifest(
        &self,
        dir: &Path,
        lang: &str,
        excludes: &[&str],
        sources: &[(String, String)],
    ) -> Result<CacheManifest, String> {
        let root = self
            .root
            .canonicalize()
            .map_err(|e| format!("bad root {}: {e}", self.root.display()))?;
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("bad module {}: {e}", dir.display()))?;
        let mut entries = Vec::with_capacity(sources.len());
        for (path, source) in sources {
            let relative = Path::new(path).strip_prefix(&dir).map_err(|_| {
                format!(
                    "collected source is outside module {}: {path}",
                    dir.display()
                )
            })?;
            entries.push(SourceManifestEntry {
                path: relative.to_string_lossy().replace('\\', "/"),
                digest: blake3::hash(source.as_bytes()).to_hex().to_string(),
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let mut excludes: Vec<String> = excludes.iter().map(|s| (*s).to_string()).collect();
        excludes.sort();
        excludes.dedup();
        Ok(CacheManifest {
            manifest_version: CACHE_MANIFEST_VERSION,
            graph_format: CACHE_GRAPH_FORMAT.to_string(),
            graph_shape_version: GRAPH_SHAPE_VERSION,
            workspace_root_digest: digest_text(&root.to_string_lossy()),
            module_path_digest: digest_text(&dir.to_string_lossy()),
            language: lang.to_string(),
            excludes,
            sources: entries,
            graph_digest: String::new(),
        })
    }

    /// Directories under the module containing `.proto` files (for
    /// `--rpc-sources`), vendored trees excluded.
    pub fn proto_dirs(&self, rel: &str) -> Vec<String> {
        match self.module_dir(rel) {
            Ok(dir) => dirs_containing(&dir, "proto", &["/vendor/", "/node_modules/", "/.git/"]),
            Err(_) => Vec::new(),
        }
    }

    /// Directories anywhere under the ROOT containing `.thrift` files (for
    /// `--thrift-sources`). Root-wide because thrift IDL usually lives in a
    /// sibling tree of the code that implements it.
    pub fn thrift_dirs(&self) -> Vec<String> {
        dirs_containing(
            &self.root,
            "thrift",
            &["/vendor/", "/node_modules/", "/.git/"],
        )
    }

    /// Cached CPG files, sorted by name.
    pub fn cached(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(&self.cache)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "cpg"))
            .collect();
        out.sort();
        out
    }
}

fn digest_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Adjacent metadata path for a persisted graph.
pub fn manifest_path(cpg_path: &Path) -> PathBuf {
    let name = cpg_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "graph.cpg".to_string());
    cpg_path.with_file_name(format!("{name}.manifest.json"))
}

fn lock_path(cpg_path: &Path) -> PathBuf {
    let name = cpg_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "graph.cpg".to_string());
    cpg_path.with_file_name(format!("{name}.lock"))
}

fn acquire_cache_lock(path: &Path) -> Result<CacheLock, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(format!(
                "cache lock is not a regular file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect cache lock {}: {error}",
                path.display()
            ))
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("cannot open cache lock {}: {e}", path.display()))?;
    let started = Instant::now();
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(CacheLock { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= LOCK_WAIT {
                    return Err(format!(
                        "timed out after {}s waiting for cache lock {}",
                        LOCK_WAIT.as_secs(),
                        path.display()
                    ));
                }
                std::thread::sleep(LOCK_RETRY);
            }
            Err(error) => {
                return Err(format!("cannot lock cache key {}: {error}", path.display()));
            }
        }
    }
}

fn read_manifest(path: &Path) -> Result<CacheManifest, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
        .len();
    if len > MAX_MANIFEST_BYTES {
        return Err(format!(
            "cache manifest {} is {len} bytes; maximum is {MAX_MANIFEST_BYTES}",
            path.display()
        ));
    }
    let mut text = String::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!(
            "cache manifest {} exceeds {MAX_MANIFEST_BYTES} bytes",
            path.display()
        ));
    }
    serde_json::from_str(&text)
        .map_err(|e| format!("invalid cache manifest {}: {e}", path.display()))
}

fn write_manifest_atomically(path: &Path, manifest: &CacheManifest) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("cannot serialize cache manifest: {e}"))?;
    bytes.push(b'\n');
    let mut temporary = tempfile::Builder::new()
        .prefix(".cpg-manifest-tmp-")
        .tempfile_in(parent)
        .map_err(|e| {
            format!(
                "cannot create temporary manifest for {}: {e}",
                path.display()
            )
        })?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| {
            format!(
                "cannot write temporary manifest for {}: {e}",
                path.display()
            )
        })?;
    temporary.persist(path).map_err(|e| {
        format!(
            "cannot publish cache manifest {}: {}",
            path.display(),
            e.error
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| format!("cannot sync cache directory {}: {e}", parent.display()))
}

fn is_cpg2(path: &Path) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)
        .map_err(|e| format!("cannot read graph header {}: {e}", path.display()))?;
    Ok(&magic == b"CPG2")
}

/// Stream a persisted graph through BLAKE3 without loading a second copy into
/// memory. MCP uses this identity to invalidate an in-process reopened graph
/// even on filesystems whose mtimes are coarse.
pub(crate) fn graph_content_digest(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
        .len();
    if len > cpg_core::graph::MAX_CPG_BYTES {
        return Err(format!(
            "graph {} is {len} bytes; maximum is {}",
            path.display(),
            cpg_core::graph::MAX_CPG_BYTES
        ));
    }
    let mut file = file.take(cpg_core::graph::MAX_CPG_BYTES + 1);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > cpg_core::graph::MAX_CPG_BYTES {
            return Err(format!(
                "graph {} exceeds {} bytes",
                path.display(),
                cpg_core::graph::MAX_CPG_BYTES
            ));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn validate_manifest_graph(path: &Path, manifest: &CacheManifest) -> Result<(), String> {
    let canonical_language = crate::Language::parse(&manifest.language)?;
    let sources_are_canonical = !manifest.sources.is_empty()
        && manifest.sources.iter().all(|source| {
            !source.path.is_empty()
                && !source.path.contains('\\')
                && Path::new(&source.path)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
                && valid_digest(&source.digest)
        })
        && manifest
            .sources
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path);
    let excludes_are_canonical = manifest.excludes.windows(2).all(|pair| pair[0] < pair[1]);
    if manifest.manifest_version != CACHE_MANIFEST_VERSION
        || manifest.graph_format != CACHE_GRAPH_FORMAT
        || manifest.graph_shape_version != GRAPH_SHAPE_VERSION
        || canonical_language.canonical_name() != manifest.language
        || !valid_digest(&manifest.workspace_root_digest)
        || !valid_digest(&manifest.module_path_digest)
        || !valid_digest(&manifest.graph_digest)
        || !sources_are_canonical
        || !excludes_are_canonical
    {
        return Err(format!(
            "cache manifest for {} has unsupported or incomplete version metadata",
            path.display()
        ));
    }
    if !is_cpg2(path)? {
        return Err(format!("cached graph {} is not CPG2", path.display()));
    }
    let actual = graph_content_digest(path)?;
    if actual != manifest.graph_digest {
        return Err(format!(
            "cached graph digest mismatch for {}",
            path.display()
        ));
    }
    cpg_core::Cpg::load(&path.to_string_lossy())
        .map_err(|e| format!("invalid cached graph {}: {e}", path.display()))?;
    Ok(())
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn cached_pair_is_fresh(path: &Path, expected: &CacheManifest) -> bool {
    let manifest = match read_manifest(&manifest_path(path)) {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    manifest.source_state_matches(expected) && validate_manifest_graph(path, &manifest).is_ok()
}

/// Resolve the frontend for a reopened graph. Cache filenames are never
/// parsed: the caller must provide a supported language or the graph must have
/// valid adjacent metadata. If both exist they must agree.
pub fn language_for_cpg(path: &Path, explicit: Option<&str>) -> Result<String, String> {
    let explicit = explicit
        .map(crate::Language::parse)
        .transpose()?
        .map(|language| language.canonical_name().to_string());
    let metadata_path = manifest_path(path);
    if !metadata_path.exists() {
        return explicit.ok_or_else(|| {
            format!(
                "--load {} has no validated language metadata; pass --lang <language>",
                path.display()
            )
        });
    }
    let manifest = read_manifest(&metadata_path)?;
    validate_manifest_graph(path, &manifest)?;
    if let Some(explicit) = explicit {
        if explicit != manifest.language {
            return Err(format!(
                "--lang {explicit} conflicts with cached graph language {} for {}",
                manifest.language,
                path.display()
            ));
        }
        return Ok(explicit);
    }
    Ok(manifest.language)
}

/// Auto-detect the dominant language of a directory by counting source files
/// per extension (vendored/generated trees skipped). Ties break toward the
/// earlier entry; an empty directory falls back to "c".
pub fn detect_lang(dir: &Path) -> &'static str {
    const EXT_LANG: &[(&str, &str)] = &[
        ("go", "go"),
        ("scala", "scala"),
        ("py", "python"),
        ("java", "java"),
        ("js", "javascript"),
        ("ts", "typescript"),
        ("c", "c"),
        ("rs", "rust"),
        ("rb", "ruby"),
        ("cpp", "cpp"),
        ("cc", "cpp"),
    ];
    let mut counts = vec![0usize; EXT_LANG.len()];
    walk(
        dir,
        &["/vendor/", "/node_modules/", "/.git/"],
        &mut |path| {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                for (i, (e, _)) in EXT_LANG.iter().enumerate() {
                    if *e == ext {
                        counts[i] += 1;
                    }
                }
            }
        },
    );
    let mut best = "c";
    let mut best_n = 0usize;
    for (i, (_, lang)) in EXT_LANG.iter().enumerate() {
        if counts[i] > best_n {
            best_n = counts[i];
            best = lang;
        }
    }
    best
}

/// The exclude set for a language: vendored, generated, and test code has no
/// place in a security CPG (it dominates file counts and drowns findings).
pub fn excludes_for(lang: &str) -> &'static [&'static str] {
    match lang {
        "go" => &[
            "/vendor/",
            "_test.go",
            ".pb.go",
            "/mocks/",
            "/testdata/",
            "/tests/",
            "/node_modules/",
        ],
        "scala" => &["/target/", "Test.scala", "Spec.scala", "/node_modules/"],
        "python" => &[
            "/vendor/",
            "_pb2.py",
            "/node_modules/",
            "_test.py",
            "/testdata/",
            "/test/",
            "/tests/",
            "conftest.py",
            "test_",
        ],
        "java" => &["/target/", "/build/", "Test.java", "/node_modules/"],
        "javascript" | "js" => &[
            "/node_modules/",
            ".min.js",
            "/dist/",
            ".test.js",
            ".spec.js",
        ],
        "typescript" | "ts" => &[
            "/node_modules/",
            "/dist/",
            "/build/",
            ".test.ts",
            ".spec.ts",
            ".d.ts",
        ],
        "cpp" | "c++" | "cxx" => &[
            "/thirdparty/",
            "/third-party/",
            "/third_party/",
            "/build/",
            "/generated/",
            "_test.",
            "/test/",
            "/tests/",
            "/testdata/",
            "/node_modules/",
            "/test_util/",
            "test_util.",
            "test_utils.",
            "/simulator/",
            "/mock/",
            "mock_",
            "_mock.",
            "flaky_",
            "/GUnitTest/",
            "Tests.cpp",
            "Test.cpp",
            "/Mock",
            "Mock.h",
            "Mocks.h",
        ],
        _ => &["/vendor/", "/node_modules/"],
    }
}

/// Sorted, deduplicated parent directories of every `*.{ext}` file under
/// `dir` (skipping `excludes` path substrings).
fn dirs_containing(dir: &Path, ext: &str, excludes: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    walk(dir, excludes, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            if let Some(parent) = path.parent() {
                out.push(parent.to_string_lossy().into_owned());
            }
        }
    });
    out.sort();
    out.dedup();
    out
}

/// Depth-first file walk, skipping paths containing any `excludes` substring.
/// Does not follow directory symlinks (cycle safety).
fn walk(dir: &Path, excludes: &[&str], visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if excludes.iter().any(|e| path_str.contains(e)) {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            walk(&path, excludes, visit);
        } else if ft.is_file() {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cpg-ws-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detect_lang_counts_dominant_extension() {
        let d = tmpdir("detect");
        std::fs::write(d.join("a.go"), "package a").unwrap();
        std::fs::write(d.join("b.go"), "package a").unwrap();
        std::fs::write(d.join("c.py"), "x = 1").unwrap();
        assert_eq!(detect_lang(&d), "go");
        // vendored files must not count
        std::fs::create_dir_all(d.join("vendor")).unwrap();
        for i in 0..5 {
            std::fs::write(d.join("vendor").join(format!("v{i}.py")), "x").unwrap();
        }
        assert_eq!(detect_lang(&d), "go");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn manifest_cache_key_hashes_canonical_path_lang_and_shape_version() {
        let root = tmpdir("key-root");
        let cache = tmpdir("key-cache");
        let left = root.join("a_b/c");
        let right = root.join("a/b_c");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        let p = ws.cache_path(&left, "go").unwrap();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("module-"), "{name}");
        assert!(name.ends_with(&format!(".go.v{GRAPH_SHAPE_VERSION}.cpg")));
        assert!(!name.contains("a_b"), "absolute path leaked into {name}");
        assert_ne!(p, ws.cache_path(&right, "go").unwrap());
        assert_ne!(p, ws.cache_path(&left, "python").unwrap());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn workspace_module_paths_are_normal_and_root_confined() {
        let root = tmpdir("module-root");
        let cache = tmpdir("module-cache");
        std::fs::create_dir_all(root.join("nested/module")).unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        assert_eq!(ws.module_dir(".").unwrap(), root.canonicalize().unwrap());
        assert_eq!(
            ws.module_dir("nested/module").unwrap(),
            root.join("nested/module").canonicalize().unwrap()
        );
        for invalid in ["", "..", "nested/../nested", "nested/./module"] {
            assert!(ws.module_dir(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(ws.module_dir(&root.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_module_paths_reject_outward_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tmpdir("module-symlink-root");
        let outside = tmpdir("module-symlink-outside");
        let cache = tmpdir("module-symlink-cache");
        symlink(&outside, root.join("outside-link")).unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        let error = ws.module_dir("outside-link").unwrap_err();
        assert!(error.contains("escapes workspace root"), "{error}");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn workspace_merge_output_accepts_only_safe_regular_targets() {
        let root = tmpdir("merge-root");
        let cache = tmpdir("merge-cache");
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        assert_eq!(
            ws.merge_output_path("combined").unwrap(),
            cache.canonicalize().unwrap().join("combined.cpg")
        );
        std::fs::write(cache.join("existing.cpg"), b"CPG1").unwrap();
        assert!(ws.merge_output_path("existing").is_ok());
        for invalid in ["", ".", "..", "nested/out", "nested\\out", "bad\0name"] {
            assert!(
                ws.merge_output_path(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        std::fs::create_dir(cache.join("directory.cpg")).unwrap();
        assert!(ws.merge_output_path("directory").is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_merge_output_rejects_symlink_target() {
        use std::os::unix::fs::symlink;

        let root = tmpdir("merge-link-root");
        let cache = tmpdir("merge-link-cache");
        let outside = root.join("outside.cpg");
        std::fs::write(&outside, b"do not overwrite").unwrap();
        symlink(&outside, cache.join("escaped.cpg")).unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        assert!(ws.merge_output_path("escaped").is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"do not overwrite");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn manifest_is_order_independent_and_covers_all_identity_inputs() {
        let root = tmpdir("manifest-root");
        let cache = tmpdir("manifest-cache");
        let module = root.join("m");
        std::fs::create_dir_all(&module).unwrap();
        let module = module.canonicalize().unwrap();
        let a = module.join("a.c").to_string_lossy().into_owned();
        let b = module.join("b.c").to_string_lossy().into_owned();
        let ws = Workspace {
            root: root.clone(),
            cache: cache.clone(),
        };
        let one = ws
            .source_manifest(
                &module,
                "c",
                &["/vendor/", "/tests/"],
                &[(a.clone(), "a".into()), (b.clone(), "b".into())],
            )
            .unwrap();
        let reordered = ws
            .source_manifest(
                &module,
                "c",
                &["/tests/", "/vendor/"],
                &[(b.clone(), "b".into()), (a.clone(), "a".into())],
            )
            .unwrap();
        assert_eq!(one, reordered);
        assert_eq!(
            serde_json::to_vec(&one).unwrap(),
            serde_json::to_vec(&reordered).unwrap()
        );

        let mut changed = one.clone();
        changed.sources[0].digest = digest_text("changed");
        assert!(!one.source_state_matches(&changed));
        changed = one.clone();
        changed.sources[0].path = "renamed.c".into();
        assert!(!one.source_state_matches(&changed));
        changed = one.clone();
        changed.language = "python".into();
        assert!(!one.source_state_matches(&changed));
        changed = one.clone();
        changed.excludes.push("/generated/".into());
        assert!(!one.source_state_matches(&changed));
        changed = one.clone();
        changed.graph_shape_version += 1;
        assert!(!one.source_state_matches(&changed));
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(cache);
    }

    fn cached_manifest(path: &Path) -> CacheManifest {
        read_manifest(&manifest_path(path)).unwrap()
    }

    #[test]
    fn manifest_cache_reuses_then_rebuilds_for_content_deletion_and_rename() {
        let root = tmpdir("root");
        let cache = tmpdir("cache");
        std::fs::create_dir_all(root.join("m")).unwrap();
        std::fs::write(root.join("m/a.c"), "int main() { return 0; }").unwrap();
        std::fs::write(root.join("m/b.c"), "int b() { return 0; }").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache,
        };
        let (cpg1, lang) = ws.ensure_cpg("m", None).expect("build");
        assert_eq!(lang, "c");
        assert!(cpg1.exists());
        let first = cached_manifest(&cpg1);
        // unchanged source -> cache reused
        let (cpg2, _) = ws.ensure_cpg("m", None).expect("reuse");
        assert_eq!(cached_manifest(&cpg2), first);

        // A content edit with the original timestamp still invalidates.
        let original_time = std::fs::metadata(root.join("m/a.c"))
            .unwrap()
            .modified()
            .unwrap();
        std::fs::write(root.join("m/a.c"), "int main() { return 1; }").unwrap();
        let f = std::fs::File::options()
            .write(true)
            .open(root.join("m/a.c"))
            .unwrap();
        f.set_modified(original_time).unwrap();
        drop(f);
        ws.ensure_cpg("m", None).expect("content rebuild");
        let after_content = cached_manifest(&cpg1);
        assert_ne!(after_content.sources, first.sources);

        std::fs::remove_file(root.join("m/b.c")).unwrap();
        ws.ensure_cpg("m", None).expect("deletion rebuild");
        let after_delete = cached_manifest(&cpg1);
        assert_ne!(after_delete.sources, after_content.sources);

        std::fs::rename(root.join("m/a.c"), root.join("m/renamed.c")).unwrap();
        ws.ensure_cpg("m", None).expect("rename rebuild");
        let after_rename = cached_manifest(&cpg1);
        assert_ne!(after_rename.sources, after_delete.sources);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ws.cache);
    }

    #[test]
    fn manifest_cache_rebuilds_for_policy_and_broken_pairs() {
        let root = tmpdir("broken-root");
        let cache = tmpdir("broken-cache");
        std::fs::create_dir_all(root.join("m/ignored")).unwrap();
        std::fs::write(root.join("m/a.c"), "int a() { return 0; }").unwrap();
        std::fs::write(root.join("m/ignored/b.c"), "int b() { return 0; }").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache,
        };
        let (cpg, _) = ws.ensure_cpg_with_excludes("m", Language::C, &[]).unwrap();
        let with_all = cached_manifest(&cpg);
        ws.ensure_cpg_with_excludes("m", Language::C, &["/ignored/"])
            .unwrap();
        assert_ne!(cached_manifest(&cpg).excludes, with_all.excludes);

        std::fs::write(&cpg, b"not a graph").unwrap();
        ws.ensure_cpg_with_excludes("m", Language::C, &["/ignored/"])
            .expect("corrupt graph rebuilds");
        assert!(is_cpg2(&cpg).unwrap());

        std::fs::write(manifest_path(&cpg), b"not json").unwrap();
        ws.ensure_cpg_with_excludes("m", Language::C, &["/ignored/"])
            .expect("corrupt manifest rebuilds");
        validate_manifest_graph(&cpg, &cached_manifest(&cpg)).unwrap();

        std::fs::remove_file(manifest_path(&cpg)).unwrap();
        ws.ensure_cpg_with_excludes("m", Language::C, &["/ignored/"])
            .expect("missing manifest rebuilds");
        std::fs::remove_file(&cpg).unwrap();
        ws.ensure_cpg_with_excludes("m", Language::C, &["/ignored/"])
            .expect("missing graph rebuilds");
        validate_manifest_graph(&cpg, &cached_manifest(&cpg)).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ws.cache);
    }

    #[test]
    fn manifest_cache_serializes_concurrent_builders() {
        let root = tmpdir("concurrent-root");
        let cache = tmpdir("concurrent-cache");
        std::fs::create_dir_all(root.join("m")).unwrap();
        std::fs::write(root.join("m/a.c"), "int main() { return 0; }").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache,
        };
        let first = ws.clone();
        let second = ws.clone();
        let a = std::thread::spawn(move || first.ensure_cpg("m", Some("c")));
        let b = std::thread::spawn(move || second.ensure_cpg("m", Some("c")));
        let (a_path, _) = a.join().unwrap().unwrap();
        let (b_path, _) = b.join().unwrap().unwrap();
        assert_eq!(a_path, b_path);
        validate_manifest_graph(&a_path, &cached_manifest(&a_path)).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ws.cache);
    }

    #[test]
    fn manifest_language_is_validated_and_conflicts_fail() {
        let root = tmpdir("language-root");
        let cache = tmpdir("language-cache");
        std::fs::create_dir_all(root.join("m")).unwrap();
        std::fs::write(root.join("m/a.py"), "def f():\n    return 1\n").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache,
        };
        let (cpg, lang) = ws.ensure_cpg("m", Some("python")).unwrap();
        assert_eq!(lang, "python");
        assert_eq!(language_for_cpg(&cpg, None).unwrap(), "python");
        let conflict = language_for_cpg(&cpg, Some("c")).unwrap_err();
        assert!(conflict.contains("conflicts"), "{conflict}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ws.cache);
    }

    #[test]
    fn idl_discovery_finds_proto_and_thrift_dirs() {
        let root = tmpdir("idl");
        std::fs::create_dir_all(root.join("svc/protos")).unwrap();
        std::fs::create_dir_all(root.join("idl/thrift")).unwrap();
        std::fs::create_dir_all(root.join("vendor/protos")).unwrap();
        std::fs::write(root.join("svc/protos/a.proto"), "syntax = \"proto3\";").unwrap();
        std::fs::write(root.join("idl/thrift/b.thrift"), "service S {}").unwrap();
        std::fs::write(root.join("vendor/protos/c.proto"), "syntax = \"proto3\";").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache: tmpdir("idl-cache"),
        };
        let protos = ws.proto_dirs(".");
        assert_eq!(protos.len(), 1, "vendored protos excluded: {protos:?}");
        assert!(protos[0].ends_with("svc/protos"));
        let thrifts = ws.thrift_dirs();
        assert_eq!(thrifts.len(), 1);
        assert!(thrifts[0].ends_with("idl/thrift"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ws.cache);
    }
}
