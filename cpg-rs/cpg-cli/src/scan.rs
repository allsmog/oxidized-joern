//! The scan layer (Gap 5): run every rule of a pack through the existing
//! interprocedural taint query and collect findings per rule. Both the
//! `cpg scan` subcommand (SARIF output) and the server's `{"cmd":"scan"}`
//! request (JSON grouped by rule id) go through `run_pack`, so the two
//! surfaces can never drift.

use crate::rules::{Rule, RulePack};
use crate::sarif::{self, SarifLog};
use cpg_analysis::Finding;
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use cpg_incremental::Project;
use std::collections::HashSet;

/// The findings one rule produced.
pub struct RuleFindings<'a> {
    pub rule: &'a Rule,
    pub findings: Vec<Finding>,
}

fn ast_nodes(cpg: &Cpg, root: NodeId) -> Vec<NodeId> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<NodeId> = cpg.out_kind(root, EdgeKind::Ast).collect();
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        output.push(node);
        stack.extend(cpg.out_kind(node, EdgeKind::Ast));
    }
    output
}

fn identifier_names(cpg: &Cpg, root: NodeId) -> HashSet<String> {
    std::iter::once(root)
        .chain(ast_nodes(cpg, root))
        .filter(|&node| {
            matches!(
                cpg.kind_of(node),
                NodeKind::Identifier
                    | NodeKind::Local
                    | NodeKind::FieldIdentifier
                    | NodeKind::MethodParameterIn
            )
        })
        .filter_map(|node| cpg.name_of(node).map(str::to_string))
        .collect()
}

fn intersects(left: &HashSet<String>, right: &HashSet<String>) -> bool {
    left.iter().any(|name| right.contains(name))
}

fn is_assignment(cpg: &Cpg, node: NodeId) -> bool {
    cpg.kind_of(node) == NodeKind::Call
        && cpg
            .name_of(node)
            .is_some_and(|name| matches!(name, "=" | "<operator>.assignment" | "assignment"))
}

fn assigned_names(cpg: &Cpg, call: NodeId) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<NodeId> = cpg.in_kind(call, EdgeKind::Ast).collect();
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        if is_assignment(cpg, node) {
            return cpg
                .arguments_of(node)
                .first()
                .map(|&lhs| identifier_names(cpg, lhs))
                .unwrap_or_default();
        }
        if cpg.kind_of(node) != NodeKind::Method {
            stack.extend(cpg.in_kind(node, EdgeKind::Ast));
        }
    }
    HashSet::new()
}

fn is_capacity_call(name: &str) -> bool {
    let name = name.trim_start_matches("::").to_ascii_lowercase();
    name.contains("prepbuff")
        || name.contains("realloc")
        || name.starts_with("reserve")
        || name.starts_with("resize")
        || name.starts_with("expand")
        || name.starts_with("grow")
        || (name.contains("ensure") && name.contains("capacity"))
}

fn qualified_sink_index(rule: &Rule, sink: &str) -> Option<usize> {
    rule.sinks.iter().find_map(|spec| {
        let (name, index) = spec.rsplit_once('@')?;
        (name.trim_start_matches("::") == sink)
            .then(|| index.parse::<usize>().ok())
            .flatten()
    })
}

fn capacity_is_provisioned(cpg: &Cpg, rule: &Rule, finding: &Finding) -> bool {
    let (Some(sink_file), Some(sink_line), Some(size_index)) = (
        finding.sink_file.as_deref(),
        finding.sink_line,
        qualified_sink_index(rule, &finding.sink),
    ) else {
        return false;
    };
    let Some(method) = cpg
        .methods()
        .into_iter()
        .filter(|&method| cpg.path_of(cpg.file_of(method)) == Some(sink_file))
        .filter(|&method| cpg.line_of(method).is_some_and(|line| line <= sink_line))
        .max_by_key(|&method| cpg.line_of(method))
    else {
        return false;
    };
    let nodes = ast_nodes(cpg, method);
    let Some(sink) = nodes.iter().copied().find(|&node| {
        cpg.kind_of(node) == NodeKind::Call
            && cpg.name_of(node) == Some(finding.sink.as_str())
            && cpg.line_of(node) == Some(sink_line)
    }) else {
        return false;
    };
    let sink_arguments = cpg.arguments_of(sink);
    let (Some(&destination), Some(&size)) =
        (sink_arguments.first(), sink_arguments.get(size_index))
    else {
        return false;
    };
    let destination_names = identifier_names(cpg, destination);
    let size_names = identifier_names(cpg, size);
    if destination_names.is_empty() || size_names.is_empty() {
        return false;
    }

    nodes.into_iter().any(|call| {
        if cpg.kind_of(call) != NodeKind::Call
            || cpg.line_of(call).is_none_or(|line| line >= sink_line)
            || !cpg.name_of(call).is_some_and(is_capacity_call)
        {
            return false;
        }
        let arguments = cpg.arguments_of(call);
        let argument_names: HashSet<String> = arguments
            .iter()
            .flat_map(|&argument| identifier_names(cpg, argument))
            .collect();
        if !intersects(&argument_names, &size_names) {
            return false;
        }
        let assigned = assigned_names(cpg, call);
        let receiver_or_destination = arguments
            .first()
            .map(|&argument| identifier_names(cpg, argument))
            .unwrap_or_default();
        intersects(&assigned, &destination_names)
            || intersects(&receiver_or_destination, &destination_names)
    })
}

