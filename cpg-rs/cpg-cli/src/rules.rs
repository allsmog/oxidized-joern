//! Declarative rule packs: named source→sink taint specs with CWE/severity
//! metadata, loaded from JSON (Gap 5 — the querydb/joern-scan equivalent).
//!
//! Format (serde is tolerant of unknown keys so the format can grow):
//!
//! ```json
//! {"rules":[{"id":"CPG-001","name":"env-to-system","cwe":"CWE-78",
//!   "description":"environment variable reaches command execution",
//!   "severity":"high",
//!   "sources":["getenv"],"sinks":["system","popen"]}]}
//! ```

use serde::Deserialize;

/// A pack of rules loaded from one JSON document.
#[derive(Debug, Clone, Deserialize)]
pub struct RulePack {
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Convention entry patterns (`NAMEPAT[@FILEPAT]`), applied at scan time
    /// exactly like `--entry-glob` — the pack-carried hook for code-first
    /// frameworks with no IDL to mine (GraphQL resolver directories,
    /// wire-protocol connection handlers). Empty for packs whose entries
    /// come from IDL mining or explicit flags.
    #[serde(default, rename = "entryGlobs", alias = "entry_globs")]
    pub entry_globs: Vec<String>,
    /// Caller-context marker phrases used by the authorization census.
    /// Absent preserves the engine defaults; an explicit empty array disables
    /// the caller-context verdict tier.
    #[serde(
        default,
        rename = "callerContextMarkers",
        alias = "caller_context_markers"
    )]
    pub caller_context_markers: Option<Vec<String>>,
    /// Service-framework constructor call names reported by the authorization
    /// census as non-enforcing framework evidence. Absent preserves the engine
    /// defaults; an explicit empty array disables this evidence.
    #[serde(
        default,
        rename = "frameworkServerCalls",
        alias = "framework_server_calls"
    )]
    pub framework_server_calls: Option<Vec<String>>,
}

/// One named taint rule. Every field except `id` has a sensible default so
/// packs stay terse; unknown keys are ignored (no `deny_unknown_fields`),
/// which lets the format grow without breaking older binaries.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Stable rule identifier (becomes SARIF `ruleId`), e.g. "CPG-001".
    pub id: String,
    /// Rule kind. Empty or "taint" (the default) = interprocedural
    /// source→sink taint query. Structural kinds reinterpret the name lists:
    /// "forbidden-call" — flag every call named in `sinks`, without requiring
    /// a taint path; "discarded-return" — flag calls named in `sinks` whose multi-assign
    /// binds a blank `_` (verified-value-discarded shape);
    /// "append-without-delete" — flag `sinks`-named calls appending a
    /// constant key matching `sources` with no `sanitizers`-named call
    /// clearing that key in the same method (duplicate-header smuggling
    /// shape). Unknown kinds produce no findings (a warning, not an error,
    /// so newer packs degrade gracefully on older binaries).
    #[serde(default)]
    pub kind: String,
    /// Short human name, e.g. "env-to-system".
    #[serde(default)]
    pub name: String,
    /// One-line description of the weakness the rule detects.
    #[serde(default)]
    pub description: String,
    /// CWE tag, e.g. "CWE-78".
    #[serde(default)]
    pub cwe: Option<String>,
    /// One of critical|high|medium|low (free-form; mapped to a SARIF level).
    #[serde(default = "default_severity")]
    pub severity: String,
    /// Calls to these names produce tainted values.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Calls to these names are dangerous when reached by a tainted argument.
    #[serde(default)]
    pub sinks: Vec<String>,
    /// Function names that neutralise taint: a flow whose only path passes
    /// through one is not reported. Threaded into the query via
    /// `Project::find_taint_with_sanitizers` (see `scan::run_pack`).
    #[serde(default)]
    pub sanitizers: Vec<String>,
    /// Entry-point methods for this rule: every parameter of a method with
    /// one of these names is attacker-controlled (RPC/handler model). Merged
    /// with any entry methods given on the command line (`--rpc-sources`,
    /// `--entry`), so an LLM-inferred spec is one self-contained document.
    #[serde(default, rename = "entryMethods", alias = "entry_methods")]
    pub entry_methods: Vec<String>,
    /// Identifiers tainted at every read — framework globals like Flask's
    /// `request` or `sys.argv`, which arrive through neither a call nor a
    /// handler parameter.
    #[serde(default, rename = "sourceIdents", alias = "source_idents")]
    pub source_idents: Vec<String>,
    /// Authorization-check call names for this codebase (`CheckClusterAccess`,
    /// `enforcePolicy`, ...). Purely advisory: they extend the built-in authz
    /// name heuristic for the authz-dominance annotation on each finding
    /// (`authz-dominated@` / `authz-partial@` / absent) — never suppression.
    #[serde(default, alias = "authzMethods", alias = "authz_methods")]
    pub authz: Vec<String>,
    /// Component-confiner names for this rule (`QueryEscape`, `RawQuery`,
    /// `Encode`, ...): calls or member-store fields through which taint
    /// passing means its placement at the sink is structurally confined —
    /// for authority-sensitive sinks (SSRF), query/path writes on a fixed
    /// host. Purely advisory: findings on such a path gain
    /// `confined@<line>:<name>` and triage after unconfined flows — never
    /// suppression.
    #[serde(default, alias = "confinerMethods", alias = "confiner_methods")]
    pub confiners: Vec<String>,
}

