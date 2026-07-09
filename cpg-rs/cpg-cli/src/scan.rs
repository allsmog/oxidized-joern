//! The scan layer (Gap 5): run every rule of a pack through the existing
//! interprocedural taint query and collect findings per rule. Both the
//! `cpg scan` subcommand (SARIF output) and the server's `{"cmd":"scan"}`
//! request (JSON grouped by rule id) go through `run_pack`, so the two
//! surfaces can never drift.

use crate::rules::{Rule, RulePack};
use crate::sarif::{self, SarifLog};
use cpg_analysis::Finding;
use cpg_core::{Cpg, Query};
use cpg_incremental::Project;

/// The findings one rule produced.
pub struct RuleFindings<'a> {
    pub rule: &'a Rule,
    pub findings: Vec<Finding>,
}

/// Run every rule in the pack against the project (one taint query per rule,
/// all reading the same incrementally-maintained summary cache).
pub fn run_pack<'a>(project: &Project, pack: &'a RulePack) -> Vec<RuleFindings<'a>> {
    pack.rules
        .iter()
        .map(|rule| {
            let sources: Vec<&str> = rule.sources.iter().map(String::as_str).collect();
            let sinks: Vec<&str> = rule.sinks.iter().map(String::as_str).collect();
            let sanitizers: Vec<&str> = rule.sanitizers.iter().map(String::as_str).collect();
            RuleFindings {
                rule,
                findings: project.find_taint_with_sanitizers(&sources, &sinks, &sanitizers),
            }
        })
        .collect()
}

/// Resolve the source file of the method a finding names (findings carry the
/// method's full name, not a file). Linear over methods — fine at scan
/// granularity, where findings number in the tens.
pub fn file_of_method(cpg: &Cpg, method_full_name: &str) -> Option<String> {
    cpg.methods()
        .into_iter()
        .find(|&m| cpg.full_name_of(m) == Some(method_full_name))
        .and_then(|m| cpg.path_of(cpg.file_of(m)))
        .map(str::to_string)
}

/// One-call scan: run the pack and emit a SARIF 2.1.0 log. `fallback_uri`
/// (typically the scanned project path) is used for findings whose method
/// cannot be mapped back to a file.
pub fn scan_to_sarif(project: &Project, pack: &RulePack, fallback_uri: &str) -> SarifLog {
    let per_rule = run_pack(project, pack);
    sarif::build_log(
        pack,
        &per_rule,
        &|method| file_of_method(&project.cpg, method),
        fallback_uri,
    )
}