/// Run every rule in the pack against the project (one taint query per rule,
/// all reading the same incrementally-maintained summary cache).
pub fn run_pack<'a>(project: &Project, pack: &'a RulePack) -> Vec<RuleFindings<'a>> {
    run_pack_entry(project, pack, &[], &[], &[])
}

/// Like [`run_pack`], plus the entry-point model: parameters of any method
/// named in `entry_methods` (typically the RPC names from a service's .proto
/// files) are treated as attacker-controlled for every rule.
pub fn run_pack_entry<'a>(
    project: &Project,
    pack: &'a RulePack,
    entry_methods: &[String],
    idl_entries: &[String],
    registered_entries: &[String],
) -> Vec<RuleFindings<'a>> {
    pack.rules
        .iter()
        .map(|rule| {
            let sources: Vec<&str> = rule.sources.iter().map(String::as_str).collect();
            let sinks: Vec<&str> = rule.sinks.iter().map(String::as_str).collect();
            let sanitizers: Vec<&str> = rule.sanitizers.iter().map(String::as_str).collect();
            // Structural kinds short-circuit: no taint query, the name lists
            // parameterise an AST census instead (see `cpg_analysis::structural`).
            match rule.kind.as_str() {
                "" | "taint" => {}
                "forbidden-call" => {
                    return RuleFindings {
                        rule,
                        findings: cpg_analysis::structural::forbidden_calls(&project.cpg, &sinks),
                    };
                }
                "unbounded-scanf" => {
                    return RuleFindings {
                        rule,
                        findings: cpg_analysis::structural::unbounded_scanf_calls(
                            &project.cpg,
                            &sinks,
                        ),
                    };
                }
                "discarded-return" => {
                    return RuleFindings {
                        rule,
                        findings: cpg_analysis::structural::discarded_returns(&project.cpg, &sinks),
                    };
                }
                "append-without-delete" => {
                    return RuleFindings {
                        rule,
                        findings: cpg_analysis::structural::append_without_delete(
                            &project.cpg,
                            &sinks,
                            &sanitizers,
                            &sources,
                        ),
                    };
                }
                other => {
                    eprintln!("rule {}: unknown kind '{other}', skipping", rule.id);
                    return RuleFindings {
                        rule,
                        findings: Vec::new(),
                    };
                }
            }
            // CLI-level entry methods plus the rule's own.
            let entries: Vec<&str> = entry_methods
                .iter()
                .map(String::as_str)
                .chain(rule.entry_methods.iter().map(String::as_str))
                .collect();
            let idents: Vec<&str> = rule.source_idents.iter().map(String::as_str).collect();
            let idl: Vec<&str> = idl_entries.iter().map(String::as_str).collect();
            let registered: Vec<&str> = registered_entries.iter().map(String::as_str).collect();
            let authz: Vec<&str> = rule.authz.iter().map(String::as_str).collect();
            let confiners: Vec<&str> = rule.confiners.iter().map(String::as_str).collect();
            let mut findings = project.find_taint_spec(
                &sources,
                &sinks,
                &sanitizers,
                &entries,
                &idl,
                &registered,
                &idents,
                &authz,
                &confiners,
            );
            if rule.capacity_provisioning_is_fix {
                findings.retain(|finding| !capacity_is_provisioned(&project.cpg, rule, finding));
            }
            RuleFindings { rule, findings }
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
    scan_to_sarif_entry(project, pack, fallback_uri, &[], &[], &[])
}

/// [`scan_to_sarif`] with entry-point methods (see [`run_pack_entry`]).
pub fn scan_to_sarif_entry(
    project: &Project,
    pack: &RulePack,
    fallback_uri: &str,
    entry_methods: &[String],
    idl_entries: &[String],
    registered_entries: &[String],
) -> SarifLog {
    let per_rule = run_pack_entry(
        project,
        pack,
        entry_methods,
        idl_entries,
        registered_entries,
    );
    sarif::build_log(
        pack,
        &per_rule,
        &|method| file_of_method(&project.cpg, method),
        fallback_uri,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_provisioning_suppression_is_explicit_and_grow_only() {
        let project = crate::build_project_from_sources(
            "c",
            &[(
                "copy.c".to_string(),
                concat!(
                    "void provisioned(void *state, char *src) {\n",
                    "  size_t size = getenv(\"N\");\n",
                    "  char *dst = prepbuffsize(state, size);\n",
                    "  memcpy(dst, src, size);\n",
                    "}\n",
                    "void checked_only(char *dst, char *src) {\n",
                    "  size_t size = getenv(\"N\");\n",
                    "  if (size < 8) { check(size); }\n",
                    "  memcpy(dst, src, size);\n",
                    "}\n",
                )
                .to_string(),
            )],
            None,
        )
        .expect("build C fixture");
        let pack = RulePack::from_json(
            r#"{"rules":[{"id":"COPY","sources":["getenv"],"sinks":["memcpy@2"],"capacityProvisioningIsFix":true}]}"#,
        )
        .expect("rule pack");
        let findings = run_pack(&project, &pack);
        assert_eq!(findings[0].findings.len(), 1, "{:#?}", findings[0].findings);
        assert!(findings[0].findings[0].method.contains("checked_only"));
        assert!(findings[0].findings[0]
            .guard
            .as_deref()
            .is_some_and(|guard| guard.starts_with("guarded@")));
    }
}