fn default_severity() -> String {
    "medium".to_string()
}

impl RulePack {
    /// Parse a rule pack from a JSON string.
    pub fn from_json(json: &str) -> Result<RulePack, String> {
        serde_json::from_str(json).map_err(|e| format!("invalid rule pack: {e}"))
    }

    /// Load a rule pack from a JSON file on disk.
    pub fn from_file(path: &str) -> Result<RulePack, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read rules file {path}: {e}"))?;
        RulePack::from_json(&text)
    }

    /// Resolve a `--rules` argument: `iris:<name>` selects a compiled-in IRIS
    /// pack, anything else is a path to a JSON file on disk.
    pub fn resolve(arg: &str) -> Result<RulePack, String> {
        match arg.strip_prefix("iris:") {
            Some(name) => iris_pack(name).ok_or_else(|| {
                let names: Vec<&str> = IRIS_PACKS.iter().map(|(n, _)| *n).collect();
                format!(
                    "no IRIS pack named '{name}'; available: {}",
                    names.join(", ")
                )
            }),
            None => RulePack::from_file(arg),
        }
    }

    /// Resolve optional pack-level authorization-census conventions. Missing
    /// fields inherit engine defaults, while explicit empty arrays stay empty.
    pub fn authz_census_config(&self) -> cpg_analysis::AuthzCensusConfig {
        let defaults = cpg_analysis::AuthzCensusConfig::default();
        cpg_analysis::AuthzCensusConfig {
            caller_context_markers: self
                .caller_context_markers
                .clone()
                .unwrap_or(defaults.caller_context_markers),
            framework_server_calls: self
                .framework_server_calls
                .clone()
                .unwrap_or(defaults.framework_server_calls),
        }
    }
}

/// The IRIS methodology packs, compiled into the binary from `iris/packs/`
/// so a bare `cpg` binary carries the whole methodology (`iris/METHODOLOGY.md`).
/// Unlike `builtin_pack`, these are target-shaped: sink vocabularies tuned to
/// a codebase family, usually entry-driven (empty or placeholder sources —
/// pair them with `--rpc-sources`/`--thrift-sources`/`--entry`).
pub const IRIS_PACKS: &[(&str, &str)] = &[
    (
        "auth-discard",
        include_str!("../../iris/packs/auth-discard.json"),
    ),
    (
        "authz-overwrite",
        include_str!("../../iris/packs/authz-overwrite.json"),
    ),
    (
        "header-trust",
        include_str!("../../iris/packs/header-trust.json"),
    ),
    (
        "file-wrappers",
        include_str!("../../iris/packs/file-wrappers.json"),
    ),
    ("safe-exec", include_str!("../../iris/packs/safe-exec.json")),
    ("go-cql", include_str!("../../iris/packs/go-cql.json")),
    ("msvs", include_str!("../../iris/packs/msvs.json")),
    (
        "oob-outparam",
        include_str!("../../iris/packs/oob-outparam.json"),
    ),
    ("py", include_str!("../../iris/packs/py.json")),
    (
        "py-heartbeat",
        include_str!("../../iris/packs/py-heartbeat.json"),
    ),
    ("scala-api", include_str!("../../iris/packs/scala-api.json")),
    ("jvm-exec", include_str!("../../iris/packs/jvm-exec.json")),
    ("ssrf", include_str!("../../iris/packs/ssrf.json")),
    ("ts-cms", include_str!("../../iris/packs/ts-cms.json")),
    ("web", include_str!("../../iris/packs/web.json")),
    ("xsvc", include_str!("../../iris/packs/xsvc.json")),
    ("xsvc-recv", include_str!("../../iris/packs/xsvc-recv.json")),
];

