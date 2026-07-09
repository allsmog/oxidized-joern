//! Minimal hand-rolled SARIF 2.1.0 emitter (Gap 5). Only the subset of the
//! schema we produce is modelled; every struct serialises to valid SARIF per
//! <https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/schemas/sarif-schema-2.1.0.json>.
//!
//! Mapping decisions:
//! - each rule in the pack → one `tool.driver.rules[]` reportingDescriptor
//!   (id, name, shortDescription, defaultConfiguration.level, and
//!   `properties.cwe`/`properties.severity` carrying the pack metadata);
//! - each taint finding → one `results[]` entry with `ruleId`, `ruleIndex`,
//!   `level` (severity mapped: critical/high→error, medium→warning,
//!   low→note), a message naming origin/sink/method, one location at the
//!   sink line, and one codeFlow whose single threadFlow replays the
//!   finding's witness path (one threadFlowLocation per step, message =
//!   the step's code text);
//! - file URIs come from the CPG file of the finding's method when known,
//!   else the scanned project path is used as a fallback; absolute paths
//!   are emitted as `file://` URIs, relative paths as relative URIs.

use crate::rules::RulePack;
use crate::scan::RuleFindings;
use serde::Serialize;

pub const SARIF_SCHEMA: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/schemas/sarif-schema-2.1.0.json";
pub const SARIF_VERSION: &str = "2.1.0";

#[derive(Debug, Serialize)]
pub struct SarifLog {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub version: String,
    pub runs: Vec<Run>,
}

#[derive(Debug, Serialize)]
pub struct Run {
    pub tool: Tool,
    pub results: Vec<SarifResult>,
}

#[derive(Debug, Serialize)]
pub struct Tool {
    pub driver: Driver,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Driver {
    pub name: String,
    pub version: String,
    pub information_uri: String,
    pub rules: Vec<ReportingDescriptor>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportingDescriptor {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub short_description: Text,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_description: Option<Text>,
    pub default_configuration: ReportingConfiguration,
    pub properties: RuleProperties,
}

#[derive(Debug, Serialize)]
pub struct ReportingConfiguration {
    pub level: String,
}

#[derive(Debug, Serialize)]
pub struct RuleProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwe: Option<String>,
    pub severity: String,
}

#[derive(Debug, Serialize)]
pub struct Text {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SarifResult {
    pub rule_id: String,
    pub rule_index: usize,
    pub level: String,
    pub message: Text,
    pub locations: Vec<Location>,
    pub code_flows: Vec<CodeFlow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub physical_location: PhysicalLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Text>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalLocation {
    pub artifact_location: ArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactLocation {
    pub uri: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub start_line: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeFlow {
    pub thread_flows: Vec<ThreadFlow>,
}

#[derive(Debug, Serialize)]
pub struct ThreadFlow {
    pub locations: Vec<ThreadFlowLocation>,
}

#[derive(Debug, Serialize)]
pub struct ThreadFlowLocation {
    pub location: Location,
}

/// Turn a filesystem path into a SARIF artifactLocation URI. Absolute paths
/// become `file://` URIs; relative paths are emitted as-is (valid relative
/// URI references). Only spaces are escaped — good enough for source trees.
fn path_to_uri(path: &str) -> String {
    let escaped = path.replace(' ', "%20");
    if escaped.starts_with('/') {
        format!("file://{escaped}")
    } else {
        escaped
    }
}

fn location(uri: &str, line: Option<u32>, message: Option<String>) -> Location {
    Location {
        physical_location: PhysicalLocation {
            artifact_location: ArtifactLocation { uri: path_to_uri(uri) },
            // SARIF requires startLine >= 1; omit the region when unknown.
            region: line.filter(|&l| l >= 1).map(|l| Region { start_line: l }),
        },
        message: message.map(|text| Text { text }),
    }
}

/// Build a complete SARIF log from a rule pack and its per-rule findings.
/// `resolve_file` maps a finding's method full-name to a source file path
/// (from the CPG); `fallback_uri` is used when that fails (e.g. the scanned
/// project directory).
pub fn build_log(
    pack: &RulePack,
    per_rule: &[RuleFindings],
    resolve_file: &dyn Fn(&str) -> Option<String>,
    fallback_uri: &str,
) -> SarifLog {
    let rules = pack
        .rules
        .iter()
        .map(|r| ReportingDescriptor {
            id: r.id.clone(),
            name: (!r.name.is_empty()).then(|| r.name.clone()),
            short_description: Text {
                text: if r.description.is_empty() {
                    r.name.clone()
                } else {
                    r.description.clone()
                },
            },
            full_description: (!r.description.is_empty()).then(|| Text {
                text: r.description.clone(),
            }),
            default_configuration: ReportingConfiguration { level: r.sarif_level().to_string() },
            properties: RuleProperties { cwe: r.cwe.clone(), severity: r.severity.clone() },
        })
        .collect();

    let mut results = Vec::new();
    for (rule_index, rf) in per_rule.iter().enumerate() {
        for f in &rf.findings {
            let uri = resolve_file(&f.method).unwrap_or_else(|| fallback_uri.to_string());
            let steps: Vec<ThreadFlowLocation> = f
                .path
                .iter()
                .map(|s| ThreadFlowLocation {
                    location: location(&uri, s.line, Some(s.code.clone())),
                })
                .collect();
            let what = if rf.rule.name.is_empty() { rf.rule.id.as_str() } else { rf.rule.name.as_str() };
            results.push(SarifResult {
                rule_id: rf.rule.id.clone(),
                rule_index,
                level: rf.rule.sarif_level().to_string(),
                message: Text {
                    text: format!(
                        "{what}: tainted value from `{}` reaches sink `{}` in `{}`",
                        f.origin, f.sink, f.method
                    ),
                },
                locations: vec![location(&uri, f.sink_line, None)],
                code_flows: vec![CodeFlow { thread_flows: vec![ThreadFlow { locations: steps }] }],
            });
        }
    }

    SarifLog {
        schema: SARIF_SCHEMA.to_string(),
        version: SARIF_VERSION.to_string(),
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: "cpg".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    information_uri: "https://github.com/allsmog/oxidized-joern".to_string(),
                    rules,
                },
            },
            results,
        }],
    }
}

impl SarifLog {
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("SARIF structs always serialise")
    }
}
