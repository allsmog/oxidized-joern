//! Minimal logical query compiler.
//!
//! The existing server exposes hand-written JSON commands. This module is the
//! first CPGQL/querydb bridge: parse a small compatibility subset into logical
//! plans, then execute those plans over the current graph. Future work can lower
//! the same `LogicalPlan` into relation rules.

use cpg_core::{Cpg, NodeId, Query};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeSelector {
    Methods,
    Calls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Predicate {
    NameEquals(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogicalPlan {
    Scan(NodeSelector),
    Filter { input: Box<LogicalPlan>, predicate: Predicate },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError(pub String);

#[derive(Default)]
pub struct QueryCompiler;

impl QueryCompiler {
    pub fn compile(input: &str) -> Result<LogicalPlan, QueryError> {
        let q = input.trim();
        if q == "method" || q == "cpg.method" {
            return Ok(LogicalPlan::Scan(NodeSelector::Methods));
        }
        if q == "call" || q == "cpg.call" {
            return Ok(LogicalPlan::Scan(NodeSelector::Calls));
        }
        if let Some(name) = parse_name_filter(q, "cpg.method.name(") {
            return Ok(LogicalPlan::Filter {
                input: Box::new(LogicalPlan::Scan(NodeSelector::Methods)),
                predicate: Predicate::NameEquals(name),
            });
        }
        if let Some(name) = parse_name_filter(q, "cpg.call.name(") {
            return Ok(LogicalPlan::Filter {
                input: Box::new(LogicalPlan::Scan(NodeSelector::Calls)),
                predicate: Predicate::NameEquals(name),
            });
        }
        Err(QueryError(format!("unsupported query subset: {q}")))
    }
}

fn parse_name_filter(q: &str, prefix: &str) -> Option<String> {
    let rest = q.strip_prefix(prefix)?;
    let quoted = rest.strip_prefix('"')?;
    let end = quoted.find('"')?;
    let name = quoted[..end].to_string();
    let after = &quoted[end + 1..];
    if after == ")" { Some(name) } else { None }
}

pub struct QueryExecutor<'a> {
    cpg: &'a Cpg,
}

impl<'a> QueryExecutor<'a> {
    pub fn new(cpg: &'a Cpg) -> Self {
        QueryExecutor { cpg }
    }

    pub fn execute(&self, plan: &LogicalPlan) -> Vec<NodeId> {
        match plan {
            LogicalPlan::Scan(NodeSelector::Methods) => self.cpg.methods(),
            LogicalPlan::Scan(NodeSelector::Calls) => self.cpg.calls(),
            LogicalPlan::Filter { input, predicate } => self
                .execute(input)
                .into_iter()
                .filter(|&n| self.matches(n, predicate))
                .collect(),
        }
    }

    fn matches(&self, node: NodeId, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::NameEquals(want) => self.cpg.name_of(node) == Some(want.as_str()),
        }
    }
}