/// Look up a compiled-in IRIS pack by name.
pub fn iris_pack(name: &str) -> Option<RulePack> {
    IRIS_PACKS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, json)| RulePack::from_json(json).expect("compiled-in IRIS pack must parse"))
}

impl Rule {
    /// Map the free-form severity onto the closed SARIF `level` vocabulary.
    pub fn sarif_level(&self) -> &'static str {
        match self.severity.to_ascii_lowercase().as_str() {
            "critical" | "high" | "error" => "error",
            "low" | "info" | "note" => "note",
            _ => "warning", // medium and anything unrecognised
        }
    }
}

/// The compiled-in default rule pack for a language, so `cpg scan` works on
/// any repository with zero configuration. Sources are the language's
/// standard attacker-input APIs, sinks its standard dangerous operations —
/// nothing project-specific belongs here (pass `--rules` for that).
///
/// Matching is by simple call name, so packs favour names that are
/// distinctive in practice (`CommandContext`, `executeQuery`) over generic
/// ones (`Get`, `run`) that would drown the report in noise.
pub fn builtin_pack(lang: &str) -> Option<RulePack> {
    let json = match lang {
        "go" => GO_RULES,
        "scala" => SCALA_RULES,
        "python" => PYTHON_RULES,
        "java" => JAVA_RULES,
        "javascript" | "js" | "typescript" | "ts" | "tsx" => JS_RULES,
        "c" => C_RULES,
        "cpp" | "c++" | "cxx" => CPP_RULES,
        _ => return None,
    };
    Some(RulePack::from_json(json).expect("builtin rule pack must parse"))
}

const GO_RULES: &str = r#"{"rules":[
  {"id":"GO-CMD-001","name":"input-to-exec","cwe":"CWE-78","severity":"high",
   "description":"request/env input reaches command execution",
   "sources":["FormValue","PostFormValue","Getenv","GetHeader","Param","Cookie","ReadAll","Decode","Unmarshal"],
   "sinks":["Command@0","CommandContext@1","StartProcess@0","<shellform>"],
   "sanitizers":["len","cap"]},
  {"id":"GO-SQL-002","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"request/env input reaches a SQL query builder",
   "sources":["FormValue","PostFormValue","Getenv","GetHeader","Param","Cookie","ReadAll","Decode","Unmarshal"],
   "sinks":["Query@0","QueryContext@1","QueryRow@0","QueryRowContext@1","Exec@0","ExecContext@1","Raw@0"],
   "sanitizers":["len","cap"]},
  {"id":"GO-PATH-003","name":"input-to-file","cwe":"CWE-22","severity":"medium",
   "description":"request/env input reaches a filesystem path operation",
   "sources":["FormValue","PostFormValue","GetHeader","Param","Cookie","Decode","Unmarshal"],
   "sinks":["Open","OpenFile","ReadFile","WriteFile","RemoveAll"],
   "sanitizers":["Base","Clean","len","cap"]},
  {"id":"GO-SSRF-004","name":"input-to-request","cwe":"CWE-918","severity":"medium",
   "description":"request input reaches an outbound HTTP request URL",
   "sources":["FormValue","PostFormValue","GetHeader","Param","Cookie","Decode","Unmarshal"],
   "sinks":["NewRequest@1","NewRequestWithContext@2","PostForm@0"],
   "sanitizers":["len","cap"]},
  {"id":"GO-SQLFMT-005","name":"sprintf-to-sql","cwe":"CWE-89","severity":"medium",
   "description":"format-built string reaches a SQL query (parameterise instead)",
   "sources":["Sprintf"],
   "sinks":["Query@0","QueryContext@1","QueryRow@0","QueryRowContext@1","Exec@0","ExecContext@1","Raw@0"],
   "sanitizers":["len","cap"]}
]}"#;

