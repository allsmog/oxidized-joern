//! CPGQL-compatible query compilation and execution.
//!
//! This is deliberately a native Rust implementation: queries compile to a
//! small logical plan and run directly over [`cpg_core::Cpg`].  It does not
//! shell out to Joern or embed Scala.  The supported surface is kept explicit
//! and is differential-tested before it is advertised as compatible.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use regex::Regex;
use std::collections::HashSet;

const MAX_QUERY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeSelector {
    Kind(NodeKind),
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Name,
    FullName,
    Code,
    TypeFullName,
    Signature,
    Filename,
    Label,
    LineNumber,
    Order,
    ArgumentIndex,
    Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Predicate {
    StringRegex { property: Property, pattern: String },
    NumberEquals { property: Property, value: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Traversal {
    Edge {
        direction: Direction,
        edge: EdgeKind,
        target: Option<NodeKind>,
    },
    ContainingMethod,
    File,
    Caller,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogicalPlan {
    Scan(NodeSelector),
    Traverse {
        input: Box<LogicalPlan>,
        traversal: Traversal,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Predicate,
    },
    Deduplicate(Box<LogicalPlan>),
    Limit {
        input: Box<LogicalPlan>,
        count: usize,
    },
    Project {
        input: Box<LogicalPlan>,
        property: Property,
    },
    Count(Box<LogicalPlan>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryResult {
    Nodes(Vec<NodeId>),
    Strings(Vec<String>),
    Integers(Vec<i64>),
    Count(usize),
}

impl QueryResult {
    pub fn len(&self) -> usize {
        match self {
            QueryResult::Nodes(v) => v.len(),
            QueryResult::Strings(v) => v.len(),
            QueryResult::Integers(v) => v.len(),
            QueryResult::Count(_) => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError(pub String);

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QueryError {}

#[derive(Default)]
pub struct QueryCompiler;

impl QueryCompiler {
    pub fn compile(input: &str) -> Result<LogicalPlan, QueryError> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(QueryError(format!(
                "query is {} bytes; maximum is {MAX_QUERY_BYTES}",
                input.len()
            )));
        }
        let parts = split_steps(input.trim())?;
        if parts.is_empty() {
            return Err(QueryError("query is empty".to_string()));
        }
        let (selector_index, selector) = if parts[0] == "cpg" {
            let selector = parts
                .get(1)
                .ok_or_else(|| QueryError("missing node-type step after `cpg`".to_string()))?;
            (2, parse_selector(selector)?)
        } else {
            (1, parse_selector(&parts[0])?)
        };

        let mut plan = LogicalPlan::Scan(selector);
        let mut projected = false;
        for (offset, step) in parts[selector_index..].iter().enumerate() {
            let position = selector_index + offset + 1;
            if matches!(step.as_str(), "l" | "toList") {
                continue;
            }
            if step == "size" || step == "count" {
                plan = LogicalPlan::Count(Box::new(plan));
                projected = true;
                continue;
            }
            if let Some(n) = parse_usize_call(step, "limit")? {
                plan = LogicalPlan::Limit {
                    input: Box::new(plan),
                    count: n,
                };
                continue;
            }
            if matches!(step.as_str(), "dedup" | "distinct") {
                plan = LogicalPlan::Deduplicate(Box::new(plan));
                continue;
            }
            if projected {
                return Err(QueryError(format!(
                    "step {position} `{step}` cannot follow a scalar projection"
                )));
            }

            if let Some((property, pattern)) = parse_string_filter(step)? {
                validate_regex(&pattern)?;
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: Predicate::StringRegex { property, pattern },
                };
                continue;
            }
            if let Some((property, value)) = parse_number_filter(step)? {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: Predicate::NumberEquals { property, value },
                };
                continue;
            }
            if let Some(index) = parse_optional_usize_call(step, "argument")? {
                plan = LogicalPlan::Traverse {
                    input: Box::new(plan),
                    traversal: Traversal::Edge {
                        direction: Direction::Out,
                        edge: EdgeKind::Argument,
                        target: None,
                    },
                };
                if let Some(index) = index {
                    plan = LogicalPlan::Filter {
                        input: Box::new(plan),
                        predicate: Predicate::NumberEquals {
                            property: Property::ArgumentIndex,
                            value: index as i64,
                        },
                    };
                }
                continue;
            }
            if let Some(traversal) = parse_traversal(step) {
                plan = LogicalPlan::Traverse {
                    input: Box::new(plan),
                    traversal,
                };
                continue;
            }
            if let Some(property) = parse_property(step) {
                plan = LogicalPlan::Project {
                    input: Box::new(plan),
                    property,
                };
                projected = true;
                continue;
            }
            return Err(QueryError(format!("unsupported step {position} `{step}`")));
        }
        Ok(plan)
    }
}

fn split_steps(input: &str) -> Result<Vec<String>, QueryError> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (i, ch) in input.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| QueryError("unmatched `)`".to_string()))?;
            }
            '.' if depth == 0 => {
                let part = input[start..i].trim();
                if part.is_empty() {
                    return Err(QueryError("empty query step".to_string()));
                }
                parts.push(part.to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return Err(QueryError("unterminated string literal".to_string()));
    }
    if depth != 0 {
        return Err(QueryError("unclosed `(`".to_string()));
    }
    let last = input[start..].trim();
    if !last.is_empty() {
        parts.push(last.to_string());
    }
    Ok(parts)
}

fn parse_selector(step: &str) -> Result<NodeSelector, QueryError> {
    let kind = match step {
        "all" => return Ok(NodeSelector::All),
        "file" => NodeKind::File,
        "namespace" => NodeKind::Namespace,
        "namespaceBlock" => NodeKind::NamespaceBlock,
        "typeDecl" => NodeKind::TypeDecl,
        "type" => NodeKind::Type,
        "typeRef" => NodeKind::TypeRef,
        "member" => NodeKind::Member,
        "method" => NodeKind::Method,
        "parameter" | "methodParameterIn" => NodeKind::MethodParameterIn,
        "methodParameterOut" => NodeKind::MethodParameterOut,
        "methodReturn" => NodeKind::MethodReturn,
        "block" => NodeKind::Block,
        "call" => NodeKind::Call,
        "identifier" => NodeKind::Identifier,
        "literal" => NodeKind::Literal,
        "local" => NodeKind::Local,
        "fieldIdentifier" => NodeKind::FieldIdentifier,
        "controlStructure" => NodeKind::ControlStructure,
        "return" => NodeKind::Return,
        "methodRef" => NodeKind::MethodRef,
        "jumpTarget" => NodeKind::JumpTarget,
        "modifier" => NodeKind::Modifier,
        "metaData" => NodeKind::MetaData,
        "unknown" => NodeKind::Unknown,
        _ => return Err(QueryError(format!("unsupported node-type step `{step}`"))),
    };
    Ok(NodeSelector::Kind(kind))
}

fn parse_property(step: &str) -> Option<Property> {
    Some(match step {
        "name" => Property::Name,
        "fullName" => Property::FullName,
        "code" => Property::Code,
        "typeFullName" => Property::TypeFullName,
        "signature" => Property::Signature,
        "filename" => Property::Filename,
        "label" => Property::Label,
        "lineNumber" => Property::LineNumber,
        "order" => Property::Order,
        "argumentIndex" => Property::ArgumentIndex,
        "id" => Property::Id,
        _ => return None,
    })
}

fn parse_string_filter(step: &str) -> Result<Option<(Property, String)>, QueryError> {
    for (name, property) in [
        ("name", Property::Name),
        ("fullName", Property::FullName),
        ("code", Property::Code),
        ("typeFullName", Property::TypeFullName),
        ("signature", Property::Signature),
        ("filename", Property::Filename),
        ("label", Property::Label),
    ] {
        if let Some(argument) = call_argument(step, name) {
            return Ok(Some((property, parse_string_literal(argument)?)));
        }
    }
    Ok(None)
}

fn parse_number_filter(step: &str) -> Result<Option<(Property, i64)>, QueryError> {
    for (name, property) in [
        ("lineNumber", Property::LineNumber),
        ("order", Property::Order),
        ("argumentIndex", Property::ArgumentIndex),
        ("id", Property::Id),
    ] {
        if let Some(argument) = call_argument(step, name) {
            let value = argument
                .trim()
                .parse::<i64>()
                .map_err(|_| QueryError(format!("`{name}` expects one integer argument")))?;
            return Ok(Some((property, value)));
        }
    }
    Ok(None)
}

fn call_argument<'a>(step: &'a str, name: &str) -> Option<&'a str> {
    step.strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn parse_string_literal(value: &str) -> Result<String, QueryError> {
    let value = value.trim();
    if value.starts_with('"') {
        return serde_json::from_str(value)
            .map_err(|e| QueryError(format!("invalid string literal: {e}")));
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Ok(inner.replace("\\'", "'").replace("\\\\", "\\"));
    }
    Err(QueryError(
        "property filters require a quoted string".to_string(),
    ))
}

fn validate_regex(pattern: &str) -> Result<(), QueryError> {
    anchored_regex(pattern).map(|_| ())
}

fn anchored_regex(pattern: &str) -> Result<Regex, QueryError> {
    Regex::new(&format!("^(?:{pattern})$")).map_err(|e| QueryError(format!("invalid regex: {e}")))
}

fn parse_usize_call(step: &str, name: &str) -> Result<Option<usize>, QueryError> {
    let Some(argument) = call_argument(step, name) else {
        return Ok(None);
    };
    argument
        .trim()
        .parse::<usize>()
        .map(Some)
        .map_err(|_| QueryError(format!("`{name}` expects one non-negative integer")))
}

fn parse_optional_usize_call(step: &str, name: &str) -> Result<Option<Option<usize>>, QueryError> {
    if step == name {
        return Ok(Some(None));
    }
    parse_usize_call(step, name).map(|v| v.map(Some))
}

fn parse_traversal(step: &str) -> Option<Traversal> {
    let edge = |direction, edge, target| Traversal::Edge {
        direction,
        edge,
        target,
    };
    Some(match step {
        "ast" | "astChildren" => edge(Direction::Out, EdgeKind::Ast, None),
        "astParent" => edge(Direction::In, EdgeKind::Ast, None),
        "parameter" => edge(
            Direction::Out,
            EdgeKind::Ast,
            Some(NodeKind::MethodParameterIn),
        ),
        "methodReturn" => edge(Direction::Out, EdgeKind::Ast, Some(NodeKind::MethodReturn)),
        "receiver" => edge(Direction::Out, EdgeKind::Receiver, None),
        "callee" => edge(Direction::Out, EdgeKind::Call, Some(NodeKind::Method)),
        "callIn" => edge(Direction::In, EdgeKind::Call, Some(NodeKind::Call)),
        "cfgNext" => edge(Direction::Out, EdgeKind::Cfg, None),
        "cfgPrev" => edge(Direction::In, EdgeKind::Cfg, None),
        "ddgOut" => edge(Direction::Out, EdgeKind::Ddg, None),
        "ddgIn" => edge(Direction::In, EdgeKind::Ddg, None),
        "reachingDefOut" => edge(Direction::Out, EdgeKind::ReachingDef, None),
        "reachingDefIn" => edge(Direction::In, EdgeKind::ReachingDef, None),
        "ref" => edge(Direction::Out, EdgeKind::Ref, None),
        "referencingIdentifiers" => edge(Direction::In, EdgeKind::Ref, None),
        "referencedType" => edge(Direction::Out, EdgeKind::EvalType, Some(NodeKind::Type)),
        "method" => Traversal::ContainingMethod,
        "file" => Traversal::File,
        "caller" => Traversal::Caller,
        _ => return None,
    })
}

pub struct QueryExecutor<'a> {
    cpg: &'a Cpg,
}

impl<'a> QueryExecutor<'a> {
    pub fn new(cpg: &'a Cpg) -> Self {
        QueryExecutor { cpg }
    }

    pub fn execute(&self, plan: &LogicalPlan) -> Result<QueryResult, QueryError> {
        match plan {
            LogicalPlan::Scan(NodeSelector::Kind(kind)) => {
                Ok(QueryResult::Nodes(self.cpg.nodes_of_kind(*kind)))
            }
            LogicalPlan::Scan(NodeSelector::All) => {
                Ok(QueryResult::Nodes(self.cpg.nodes().collect()))
            }
            LogicalPlan::Traverse { input, traversal } => {
                let nodes = self.nodes(input)?;
                Ok(QueryResult::Nodes(self.traverse(&nodes, *traversal)))
            }
            LogicalPlan::Filter { input, predicate } => {
                let nodes = self.nodes(input)?;
                Ok(QueryResult::Nodes(self.filter(nodes, predicate)?))
            }
            LogicalPlan::Deduplicate(input) => {
                let value = self.execute(input)?;
                Ok(deduplicate(value))
            }
            LogicalPlan::Limit { input, count } => {
                let value = self.execute(input)?;
                Ok(limit(value, *count))
            }
            LogicalPlan::Project { input, property } => {
                let nodes = self.nodes(input)?;
                Ok(self.project(&nodes, *property))
            }
            LogicalPlan::Count(input) => Ok(QueryResult::Count(self.execute(input)?.len())),
        }
    }

    fn nodes(&self, plan: &LogicalPlan) -> Result<Vec<NodeId>, QueryError> {
        match self.execute(plan)? {
            QueryResult::Nodes(nodes) => Ok(nodes),
            _ => Err(QueryError(
                "node traversal cannot follow a scalar projection".to_string(),
            )),
        }
    }

    fn filter(&self, nodes: Vec<NodeId>, predicate: &Predicate) -> Result<Vec<NodeId>, QueryError> {
        match predicate {
            Predicate::StringRegex { property, pattern } => {
                let regex = anchored_regex(pattern)?;
                Ok(nodes
                    .into_iter()
                    .filter(|&node| {
                        self.string_property(node, *property)
                            .is_some_and(|value| regex.is_match(&value))
                    })
                    .collect())
            }
            Predicate::NumberEquals { property, value } => Ok(nodes
                .into_iter()
                .filter(|&node| self.number_property(node, *property) == Some(*value))
                .collect()),
        }
    }

    fn traverse(&self, nodes: &[NodeId], traversal: Traversal) -> Vec<NodeId> {
        let mut out = Vec::new();
        for &node in nodes {
            match traversal {
                Traversal::Edge {
                    direction,
                    edge,
                    target,
                } => {
                    let neighbours: Box<dyn Iterator<Item = NodeId>> = match direction {
                        Direction::Out => Box::new(self.cpg.out_kind(node, edge)),
                        Direction::In => Box::new(self.cpg.in_kind(node, edge)),
                    };
                    out.extend(
                        neighbours
                            .filter(|&n| target.is_none_or(|kind| self.cpg.kind_of(n) == kind)),
                    );
                }
                Traversal::ContainingMethod => {
                    if let Some(method) = self.containing_method(node) {
                        out.push(method);
                    }
                }
                Traversal::File => {
                    let file_id = self.cpg.file_of(node);
                    if let Some(file) = self
                        .cpg
                        .nodes_in_file(file_id)
                        .iter()
                        .copied()
                        .find(|&n| self.cpg.kind_of(n) == NodeKind::File)
                    {
                        out.push(file);
                    }
                }
                Traversal::Caller => {
                    for call in self.cpg.in_kind(node, EdgeKind::Call) {
                        if let Some(method) = self.containing_method(call) {
                            out.push(method);
                        }
                    }
                }
            }
        }
        out
    }

    fn containing_method(&self, start: NodeId) -> Option<NodeId> {
        if self.cpg.kind_of(start) == NodeKind::Method {
            return Some(start);
        }
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            for parent in self
                .cpg
                .in_kind(node, EdgeKind::Ast)
                .chain(self.cpg.in_kind(node, EdgeKind::Contains))
            {
                if self.cpg.kind_of(parent) == NodeKind::Method {
                    return Some(parent);
                }
                stack.push(parent);
            }
        }
        None
    }

    fn project(&self, nodes: &[NodeId], property: Property) -> QueryResult {
        if matches!(
            property,
            Property::LineNumber | Property::Order | Property::ArgumentIndex | Property::Id
        ) {
            QueryResult::Integers(
                nodes
                    .iter()
                    .filter_map(|&node| self.number_property(node, property))
                    .collect(),
            )
        } else {
            QueryResult::Strings(
                nodes
                    .iter()
                    .filter_map(|&node| self.string_property(node, property))
                    .collect(),
            )
        }
    }

    fn string_property(&self, node: NodeId, property: Property) -> Option<String> {
        Some(match property {
            Property::Name => self.cpg.name_of(node)?.to_string(),
            Property::FullName => self.cpg.full_name_of(node)?.to_string(),
            Property::Code => self.cpg.code_of(node)?.to_string(),
            Property::TypeFullName => self.cpg.type_full_name_of(node)?.to_string(),
            Property::Signature => self.cpg.signature_of(node)?.to_string(),
            Property::Filename => self.cpg.path_of(self.cpg.file_of(node))?.to_string(),
            Property::Label => node_kind_label(self.cpg.kind_of(node)).to_string(),
            _ => return None,
        })
    }

    fn number_property(&self, node: NodeId, property: Property) -> Option<i64> {
        Some(match property {
            Property::LineNumber => self.cpg.line_of(node)? as i64,
            Property::Order => self.cpg.order_of(node) as i64,
            Property::ArgumentIndex => self.cpg.argument_index_of(node) as i64,
            Property::Id => node.0 as i64,
            _ => return None,
        })
    }
}

