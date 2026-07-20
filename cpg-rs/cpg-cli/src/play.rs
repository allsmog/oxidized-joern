//! Play Framework routes mining -> curated taint entries.
//!
//! Play's `conf/routes` is a code-first HTTP IDL: each line binds
//! `VERB /path package.Controller.method(args)`. HTTP/websocket services
//! have no protobuf or thrift to mine, so without this every Play app scans
//! with zero framework entries. Mining is file-syntax only (no Play
//! dependency) and resolution is against the graph's (class, method) pairs
//! — the same rule the thrift stitch uses — so it works for any Play app,
//! Scala or Java.

use cpg_core::{Cpg, Query};

#[derive(Debug, Clone)]
pub struct PlayRoute {
    pub verb: String,
    pub path: String,
    /// Dotted controller reference as written (`controllers.Api`).
    pub controller: String,
    pub method: String,
}

const VERBS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

/// Parse one routes file. Line-based: `VERB` must be a known HTTP method and
/// the path must start with `/` — together those keep non-Play files that
/// happen to be named `routes` from producing garbage. Comment (`#`),
/// modifier (`+ nocsrf`), and sub-router include (`->`) lines are skipped;
/// includes point at another `.routes` file which the directory scan picks
/// up on its own.
pub fn parse_routes(src: &str, out: &mut Vec<PlayRoute>) {
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('+') || t.starts_with("->") {
            continue;
        }
        let Some((verb, rest)) = t.split_once(char::is_whitespace) else { continue };
        if !VERBS.contains(&verb) {
            continue;
        }
        let rest = rest.trim_start();
        let Some((path, action)) = rest.split_once(char::is_whitespace) else { continue };
        if !path.starts_with('/') {
            continue;
        }
        // `@controllers.Api.m` = injected-instance reference; the `@` is
        // routing syntax, not part of the name.
        let action = action.trim_start().trim_start_matches('@');
        let target = action.split('(').next().unwrap_or("").trim();
        let Some((controller, method)) = target.rsplit_once('.') else { continue };
        if controller.is_empty()
            || method.is_empty()
            || !method.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        out.push(PlayRoute {
            verb: verb.to_string(),
            path: path.to_string(),
            controller: controller.to_string(),
            method: method.to_string(),
        });
    }
}

/// All routes under `path` (recursive): files named exactly `routes` or
/// `*.routes` (sub-router convention). A file path parses directly.
pub fn play_routes(path: &std::path::Path, out: &mut Vec<PlayRoute>) {
    if path.is_file() {
        if let Ok(src) = std::fs::read_to_string(path) {
            parse_routes(&src, out);
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if name == "target" || name == "node_modules" || name == ".git" || name == "vendor" {
                continue;
            }
            play_routes(&p, out);
        } else if name == "routes" || name.ends_with(".routes") {
            if let Ok(src) = std::fs::read_to_string(&p) {
                parse_routes(&src, out);
            }
        }
    }
}

/// Resolve routes against the graph: a route matches a method whose simple
/// name is the route's method and whose qualified full name is
/// `Class<delim>method` for the route's controller simple name (both `.` and
/// `::` delimiters accepted, so the graph's language decides). Returns the
/// full-name entries (the form `--entry` matches verbatim) plus the count of
/// routes with no graph method — the honesty number for the coverage report.
pub fn play_entries(cpg: &Cpg, routes: &[PlayRoute]) -> (Vec<String>, usize) {
    use std::collections::{HashMap, HashSet};
    // (class, method) -> full names present in the graph.
    let mut by_pair: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for m in cpg.methods() {
        let (Some(name), Some(full)) = (cpg.name_of(m), cpg.full_name_of(m)) else { continue };
        let Some(prefix) = full.strip_suffix(name) else { continue };
        let Some(class) = prefix.strip_suffix("::").or_else(|| prefix.strip_suffix('.')) else {
            continue;
        };
        // Nested qualifiers (`pkg.Outer.Inner`): the route names the
        // innermost class, so key on the last segment.
        let class = class.rsplit(['.', ':']).next().unwrap_or(class);
        if class.is_empty() {
            continue;
        }
        by_pair
            .entry((class.to_string(), name.to_string()))
            .or_default()
            .insert(full.to_string());
    }
    let mut out: Vec<String> = Vec::new();
    let mut unresolved = 0usize;
    for r in routes {
        let class = r.controller.rsplit('.').next().unwrap_or(&r.controller);
        match by_pair.get(&(class.to_string(), r.method.clone())) {
            Some(fulls) => out.extend(fulls.iter().cloned()),
            None => unresolved += 1,
        }
    }
    out.sort();
    out.dedup();
    (out, unresolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_routes_handles_play_syntax() {
        let src = r#"
# comment line
GET     /api/session            controllers.Api.session
POST    /api/saml/:id/response  @controllers.SsoController.samlResponse(id: String)
+ nocsrf
POST    /api/csrf_exempt        controllers.Api.csrfExempt(kind: Option[String] ?= None)
->      /sub                    sub.Routes
GET     /assets/*file           controllers.Assets.versioned(path="/public", file: Asset)
NOTAVERB /x                     controllers.Api.nope
GET     notapath                controllers.Api.nope
GET     /noaction
"#;
        let mut routes = Vec::new();
        parse_routes(src, &mut routes);
        let got: Vec<(&str, &str, &str)> = routes
            .iter()
            .map(|r| (r.verb.as_str(), r.controller.as_str(), r.method.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("GET", "controllers.Api", "session"),
                ("POST", "controllers.SsoController", "samlResponse"),
                ("POST", "controllers.Api", "csrfExempt"),
                ("GET", "controllers.Assets", "versioned"),
            ]
        );
    }

    #[test]
    fn play_entries_resolve_against_graph_full_names() {
        use cpg_core::CpgBuilder;
        let mut cpg = Cpg::new();
        let f = cpg.file_id("app/controllers/Api.scala");
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            b.method("session", "Api.session", "", Some(10));
            b.method("session", "SessionStore.session", "", Some(90)); // same name, wrong class
            b.method("helper", "Api.helper", "", Some(20)); // not routed
        }
        let routes = vec![
            PlayRoute {
                verb: "GET".into(),
                path: "/api/session".into(),
                controller: "controllers.Api".into(),
                method: "session".into(),
            },
            PlayRoute {
                verb: "GET".into(),
                path: "/assets".into(),
                controller: "controllers.Assets".into(),
                method: "versioned".into(), // library controller: not in graph
            },
        ];
        let (entries, unresolved) = play_entries(&cpg, &routes);
        assert_eq!(entries, vec!["Api.session".to_string()]);
        assert_eq!(unresolved, 1);
    }
}