const SCALA_RULES: &str = r#"{"rules":[
  {"id":"SC-SQL-001","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"request input reaches SQL execution",
   "sources":["getQueryString","queryString","bodyText","asJson","asFormUrlEncoded","arg","header"],
   "sinks":["executeQuery","executeUpdate","sql","sqlu"]},
  {"id":"SC-CMD-002","name":"input-to-process","cwe":"CWE-78","severity":"high",
   "description":"request input reaches process execution",
   "sources":["getQueryString","queryString","bodyText","asJson","asFormUrlEncoded","arg","header"],
   "sinks":["Process","exec","lineStream"]},
  {"id":"SC-PATH-003","name":"input-to-file","cwe":"CWE-22","severity":"medium",
   "description":"request input reaches a filesystem path operation",
   "sources":["getQueryString","queryString","bodyText","asJson","asFormUrlEncoded","arg","header"],
   "sinks":["FileInputStream","FileOutputStream","fromFile"]}
]}"#;

const PYTHON_RULES: &str = r#"{"rules":[
  {"id":"PY-CMD-001","name":"input-to-shell","cwe":"CWE-78","severity":"high",
   "description":"user/env input reaches shell execution",
   "sources":["input","getenv","get_json","recv","read"],
   "sinks":["system","popen","call","check_output","check_call"],
   "sanitizers":["quote"]},
  {"id":"PY-EVAL-002","name":"input-to-eval","cwe":"CWE-95","severity":"critical",
   "description":"user/env input reaches eval/exec",
   "sources":["input","getenv","get_json","recv","read"],
   "sinks":["eval","exec","literal_eval"]},
  {"id":"PY-SQL-003","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"user/env input reaches SQL execution",
   "sources":["input","getenv","get_json","recv","read"],
   "sinks":["execute@0","executemany@0","executescript@0"]}
]}"#;

const JAVA_RULES: &str = r#"{"rules":[
  {"id":"JV-CMD-001","name":"input-to-exec","cwe":"CWE-78","severity":"high",
   "description":"servlet input reaches command execution",
   "sources":["getParameter","getHeader","getQueryString","getInputStream","getCookies","readLine","nextLine"],
   "sinks":["exec"]},
  {"id":"JV-SQL-002","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"servlet input reaches SQL execution",
   "sources":["getParameter","getHeader","getQueryString","getInputStream","getCookies","readLine","nextLine"],
   "sinks":["executeQuery","executeUpdate","addBatch"]},
  {"id":"JV-PATH-003","name":"input-to-file","cwe":"CWE-22","severity":"medium",
   "description":"servlet input reaches a filesystem path operation",
   "sources":["getParameter","getHeader","getQueryString","getCookies"],
   "sinks":["FileInputStream","FileOutputStream","FileReader","FileWriter"]},
  {"id":"JV-REDIR-004","name":"input-to-redirect","cwe":"CWE-601","severity":"medium",
   "description":"servlet input reaches an open redirect",
   "sources":["getParameter","getHeader","getQueryString"],
   "sinks":["sendRedirect"]}
]}"#;

const JS_RULES: &str = r#"{"rules":[
  {"id":"JS-CMD-001","name":"input-to-exec","cwe":"CWE-78","severity":"high",
   "description":"request input reaches command execution",
   "sources":["query","param","body","get"],
   "sinks":["exec","execSync","spawn","spawnSync"]},
  {"id":"JS-EVAL-002","name":"input-to-eval","cwe":"CWE-95","severity":"critical",
   "description":"request input reaches code evaluation",
   "sources":["query","param","body","get"],
   "sinks":["eval","Function"]},
  {"id":"JS-PATH-003","name":"input-to-file","cwe":"CWE-22","severity":"medium",
   "description":"request input reaches a filesystem path operation",
   "sources":["query","param","body","get"],
   "sinks":["readFile","readFileSync","writeFile","writeFileSync","createReadStream"]}
]}"#;