fn deduplicate(value: QueryResult) -> QueryResult {
    match value {
        QueryResult::Nodes(values) => QueryResult::Nodes(stable_dedup(values)),
        QueryResult::Strings(values) => QueryResult::Strings(stable_dedup(values)),
        QueryResult::Integers(values) => QueryResult::Integers(stable_dedup(values)),
        count @ QueryResult::Count(_) => count,
    }
}

fn stable_dedup<T: Eq + std::hash::Hash + Clone>(values: Vec<T>) -> Vec<T> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn limit(value: QueryResult, count: usize) -> QueryResult {
    match value {
        QueryResult::Nodes(mut values) => {
            values.truncate(count);
            QueryResult::Nodes(values)
        }
        QueryResult::Strings(mut values) => {
            values.truncate(count);
            QueryResult::Strings(values)
        }
        QueryResult::Integers(mut values) => {
            values.truncate(count);
            QueryResult::Integers(values)
        }
        value @ QueryResult::Count(_) => value,
    }
}

pub fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "FILE",
        NodeKind::Namespace => "NAMESPACE",
        NodeKind::TypeDecl => "TYPE_DECL",
        NodeKind::Member => "MEMBER",
        NodeKind::Method => "METHOD",
        NodeKind::MethodParameterIn => "METHOD_PARAMETER_IN",
        NodeKind::MethodReturn => "METHOD_RETURN",
        NodeKind::Block => "BLOCK",
        NodeKind::Call => "CALL",
        NodeKind::Identifier => "IDENTIFIER",
        NodeKind::Literal => "LITERAL",
        NodeKind::Local => "LOCAL",
        NodeKind::FieldIdentifier => "FIELD_IDENTIFIER",
        NodeKind::ControlStructure => "CONTROL_STRUCTURE",
        NodeKind::Return => "RETURN",
        NodeKind::MethodRef => "METHOD_REF",
        NodeKind::Unknown => "UNKNOWN",
        NodeKind::MethodParameterOut => "METHOD_PARAMETER_OUT",
        NodeKind::TypeRef => "TYPE_REF",
        NodeKind::JumpTarget => "JUMP_TARGET",
        NodeKind::Modifier => "MODIFIER",
        NodeKind::NamespaceBlock => "NAMESPACE_BLOCK",
        NodeKind::Type => "TYPE",
        NodeKind::MetaData => "META_DATA",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::CpgBuilder;

    fn fixture() -> Cpg {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("query.c");
        let mut b = CpgBuilder::new(&mut cpg, file);
        let file_node = b.file_node("query.c");
        let method = b.method("main", "main", "int()", Some(1));
        b.contains(file_node, method);
        let parameter = b.parameter("argc", "int", 1);
        b.ast_child(method, parameter);
        let call = b.call("strcpy", "strcpy(dst, src)", Some(4));
        b.ast_child(method, call);
        let dst = b.identifier("dst", Some(4));
        let src = b.identifier("src", Some(4));
        b.add_argument(call, dst, 1);
        b.add_argument(call, src, 2);
        cpg
    }

    fn run(query: &str) -> QueryResult {
        let cpg = fixture();
        let plan = QueryCompiler::compile(query).expect("compile");
        QueryExecutor::new(&cpg).execute(&plan).expect("execute")
    }

    #[test]
    fn compiles_node_type_and_anchored_regex_filters() {
        assert_eq!(
            run(r#"cpg.call.name("str.*") .code("strcpy\\(.*")"#).len(),
            1
        );
        assert_eq!(run(r#"cpg.call.name("copy")"#).len(), 0);
    }

    #[test]
    fn traverses_arguments_and_projects_properties() {
        assert_eq!(
            run(r#"cpg.call.name("strcpy").argument(2).code.l"#),
            QueryResult::Strings(vec!["src".to_string()])
        );
        assert_eq!(
            run(r#"cpg.method.name("main").parameter.name.toList"#),
            QueryResult::Strings(vec!["argc".to_string()])
        );
    }

    #[test]
    fn supports_owner_file_dedup_and_count() {
        assert_eq!(
            run(r#"cpg.call.argument.method.name"#),
            QueryResult::Strings(vec!["main".to_string(), "main".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.argument.method.dedup.size"#),
            QueryResult::Count(1)
        );
        assert_eq!(
            run(r#"cpg.call.file.filename"#),
            QueryResult::Strings(vec!["query.c".to_string()])
        );
    }

    #[test]
    fn rejects_invalid_and_post_projection_traversals() {
        assert!(QueryCompiler::compile(r#"cpg.call.name("[")"#).is_err());
        assert!(QueryCompiler::compile("cpg.call.name.argument").is_err());
        assert!(QueryCompiler::compile("cpg.noSuchNode").is_err());
    }

    #[test]
    fn committed_compatibility_catalog_compiles() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../acceptance/cpgql/catalog.json"))
                .expect("catalog JSON");
        let cases = catalog["tiers"][0]["cases"]
            .as_array()
            .expect("catalog cases");
        assert!(cases.len() >= 25, "core tier must not silently shrink");
        for case in cases {
            let id = case["id"].as_str().expect("case id");
            let query = case["query"].as_str().expect("case query");
            QueryCompiler::compile(query).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
}
