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
}

/// One named taint rule. Every field except `id` has a sensible default so
/// packs stay terse; unknown keys are ignored (no `deny_unknown_fields`),
/// which lets the format grow without breaking older binaries.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// Stable rule identifier (becomes SARIF `ruleId`), e.g. "CPG-001".
    pub id: String,
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
}