const CPP_RULES: &str = r#"{"rules":[
  {"id":"CPP-CMD-001","name":"input-to-exec","cwe":"CWE-78","severity":"high",
   "description":"external input reaches command execution",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["system@0","popen@0","execl","execlp","execv","execvp","execve","Subprocess@0","fork_exec"]},
  {"id":"CPP-FMT-002","name":"input-to-format","cwe":"CWE-134","severity":"high",
   "description":"external input reaches a format-string position",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["printf@0","fprintf@1","sprintf@1","snprintf@2","syslog@1","vsnprintf@2"]},
  {"id":"CPP-BUF-003","name":"input-to-unbounded-copy","cwe":"CWE-120","severity":"high",
   "description":"external input reaches an unbounded or size-controlled copy",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["strcpy@1","strcat@1","gets","memcpy@2","alloca@0","VirtualAlloc@1"]},
  {"id":"CPP-SQL-004","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"external input reaches SQL execution",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["sqlite3_exec@1","sqlite3_prepare@1","sqlite3_prepare_v2@1","mysql_query@1","PQexec@1"]},
  {"id":"CPP-PATH-005","name":"input-to-file","cwe":"CWE-22","severity":"medium",
   "description":"external input reaches a filesystem path operation",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["fopen@0","unlink@0","rmdir@0","mkdir@0","rename@0","chmod@0","chown@0"]},
  {"id":"CPP-LIB-006","name":"input-to-dlopen","cwe":"CWE-114","severity":"high",
   "description":"external input controls the path of a dynamically loaded library",
   "sources":["getenv","read@out1","pread@out1","recv@out1","recvfrom@out1","fread@out0","fgets","fgets@out0","ReadFile@out1","JetRetrieveColumn@out3"],
   "sinks":["::dlopen@0","LoadLibraryA@0","LoadLibraryW@0","LoadLibraryExA@0","LoadLibraryExW@0"]}
]}"#;

