//! Workspace driver: root-relative CPG analysis for ANY repository (`cpg x`).
//!
//! This is the former `cpgx` shell wrapper absorbed into the binary, so a
//! single self-contained executable runs the whole IRIS loop anywhere:
//! module paths relative to a project root, language auto-detection by file
//! census, per-language exclude sets for vendored/generated/test code, a
//! versioned mtime CPG cache, and gRPC/thrift IDL auto-discovery for
//! entry-point mining.
//!
//! Nothing here is tied to a machine or project: the root comes from
//! `-C`/`$CPGX_ROOT`/cwd, the cache from `$CPG_CACHE`/`$XDG_CACHE_HOME/cpg`/
//! `~/.cache/cpg`.

use std::path::{Path, PathBuf};

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

/// A project root plus the CPG cache directory.
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
        Ok(Workspace { root, cache })
    }

    /// Absolute directory for a root-relative module path (`.` = the root).
    pub fn module_dir(&self, rel: &str) -> Result<PathBuf, String> {
        let dir = if rel == "." {
            self.root.clone()
        } else {
            self.root.join(rel)
        };
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("no such directory {}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("not a directory: {}", dir.display()));
        }
        Ok(dir)
    }

    /// The cache file a module/lang pair maps to. Keyed by sanitized absolute
    /// path (same module name in two projects never collides), language, and
    /// [`GRAPH_SHAPE_VERSION`].
    pub fn cache_path(&self, dir: &Path, lang: &str) -> PathBuf {
        let key: String = dir
            .to_string_lossy()
            .trim_start_matches('/')
            .chars()
            .map(|c| if c == '/' || c == '\\' { '_' } else { c })
            .collect();
        self.cache
            .join(format!("{key}.{lang}.v{GRAPH_SHAPE_VERSION}.cpg"))
    }

    /// Build-or-reuse the CPG for a module: returns `(cpg_path, lang)`.
    /// Rebuilds when the cache is missing or any in-scope source file is
    /// newer than it.
    pub fn ensure_cpg(&self, rel: &str, lang: Option<&str>) -> Result<(PathBuf, String), String> {
        let dir = self.module_dir(rel)?;
        let lang = match lang {
            Some(l) => l.to_string(),
            None => detect_lang(&dir).to_string(),
        };
        let cpg_path = self.cache_path(&dir, &lang);
        let excludes = excludes_for(&lang);
        let stale = match std::fs::metadata(&cpg_path).and_then(|m| m.modified()) {
            Err(_) => true, // no cache yet
            Ok(cache_mtime) => {
                let exts = crate::make_project(&lang).1;
                newest_source_mtime(&dir, exts, excludes).is_some_and(|src| src > cache_mtime)
            }
        };
        if stale {
            eprintln!(
                "building {} ({lang}) -> {}",
                dir.display(),
                cpg_path.display()
            );
            let project = crate::build_project_filtered(&dir.to_string_lossy(), &lang, excludes);
            project
                .cpg
                .save(&cpg_path.to_string_lossy())
                .map_err(|e| format!("save failed: {e}"))?;
        }
        Ok((cpg_path, lang))
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

/// The language a cache file was built for (`<key>.<lang>.v<N>.cpg`).
pub fn lang_of_cpg(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parts: Vec<&str> = name.split('.').collect();
    // strip trailing "cpg" and "v<N>"
    parts.pop();
    if parts.last().is_some_and(|p| p.starts_with('v')) {
        parts.pop();
    }
    parts.pop().unwrap_or("c").to_string()
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

/// Newest modification time of any file under `dir` with one of `exts`,
/// skipping `excludes` substrings. None when no such file exists.
fn newest_source_mtime(
    dir: &Path,
    exts: &[&str],
    excludes: &[&str],
) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    walk(dir, excludes, &mut |path| {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            if let Ok(m) = std::fs::metadata(path).and_then(|m| m.modified()) {
                if newest.is_none_or(|n| m > n) {
                    newest = Some(m);
                }
            }
        }
    });
    newest
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
    fn cache_key_embeds_path_lang_and_shape_version() {
        let ws = Workspace {
            root: PathBuf::from("/"),
            cache: PathBuf::from("/tmp/cache"),
        };
        let p = ws.cache_path(Path::new("/some/repo/module"), "go");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            name,
            format!("some_repo_module.go.v{GRAPH_SHAPE_VERSION}.cpg")
        );
        assert_eq!(lang_of_cpg(&p), "go");
    }

    #[test]
    fn ensure_cpg_builds_then_reuses_then_rebuilds_on_edit() {
        let root = tmpdir("root");
        let cache = tmpdir("cache");
        std::fs::create_dir_all(root.join("m")).unwrap();
        std::fs::write(root.join("m/a.c"), "int main() { return 0; }").unwrap();
        let ws = Workspace {
            root: root.clone(),
            cache,
        };
        let (cpg1, lang) = ws.ensure_cpg("m", None).expect("build");
        assert_eq!(lang, "c");
        assert!(cpg1.exists());
        let mtime1 = std::fs::metadata(&cpg1).unwrap().modified().unwrap();
        // unchanged source -> cache reused
        let (cpg2, _) = ws.ensure_cpg("m", None).expect("reuse");
        assert_eq!(
            std::fs::metadata(&cpg2).unwrap().modified().unwrap(),
            mtime1
        );
        // touch the source into the future -> rebuild
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let f = std::fs::File::options()
            .write(true)
            .open(root.join("m/a.c"))
            .unwrap();
        f.set_modified(future).unwrap();
        drop(f);
        let (cpg3, _) = ws.ensure_cpg("m", None).expect("rebuild");
        assert!(std::fs::metadata(&cpg3).unwrap().modified().unwrap() > mtime1);
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