const C_RULES: &str = r#"{"rules":[
  {"id":"C-CMD-001","name":"input-to-system","cwe":"CWE-78","severity":"high",
   "description":"external input reaches shell execution",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["system@0","popen@0","execl@0","execlp@0","execv@0","execvp@0","execve@0"]},
  {"id":"C-BUF-002","name":"input-to-unbounded-copy","cwe":"CWE-120","severity":"high",
   "description":"external input reaches an unbounded string copy",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["strcpy@1","strcat@1","sprintf@1","vsprintf@1"]},
  {"id":"C-FMT-003","name":"input-to-format","cwe":"CWE-134","severity":"medium",
   "description":"external input reaches a format string position",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["printf@0","fprintf@1","sprintf@1","snprintf@2","syslog@1","vsnprintf@2"]},
  {"id":"C-API-004","kind":"forbidden-call","name":"unbounded-line-input","cwe":"CWE-242","severity":"critical",
   "description":"gets cannot limit input and is always unsafe",
   "sinks":["gets"]},
  {"id":"C-SQL-005","name":"input-to-sql","cwe":"CWE-89","severity":"high",
   "description":"external input reaches SQL execution text",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["sqlite3_exec@1","sqlite3_prepare@1","sqlite3_prepare_v2@1","mysql_query@1","PQexec@1"]},
  {"id":"C-PATH-006","name":"input-to-file","cwe":"CWE-22","severity":"high",
   "description":"external input reaches a filesystem path operation",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["fopen@0","open@0","unlink@0","remove@0","rmdir@0","mkdir@0","rename@0","rename@1","chmod@0","chown@0"]},
  {"id":"C-LIB-007","name":"input-to-dlopen","cwe":"CWE-114","severity":"high",
   "description":"external input controls a dynamically loaded library path",
   "sources":["getenv","gets","gets@out0","fgets","fgets@out0","scanf@out1","read@out1","recv@out1","fread@out0"],
   "sinks":["dlopen@0","LoadLibraryA@0","LoadLibraryW@0","LoadLibraryExA@0","LoadLibraryExW@0"]}
]}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_rule() {
        let pack = RulePack::from_json(
            r#"{"rules":[{"id":"CPG-001","sources":["getenv"],"sinks":["system"]}]}"#,
        )
        .unwrap();
        assert_eq!(pack.rules.len(), 1);
        assert_eq!(pack.rules[0].id, "CPG-001");
        assert_eq!(pack.rules[0].severity, "medium");
        assert_eq!(pack.rules[0].sarif_level(), "warning");
    }

    #[test]
    fn tolerates_unknown_keys_and_sanitizers() {
        let pack = RulePack::from_json(
            r#"{"version":99,"rules":[{"id":"X","name":"n","cwe":"CWE-78",
                 "severity":"high","sources":["a"],"sinks":["b"],
                 "sanitizers":["escape"],"futureKey":{"nested":true}}]}"#,
        )
        .unwrap();
        assert_eq!(pack.rules[0].sanitizers, vec!["escape"]);
        assert_eq!(pack.rules[0].sarif_level(), "error");
    }

    #[test]
    fn rejects_missing_id() {
        assert!(RulePack::from_json(r#"{"rules":[{"sources":["a"]}]}"#).is_err());
    }

    #[test]
    fn parses_pack_entry_globs() {
        let pack = RulePack::from_json(
            r#"{"entryGlobs":["Queries.*@*/resolvers/*","*Handler.handle*"],
                 "rules":[{"id":"X","sources":["a"],"sinks":["b"]}]}"#,
        )
        .unwrap();
        assert_eq!(
            pack.entry_globs,
            vec!["Queries.*@*/resolvers/*", "*Handler.handle*"]
        );
        // Absent field defaults to empty (older packs unaffected).
        let bare =
            RulePack::from_json(r#"{"rules":[{"id":"Y","sources":["a"],"sinks":["b"]}]}"#).unwrap();
        assert!(bare.entry_globs.is_empty());
    }

    #[test]
    fn parses_authorization_census_conventions() {
        let absent = RulePack::from_json(r#"{"rules":[]}"#).unwrap();
        assert!(absent.caller_context_markers.is_none());
        assert!(absent.framework_server_calls.is_none());
        let defaults = absent.authz_census_config();
        assert!(defaults
            .caller_context_markers
            .iter()
            .any(|m| m == "subject context"));
        assert!(defaults
            .caller_context_markers
            .iter()
            .any(|m| m == "caller claims"));
        assert!(defaults
            .framework_server_calls
            .iter()
            .any(|m| m == "NewGRPCServer"));

        let disabled = RulePack::from_json(
            r#"{"callerContextMarkers":[],"frameworkServerCalls":[],"rules":[]}"#,
        )
        .unwrap();
        assert_eq!(disabled.caller_context_markers, Some(vec![]));
        assert_eq!(disabled.framework_server_calls, Some(vec![]));
        let disabled_config = disabled.authz_census_config();
        assert!(disabled_config.caller_context_markers.is_empty());
        assert!(disabled_config.framework_server_calls.is_empty());

        let custom = RulePack::from_json(
            r#"{"caller_context_markers":["access tag"],
                 "framework_server_calls":["BuildControlPlaneServer"],"rules":[]}"#,
        )
        .unwrap();
        assert_eq!(
            custom.caller_context_markers,
            Some(vec!["access tag".into()])
        );
        assert_eq!(
            custom.framework_server_calls,
            Some(vec!["BuildControlPlaneServer".into()])
        );
    }

    #[test]
    fn iris_packs_parse_and_carry_sinks() {
        for (name, _) in IRIS_PACKS {
            let pack = iris_pack(name).unwrap_or_else(|| panic!("no IRIS pack {name}"));
            assert!(!pack.rules.is_empty(), "{name} pack is empty");
            for rule in &pack.rules {
                assert!(!rule.id.is_empty(), "{name}: rule without id");
                // IRIS packs are entry-driven: sources may be empty, sinks never.
                assert!(!rule.sinks.is_empty(), "{name}/{}: no sinks", rule.id);
            }
        }
        assert!(iris_pack("no-such-pack").is_none());
    }

    #[test]
    fn resolve_dispatches_iris_prefix_and_paths() {
        assert!(RulePack::resolve("iris:jvm-exec").is_ok());
        let err = RulePack::resolve("iris:bogus").unwrap_err();
        assert!(
            err.contains("jvm-exec"),
            "error lists available packs: {err}"
        );
        assert!(RulePack::resolve("/nonexistent/rules.json").is_err());
    }

    #[test]
    fn builtin_packs_parse_and_are_complete() {
        for lang in [
            "go",
            "scala",
            "python",
            "java",
            "javascript",
            "js",
            "typescript",
            "ts",
            "tsx",
            "c",
            "cpp",
        ] {
            let pack = builtin_pack(lang).unwrap_or_else(|| panic!("no builtin pack for {lang}"));
            assert!(!pack.rules.is_empty(), "{lang} pack is empty");
            for rule in &pack.rules {
                if rule.kind.is_empty() || rule.kind == "taint" {
                    assert!(!rule.sources.is_empty(), "{}: no sources", rule.id);
                }
                assert!(!rule.sinks.is_empty(), "{}: no sinks", rule.id);
                assert!(rule.cwe.is_some(), "{}: no CWE", rule.id);
            }
        }
        assert!(builtin_pack("cobol").is_none());
    }
}
