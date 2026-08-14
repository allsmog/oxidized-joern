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
const MAX_REPEAT_DEPTH: usize = 4096;
const MAX_FLOW_PATHS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeSelector {
    Kind(NodeKind),
    All,
    /// Relative traversal seed used inside `where`/`whereNot` lambdas.
    Input,
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
    StringRegex {
        property: Property,
        pattern: String,
        negated: bool,
    },
    NumberCompare {
        property: Property,
        comparison: NumberComparison,
        value: i64,
    },
    Kind(NodeKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberComparison {
    Equals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
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
    AstDescendants,
    AstAncestors,
    DescendantsOfKind(NodeKind),
    ContainingMethod,
    File,
    Caller,
    CallOut,
    ParentBlock,
    InCall,
    Dominates,
    DominatedBy,
    PostDominates,
    PostDominatedBy,
    Controls,
    ControlledBy,
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
    Where {
        input: Box<LogicalPlan>,
        predicate: Box<LogicalPlan>,
        negated: bool,
    },
    ReachableBy {
        sinks: Box<LogicalPlan>,
        sources: Box<LogicalPlan>,
    },
    ReachableByFlows {
        sinks: Box<LogicalPlan>,
        sources: Box<LogicalPlan>,
    },
    BooleanWhere {
        input: Box<LogicalPlan>,
        predicates: Vec<LogicalPlan>,
        require_all: bool,
    },
    Repeat {
        input: Box<LogicalPlan>,
        body: Box<LogicalPlan>,
        until: Option<Box<LogicalPlan>>,
        emit_all: bool,
        emit_when: Option<Box<LogicalPlan>>,
        times: Option<usize>,
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
    Paths(Vec<Vec<NodeId>>),
    Count(usize),
}

struct RepeatSpec {
    body: LogicalPlan,
    until: Option<LogicalPlan>,
    emit_all: bool,
    emit_when: Option<LogicalPlan>,
    times: Option<usize>,
}

impl QueryResult {
    pub fn len(&self) -> usize {
        match self {
            QueryResult::Nodes(v) => v.len(),
            QueryResult::Strings(v) => v.len(),
            QueryResult::Integers(v) => v.len(),
            QueryResult::Paths(v) => v.len(),
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
        compile_parts(&parts, false)
    }
}

fn compile_relative(input: &str) -> Result<LogicalPlan, QueryError> {
    let parts = split_steps(input.trim())?;
    if parts.first().map(String::as_str) != Some("_") {
        return Err(QueryError(
            "where/filter expressions must use the `_` traversal parameter".to_string(),
        ));
    }
    compile_parts(&parts, true)
}

fn compile_parts(parts: &[String], relative: bool) -> Result<LogicalPlan, QueryError> {
    let (selector_index, mut plan) = if relative {
        (1, LogicalPlan::Scan(NodeSelector::Input))
    } else if parts[0] == "cpg" {
        let selector = parts
            .get(1)
            .ok_or_else(|| QueryError("missing node-type step after `cpg`".to_string()))?;
        (2, parse_selector_plan(selector)?)
    } else {
        (1, parse_selector_plan(&parts[0])?)
    };

    let mut projected = false;
    for (offset, step) in parts[selector_index..].iter().enumerate() {
        let position = selector_index + offset + 1;
        if matches!(
            step.as_str(),
            "l" | "toList" | "toJson" | "toJsonPretty" | "p" | "browse" | "clone"
        ) {
            continue;
        }
        if step == "size" || step == "count" {
            plan = LogicalPlan::Count(Box::new(plan));
            projected = true;
            continue;
        }
        if let Some(n) = parse_usize_call(step, "limit")?.or(parse_usize_call(step, "take")?) {
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                count: n,
            };
            continue;
        }
        if matches!(step.as_str(), "head" | "headOption") {
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                count: 1,
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

        if step.starts_with("repeat") {
            let RepeatSpec {
                body,
                until,
                emit_all,
                emit_when,
                times,
            } = parse_repeat_step(step)?;
            plan = LogicalPlan::Repeat {
                input: Box::new(plan),
                body: Box::new(body),
                until: until.map(Box::new),
                emit_all,
                emit_when: emit_when.map(Box::new),
                times,
            };
            continue;
        }
        if let Some(argument) = call_argument(step, "reachableByFlows") {
            let sources = QueryCompiler::compile(argument.trim())?;
            plan = LogicalPlan::ReachableByFlows {
                sinks: Box::new(plan),
                sources: Box::new(sources),
            };
            continue;
        }
        if let Some(argument) = call_argument(step, "reachableBy") {
            let sources = QueryCompiler::compile(argument.trim())?;
            plan = LogicalPlan::ReachableBy {
                sinks: Box::new(plan),
                sources: Box::new(sources),
            };
            continue;
        }
        if let Some((predicates, require_all)) = parse_boolean_where(step)? {
            plan = LogicalPlan::BooleanWhere {
                input: Box::new(plan),
                predicates,
                require_all,
            };
            continue;
        }
        let where_expression = ["where", "filter"]
            .iter()
            .find_map(|name| call_argument(step, name));
        let where_not_expression = ["whereNot", "filterNot"]
            .iter()
            .find_map(|name| call_argument(step, name));
        if let Some(expression) = where_expression.or(where_not_expression) {
            plan = LogicalPlan::Where {
                input: Box::new(plan),
                predicate: Box::new(compile_relative(expression.trim())?),
                negated: where_not_expression.is_some(),
            };
            continue;
        }
        if let Some(kind) = parse_kind_filter(step) {
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: Predicate::Kind(kind),
            };
            continue;
        }
        if let Some((property, pattern, negated)) = parse_string_filter(step)? {
            validate_regex(&pattern)?;
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: Predicate::StringRegex {
                    property,
                    pattern,
                    negated,
                },
            };
            continue;
        }
        if let Some((property, comparison, value)) = parse_number_filter(step)? {
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: Predicate::NumberCompare {
                    property,
                    comparison,
                    value,
                },
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
                    predicate: Predicate::NumberCompare {
                        property: Property::ArgumentIndex,
                        comparison: NumberComparison::Equals,
                        value: index as i64,
                    },
                };
            }
            continue;
        }
        if let Some((traversal, pattern)) = parse_named_traversal(step)? {
            validate_regex(&pattern)?;
            plan = LogicalPlan::Traverse {
                input: Box::new(plan),
                traversal,
            };
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: Predicate::StringRegex {
                    property: Property::Name,
                    pattern,
                    negated: false,
                },
            };
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

fn parse_selector_plan(step: &str) -> Result<LogicalPlan, QueryError> {
    if step == "assignment" {
        return Ok(LogicalPlan::Filter {
            input: Box::new(LogicalPlan::Scan(NodeSelector::Kind(NodeKind::Call))),
            predicate: Predicate::StringRegex {
                property: Property::Name,
                pattern: "(?:<operator>\\.)?(?:assignment|assignmentPlus|assignmentMinus|assignmentMultiplication|assignmentDivision|assignmentModulo|assignmentAnd|assignmentOr|assignmentXor|assignmentShiftLeft|assignmentArithmeticShiftRight|assignmentLogicalShiftRight)|=".to_string(),
                negated: false,
            },
        });
    }
    if let Ok(selector) = parse_selector(step) {
        return Ok(LogicalPlan::Scan(selector));
    }
    let Some(open) = step.find('(') else {
        return Err(QueryError(format!("unsupported node-type step `{step}`")));
    };
    let name = &step[..open];
    let selector = parse_selector(name)?;
    let argument = call_argument(step, name)
        .ok_or_else(|| QueryError(format!("invalid node-type filter `{step}`")))?;
    let values = parse_string_arguments(argument)?;
    if values.is_empty() {
        return Err(QueryError(format!(
            "node-type filter `{name}` expects at least one quoted string"
        )));
    }
    let pattern = values.join("|");
    validate_regex(&pattern)?;
    Ok(LogicalPlan::Filter {
        input: Box::new(LogicalPlan::Scan(selector)),
        predicate: Predicate::StringRegex {
            property: Property::Name,
            pattern,
            negated: false,
        },
    })
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
        "return" | "returns" => NodeKind::Return,
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

fn parse_string_filter(step: &str) -> Result<Option<(Property, String, bool)>, QueryError> {
    for (name, property) in [
        ("name", Property::Name),
        ("fullName", Property::FullName),
        ("code", Property::Code),
        ("typeFullName", Property::TypeFullName),
        ("signature", Property::Signature),
        ("filename", Property::Filename),
        ("label", Property::Label),
    ] {
        for (suffix, exact, negated) in [
            ("Exact", true, false),
            ("Not", false, true),
            ("", false, false),
        ] {
            let method = format!("{name}{suffix}");
            if let Some(argument) = call_argument(step, &method) {
                let values = parse_string_arguments(argument)?;
                if values.is_empty() {
                    return Err(QueryError(format!(
                        "`{method}` expects at least one quoted string"
                    )));
                }
                let patterns: Vec<String> = values
                    .into_iter()
                    .map(|value| if exact { regex::escape(&value) } else { value })
                    .collect();
                return Ok(Some((property, patterns.join("|"), negated)));
            }
        }
    }
    Ok(None)
}

fn parse_number_filter(
    step: &str,
) -> Result<Option<(Property, NumberComparison, i64)>, QueryError> {
    for (name, property) in [
        ("lineNumber", Property::LineNumber),
        ("order", Property::Order),
        ("argumentIndex", Property::ArgumentIndex),
        ("id", Property::Id),
    ] {
        for (suffix, comparison) in [
            ("Gt", NumberComparison::GreaterThan),
            ("Gte", NumberComparison::GreaterThanOrEqual),
            ("Lt", NumberComparison::LessThan),
            ("Lte", NumberComparison::LessThanOrEqual),
            ("", NumberComparison::Equals),
        ] {
            let method = format!("{name}{suffix}");
            if let Some(argument) = call_argument(step, &method) {
                let value = argument
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| QueryError(format!("`{method}` expects one integer argument")))?;
                return Ok(Some((property, comparison, value)));
            }
        }
    }
    Ok(None)
}

fn parse_kind_filter(step: &str) -> Option<NodeKind> {
    let selector = step.strip_prefix("is")?;
    let mut chars = selector.chars();
    let first = chars.next()?.to_ascii_lowercase();
    let selector = format!("{first}{}", chars.as_str());
    match parse_selector(&selector).ok()? {
        NodeSelector::Kind(kind) => Some(kind),
        NodeSelector::All | NodeSelector::Input => None,
    }
}

fn call_argument<'a>(step: &'a str, name: &str) -> Option<&'a str> {
    step.strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn parse_repeat_step(step: &str) -> Result<RepeatSpec, QueryError> {
    let groups = call_groups(step, "repeat")?;
    if groups.len() != 2 {
        return Err(QueryError(
            "`repeat` expects a traversal and one options group".to_string(),
        ));
    }
    let body = compile_relative(groups[0].trim())?;
    let option_parts = split_steps(groups[1].trim())?;
    if option_parts.first().map(String::as_str) != Some("_") {
        return Err(QueryError(
            "repeat options must use the `_` traversal parameter".to_string(),
        ));
    }
    let mut until = None;
    let mut emit_all = false;
    let mut emit_when = None;
    let mut times = None;
    for option in &option_parts[1..] {
        if option == "emit" {
            emit_all = true;
        } else if let Some(expression) = call_argument(option, "emit") {
            emit_when = Some(compile_relative(expression.trim())?);
        } else if let Some(expression) = call_argument(option, "until") {
            until = Some(compile_relative(expression.trim())?);
        } else if let Some(count) =
            parse_usize_call(option, "times")?.or(parse_usize_call(option, "maxDepth")?)
        {
            if count > MAX_REPEAT_DEPTH {
                return Err(QueryError(format!(
                    "repeat depth {count} exceeds maximum {MAX_REPEAT_DEPTH}"
                )));
            }
            times = Some(count);
        } else {
            return Err(QueryError(format!("unsupported repeat option `{option}`")));
        }
    }
    if until.is_none() && times.is_none() && !emit_all && emit_when.is_none() {
        return Err(QueryError(
            "repeat requires `until`, `times`, or `emit`".to_string(),
        ));
    }
    Ok(RepeatSpec {
        body,
        until,
        emit_all,
        emit_when,
        times,
    })
}

fn parse_boolean_where(step: &str) -> Result<Option<(Vec<LogicalPlan>, bool)>, QueryError> {
    for (name, require_all) in [("and", true), ("or", false)] {
        let Some(arguments) = call_argument(step, name) else {
            continue;
        };
        let expressions = split_top_level_arguments(arguments)?;
        if expressions.len() < 2 {
            return Err(QueryError(format!(
                "`{name}` expects at least two traversal predicates"
            )));
        }
        let predicates = expressions
            .into_iter()
            .map(|expression| compile_relative(expression.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Some((predicates, require_all)));
    }
    Ok(None)
}

fn split_top_level_arguments(input: &str) -> Result<Vec<&str>, QueryError> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
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
                    .ok_or_else(|| QueryError("unmatched `)` in arguments".to_string()))?;
            }
            ',' if depth == 0 => {
                let argument = input[start..index].trim();
                if argument.is_empty() {
                    return Err(QueryError("empty argument".to_string()));
                }
                arguments.push(argument);
                start = index + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return Err(QueryError("unclosed argument expression".to_string()));
    }
    let final_argument = input[start..].trim();
    if final_argument.is_empty() {
        return Err(QueryError("empty argument".to_string()));
    }
    arguments.push(final_argument);
    Ok(arguments)
}

fn call_groups<'a>(step: &'a str, name: &str) -> Result<Vec<&'a str>, QueryError> {
    let mut rest = step
        .strip_prefix(name)
        .ok_or_else(|| QueryError(format!("expected `{name}`")))?;
    let mut groups = Vec::new();
    while !rest.is_empty() {
        if !rest.starts_with('(') {
            return Err(QueryError(format!("invalid `{name}` argument groups")));
        }
        let mut depth = 0_u32;
        let mut quote = None;
        let mut escaped = false;
        let mut end = None;
        for (index, ch) in rest.char_indices() {
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == delimiter {
                    quote = None;
                }
                continue;
            }
            match ch {
                '\'' | '"' => quote = Some(ch),
                '(' => depth += 1,
                ')' => {
                    depth = depth.checked_sub(1).ok_or_else(|| {
                        QueryError(format!("unmatched `)` in `{name}` arguments"))
                    })?;
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| QueryError(format!("unclosed `{name}` argument")))?;
        groups.push(&rest[1..end]);
        rest = &rest[end + 1..];
    }
    Ok(groups)
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

fn parse_string_arguments(arguments: &str) -> Result<Vec<String>, QueryError> {
    let mut values = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in arguments.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
        } else {
            match ch {
                '\'' | '"' => quote = Some(ch),
                ',' => {
                    values.push(parse_string_literal(arguments[start..index].trim())?);
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    if quote.is_some() {
        return Err(QueryError("unterminated string literal".to_string()));
    }
    if !arguments[start..].trim().is_empty() {
        values.push(parse_string_literal(arguments[start..].trim())?);
    }
    Ok(values)
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
        "ast" => Traversal::AstDescendants,
        "astChildren" => edge(Direction::Out, EdgeKind::Ast, None),
        "astParent" => edge(Direction::In, EdgeKind::Ast, None),
        "inAst" | "inAstMinusLeaf" => Traversal::AstAncestors,
        "block" => Traversal::DescendantsOfKind(NodeKind::Block),
        "call" => Traversal::DescendantsOfKind(NodeKind::Call),
        "controlStructure" => Traversal::DescendantsOfKind(NodeKind::ControlStructure),
        "fieldIdentifier" => Traversal::DescendantsOfKind(NodeKind::FieldIdentifier),
        "identifier" => Traversal::DescendantsOfKind(NodeKind::Identifier),
        "literal" => Traversal::DescendantsOfKind(NodeKind::Literal),
        "local" => Traversal::DescendantsOfKind(NodeKind::Local),
        "methodRef" => Traversal::DescendantsOfKind(NodeKind::MethodRef),
        "return" => Traversal::DescendantsOfKind(NodeKind::Return),
        "typeRef" => Traversal::DescendantsOfKind(NodeKind::TypeRef),
        "parameter" => edge(
            Direction::Out,
            EdgeKind::Ast,
            Some(NodeKind::MethodParameterIn),
        ),
        "parameterOut" => edge(
            Direction::Out,
            EdgeKind::Ast,
            Some(NodeKind::MethodParameterOut),
        ),
        "methodReturn" => edge(Direction::Out, EdgeKind::Ast, Some(NodeKind::MethodReturn)),
        "receiver" => edge(Direction::Out, EdgeKind::Receiver, None),
        "callee" => edge(Direction::Out, EdgeKind::Call, Some(NodeKind::Method)),
        "callIn" => edge(Direction::In, EdgeKind::Call, Some(NodeKind::Call)),
        "callOut" => Traversal::CallOut,
        "inCall" => Traversal::InCall,
        "cfgNext" => edge(Direction::Out, EdgeKind::Cfg, None),
        "cfgPrev" => edge(Direction::In, EdgeKind::Cfg, None),
        "dominates" => Traversal::Dominates,
        "dominatedBy" => Traversal::DominatedBy,
        "postDominates" => Traversal::PostDominates,
        "postDominatedBy" => Traversal::PostDominatedBy,
        "controls" => Traversal::Controls,
        "controlledBy" => Traversal::ControlledBy,
        "ddgOut" => edge(Direction::Out, EdgeKind::Ddg, None),
        "ddgIn" => edge(Direction::In, EdgeKind::Ddg, None),
        "reachingDefOut" => edge(Direction::Out, EdgeKind::ReachingDef, None),
        "reachingDefIn" => edge(Direction::In, EdgeKind::ReachingDef, None),
        "ref" | "refsTo" => edge(Direction::Out, EdgeKind::Ref, None),
        "referencingIdentifiers" => edge(Direction::In, EdgeKind::Ref, None),
        "referencedType" | "typ" => edge(Direction::Out, EdgeKind::EvalType, Some(NodeKind::Type)),
        "evalType" => edge(Direction::Out, EdgeKind::EvalType, None),
        "parameterLink" => edge(Direction::Out, EdgeKind::ParameterLink, None),
        "condition" => edge(Direction::Out, EdgeKind::Condition, None),
        "whenTrue" => edge(Direction::Out, EdgeKind::TrueBody, None),
        "whenFalse" => edge(Direction::Out, EdgeKind::FalseBody, None),
        "method" => Traversal::ContainingMethod,
        "file" => Traversal::File,
        "caller" => Traversal::Caller,
        "parentBlock" => Traversal::ParentBlock,
        _ => return None,
    })
}

fn parse_named_traversal(step: &str) -> Result<Option<(Traversal, String)>, QueryError> {
    let Some(open) = step.find('(') else {
        return Ok(None);
    };
    let name = &step[..open];
    let Some(traversal) = parse_traversal(name) else {
        return Ok(None);
    };
    let Some(arguments) = call_argument(step, name) else {
        return Err(QueryError(format!("invalid traversal filter `{step}`")));
    };
    let values = parse_string_arguments(arguments)?;
    if values.is_empty() {
        return Err(QueryError(format!(
            "traversal filter `{name}` expects at least one quoted string"
        )));
    }
    Ok(Some((traversal, values.join("|"))))
}

pub struct QueryExecutor<'a> {
    cpg: &'a Cpg,
}

impl<'a> QueryExecutor<'a> {
    pub fn new(cpg: &'a Cpg) -> Self {
        QueryExecutor { cpg }
    }

    pub fn execute(&self, plan: &LogicalPlan) -> Result<QueryResult, QueryError> {
        self.execute_seeded(plan, &[])
    }

    fn execute_seeded(
        &self,
        plan: &LogicalPlan,
        seed: &[NodeId],
    ) -> Result<QueryResult, QueryError> {
        match plan {
            LogicalPlan::Scan(NodeSelector::Kind(kind)) => {
                Ok(QueryResult::Nodes(self.cpg.nodes_of_kind(*kind)))
            }
            LogicalPlan::Scan(NodeSelector::All) => {
                Ok(QueryResult::Nodes(self.cpg.nodes().collect()))
            }
            LogicalPlan::Scan(NodeSelector::Input) => Ok(QueryResult::Nodes(seed.to_vec())),
            LogicalPlan::Traverse { input, traversal } => {
                let nodes = self.nodes(input, seed)?;
                Ok(QueryResult::Nodes(self.traverse(&nodes, *traversal)))
            }
            LogicalPlan::Filter { input, predicate } => {
                let nodes = self.nodes(input, seed)?;
                Ok(QueryResult::Nodes(self.filter(nodes, predicate)?))
            }
            LogicalPlan::Where {
                input,
                predicate,
                negated,
            } => {
                let nodes = self.nodes(input, seed)?;
                let mut out = Vec::new();
                for node in nodes {
                    let matched = !self.execute_seeded(predicate, &[node])?.is_empty();
                    if matched != *negated {
                        out.push(node);
                    }
                }
                Ok(QueryResult::Nodes(out))
            }
            LogicalPlan::ReachableBy { sinks, sources } => {
                let sinks = self.nodes(sinks, seed)?;
                let source_nodes = self.nodes(sources, seed)?;
                let source_set: HashSet<NodeId> = source_nodes.iter().copied().collect();
                let mut matched = HashSet::new();
                for sink in sinks {
                    matched.extend(self.reaching_sources(sink, &source_set));
                }
                Ok(QueryResult::Nodes(
                    source_nodes
                        .into_iter()
                        .filter(|source| matched.contains(source))
                        .collect(),
                ))
            }
            LogicalPlan::ReachableByFlows { sinks, sources } => {
                let sinks = self.nodes(sinks, seed)?;
                let sources = self.nodes(sources, seed)?;
                let source_set: HashSet<NodeId> = sources.into_iter().collect();
                let mut paths = Vec::new();
                for sink in sinks {
                    self.reaching_paths(sink, &source_set, &mut paths)?;
                }
                Ok(QueryResult::Paths(stable_dedup(paths)))
            }
            LogicalPlan::BooleanWhere {
                input,
                predicates,
                require_all,
            } => {
                let nodes = self.nodes(input, seed)?;
                let mut out = Vec::new();
                for node in nodes {
                    let mut matches = predicates.iter().map(|predicate| {
                        self.execute_seeded(predicate, &[node])
                            .map(|result| !result.is_empty())
                    });
                    let matched = if *require_all {
                        matches.try_fold(true, |state, value| value.map(|value| state && value))?
                    } else {
                        matches.try_fold(false, |state, value| value.map(|value| state || value))?
                    };
                    if matched {
                        out.push(node);
                    }
                }
                Ok(QueryResult::Nodes(out))
            }
            LogicalPlan::Repeat {
                input,
                body,
                until,
                emit_all,
                emit_when,
                times,
            } => {
                let starts = self.nodes(input, seed)?;
                Ok(QueryResult::Nodes(self.repeat(
                    starts,
                    body,
                    until.as_deref(),
                    *emit_all,
                    emit_when.as_deref(),
                    *times,
                )?))
            }
            LogicalPlan::Deduplicate(input) => {
                let value = self.execute_seeded(input, seed)?;
                Ok(deduplicate(value))
            }
            LogicalPlan::Limit { input, count } => {
                let value = self.execute_seeded(input, seed)?;
                Ok(limit(value, *count))
            }
            LogicalPlan::Project { input, property } => {
                let nodes = self.nodes(input, seed)?;
                Ok(self.project(&nodes, *property))
            }
            LogicalPlan::Count(input) => {
                Ok(QueryResult::Count(self.execute_seeded(input, seed)?.len()))
            }
        }
    }

    fn nodes(&self, plan: &LogicalPlan, seed: &[NodeId]) -> Result<Vec<NodeId>, QueryError> {
        match self.execute_seeded(plan, seed)? {
            QueryResult::Nodes(nodes) => Ok(nodes),
            _ => Err(QueryError(
                "node traversal cannot follow a scalar projection".to_string(),
            )),
        }
    }

    fn filter(&self, nodes: Vec<NodeId>, predicate: &Predicate) -> Result<Vec<NodeId>, QueryError> {
        match predicate {
            Predicate::StringRegex {
                property,
                pattern,
                negated,
            } => {
                let regex = anchored_regex(pattern)?;
                Ok(nodes
                    .into_iter()
                    .filter(|&node| {
                        self.string_property(node, *property)
                            .is_some_and(|value| regex.is_match(&value) != *negated)
                    })
                    .collect())
            }
            Predicate::NumberCompare {
                property,
                comparison,
                value,
            } => Ok(nodes
                .into_iter()
                .filter(|&node| {
                    self.number_property(node, *property)
                        .is_some_and(|actual| compare_number(actual, *comparison, *value))
                })
                .collect()),
            Predicate::Kind(kind) => Ok(nodes
                .into_iter()
                .filter(|&node| self.cpg.kind_of(node) == *kind)
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
                Traversal::AstDescendants => {
                    out.extend(self.ast_descendants(node, None));
                }
                Traversal::AstAncestors => {
                    let mut stack: Vec<NodeId> = self.cpg.in_kind(node, EdgeKind::Ast).collect();
                    let mut seen = HashSet::new();
                    while let Some(parent) = stack.pop() {
                        if !seen.insert(parent) {
                            continue;
                        }
                        out.push(parent);
                        // Joern's inAst traversal stops at the enclosing
                        // method. C's schema-faithful per-file `<global>`
                        // wrapper has an AST edge to each real method, but it
                        // is not an ancestor exposed by semantic CPGQL.
                        if self.cpg.kind_of(parent) != NodeKind::Method {
                            stack.extend(self.cpg.in_kind(parent, EdgeKind::Ast));
                        }
                    }
                }
                Traversal::DescendantsOfKind(kind) => {
                    out.extend(self.ast_descendants(node, Some(kind)));
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
                Traversal::CallOut => {
                    let calls = if self.cpg.kind_of(node) == NodeKind::Call {
                        vec![node]
                    } else {
                        self.ast_descendants(node, Some(NodeKind::Call))
                    };
                    for call in calls {
                        out.extend(
                            self.cpg
                                .out_kind(call, EdgeKind::Call)
                                .filter(|&target| self.cpg.kind_of(target) == NodeKind::Method),
                        );
                    }
                }
                Traversal::ParentBlock => {
                    if let Some(block) = self.nearest_ast_ancestor(node, NodeKind::Block) {
                        out.push(block);
                    }
                }
                Traversal::InCall => {
                    if let Some(call) = self.nearest_ast_ancestor(node, NodeKind::Call) {
                        out.push(call);
                    }
                }
                Traversal::Dominates => {
                    out.extend(self.transitive_edge(node, Direction::Out, EdgeKind::Dominate));
                }
                Traversal::DominatedBy => {
                    out.extend(self.transitive_edge(node, Direction::In, EdgeKind::Dominate));
                }
                Traversal::PostDominates => {
                    out.extend(self.transitive_edge(node, Direction::Out, EdgeKind::PostDominate));
                }
                Traversal::PostDominatedBy => {
                    out.extend(self.transitive_edge(node, Direction::In, EdgeKind::PostDominate));
                }
                Traversal::Controls => out.extend(self.controlled_nodes(node)),
                Traversal::ControlledBy => {
                    out.extend(
                        self.cpg
                            .nodes()
                            .filter(|&candidate| self.controlled_nodes(candidate).contains(&node)),
                    );
                }
            }
        }
        out
    }

    fn ast_descendants(&self, root: NodeId, target: Option<NodeKind>) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut stack: Vec<NodeId> = self.cpg.out_kind(root, EdgeKind::Ast).collect();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if target.is_none_or(|kind| self.cpg.kind_of(node) == kind) {
                out.push(node);
            }
            stack.extend(self.cpg.out_kind(node, EdgeKind::Ast));
        }
        out
    }

    fn nearest_ast_ancestor(&self, start: NodeId, target: NodeKind) -> Option<NodeId> {
        let mut frontier: Vec<NodeId> = self.cpg.in_kind(start, EdgeKind::Ast).collect();
        let mut seen = HashSet::new();
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for node in frontier {
                if !seen.insert(node) {
                    continue;
                }
                if self.cpg.kind_of(node) == target {
                    return Some(node);
                }
                next.extend(self.cpg.in_kind(node, EdgeKind::Ast));
            }
            frontier = next;
        }
        None
    }

    fn transitive_edge(&self, start: NodeId, direction: Direction, edge: EdgeKind) -> Vec<NodeId> {
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        let mut stack: Vec<NodeId> = match direction {
            Direction::Out => self.cpg.out_kind(start, edge).collect(),
            Direction::In => self.cpg.in_kind(start, edge).collect(),
        };
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            output.push(node);
            match direction {
                Direction::Out => stack.extend(self.cpg.out_kind(node, edge)),
                Direction::In => stack.extend(self.cpg.in_kind(node, edge)),
            }
        }
        output
    }

    fn controlled_nodes(&self, controller: NodeId) -> Vec<NodeId> {
        let immediate_postdominator = self.cpg.in_kind(controller, EdgeKind::PostDominate).next();
        let mut output = Vec::new();
        let mut emitted = HashSet::new();
        for successor in self.cpg.out_kind(controller, EdgeKind::Cfg) {
            let mut runner = Some(successor);
            let mut branch_seen = HashSet::new();
            while let Some(node) = runner {
                if Some(node) == immediate_postdominator || !branch_seen.insert(node) {
                    break;
                }
                if emitted.insert(node) {
                    output.push(node);
                }
                runner = self.cpg.in_kind(node, EdgeKind::PostDominate).next();
            }
        }
        output
    }

    fn reaching_sources(&self, sink: NodeId, sources: &HashSet<NodeId>) -> HashSet<NodeId> {
        let mut matched = HashSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![sink];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if sources.contains(&node) {
                matched.insert(node);
            }
            for predecessor in self
                .cpg
                .in_kind(node, EdgeKind::ReachingDef)
                .chain(self.cpg.in_kind(node, EdgeKind::Ddg))
            {
                if sources.contains(&predecessor) {
                    matched.insert(predecessor);
                }
                stack.push(predecessor);
            }
        }
        matched
    }

    fn reaching_paths(
        &self,
        sink: NodeId,
        sources: &HashSet<NodeId>,
        output: &mut Vec<Vec<NodeId>>,
    ) -> Result<(), QueryError> {
        let mut stack = vec![(sink, vec![sink], HashSet::from([sink]))];
        while let Some((node, reverse_path, seen)) = stack.pop() {
            if sources.contains(&node) {
                let mut path = reverse_path;
                path.reverse();
                output.push(path);
                if output.len() >= MAX_FLOW_PATHS {
                    return Err(QueryError(format!(
                        "reachableByFlows exceeded {MAX_FLOW_PATHS} paths"
                    )));
                }
                continue;
            }
            if reverse_path.len() >= MAX_REPEAT_DEPTH {
                return Err(QueryError(format!(
                    "reachableByFlows exceeded {MAX_REPEAT_DEPTH} nodes in one path"
                )));
            }
            let mut predecessors: Vec<NodeId> = self
                .cpg
                .in_kind(node, EdgeKind::ReachingDef)
                .chain(self.cpg.in_kind(node, EdgeKind::Ddg))
                .collect();
            predecessors.reverse();
            for predecessor in predecessors {
                if seen.contains(&predecessor) {
                    continue;
                }
                let mut next_path = reverse_path.clone();
                next_path.push(predecessor);
                let mut next_seen = seen.clone();
                next_seen.insert(predecessor);
                stack.push((predecessor, next_path, next_seen));
            }
        }
        Ok(())
    }

    fn repeat(
        &self,
        starts: Vec<NodeId>,
        body: &LogicalPlan,
        until: Option<&LogicalPlan>,
        emit_all: bool,
        emit_when: Option<&LogicalPlan>,
        times: Option<usize>,
    ) -> Result<Vec<NodeId>, QueryError> {
        if times == Some(0) {
            return Ok(if emit_all { Vec::new() } else { starts });
        }
        let mut emitted = Vec::new();
        let mut frontier = starts.clone();
        let mut seen: HashSet<NodeId> = starts.into_iter().collect();
        for depth in 1..=times.unwrap_or(MAX_REPEAT_DEPTH) {
            let next = match self.execute_seeded(body, &frontier)? {
                QueryResult::Nodes(nodes) => stable_dedup(nodes),
                _ => {
                    return Err(QueryError(
                        "repeat body must produce a node traversal".to_string(),
                    ));
                }
            };
            if next.is_empty() {
                return Ok(stable_dedup(emitted));
            }
            let at_limit = times == Some(depth);
            let mut continuing = Vec::new();
            for node in next {
                let terminal = if let Some(predicate) = until {
                    !self.execute_seeded(predicate, &[node])?.is_empty()
                } else {
                    false
                };
                let selected = if let Some(predicate) = emit_when {
                    !self.execute_seeded(predicate, &[node])?.is_empty()
                } else {
                    false
                };
                if emit_all
                    || selected
                    || terminal
                    || (at_limit && !emit_all && emit_when.is_none())
                {
                    emitted.push(node);
                }
                if !terminal && !at_limit && seen.insert(node) {
                    continuing.push(node);
                }
            }
            if continuing.is_empty() {
                return Ok(stable_dedup(emitted));
            }
            frontier = continuing;
        }
        Err(QueryError(format!(
            "repeat did not terminate within {MAX_REPEAT_DEPTH} traversals"
        )))
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

fn compare_number(actual: i64, comparison: NumberComparison, expected: i64) -> bool {
    match comparison {
        NumberComparison::Equals => actual == expected,
        NumberComparison::GreaterThan => actual > expected,
        NumberComparison::GreaterThanOrEqual => actual >= expected,
        NumberComparison::LessThan => actual < expected,
        NumberComparison::LessThanOrEqual => actual <= expected,
    }
}

fn deduplicate(value: QueryResult) -> QueryResult {
    match value {
        QueryResult::Nodes(values) => QueryResult::Nodes(stable_dedup(values)),
        QueryResult::Strings(values) => QueryResult::Strings(stable_dedup(values)),
        QueryResult::Integers(values) => QueryResult::Integers(stable_dedup(values)),
        QueryResult::Paths(values) => QueryResult::Paths(stable_dedup(values)),
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
        QueryResult::Paths(mut values) => {
            values.truncate(count);
            QueryResult::Paths(values)
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
        NodeKind::Binding => "BINDING",
        NodeKind::ClosureBinding => "CLOSURE_BINDING",
        NodeKind::Annotation => "ANNOTATION",
        NodeKind::AnnotationLiteral => "ANNOTATION_LITERAL",
        NodeKind::AnnotationParameter => "ANNOTATION_PARAMETER",
        NodeKind::AnnotationParameterAssign => "ANNOTATION_PARAMETER_ASSIGN",
        NodeKind::ArrayInitializer => "ARRAY_INITIALIZER",
        NodeKind::Comment => "COMMENT",
        NodeKind::ConfigFile => "CONFIG_FILE",
        NodeKind::Dependency => "DEPENDENCY",
        NodeKind::Finding => "FINDING",
        NodeKind::Import => "IMPORT",
        NodeKind::JumpLabel => "JUMP_LABEL",
        NodeKind::KeyValuePair => "KEY_VALUE_PAIR",
        NodeKind::Tag => "TAG",
        NodeKind::TagNodePair => "TAG_NODE_PAIR",
        NodeKind::TemplateDom => "TEMPLATE_DOM",
        NodeKind::TypeArgument => "TYPE_ARGUMENT",
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
        let source = b.call("getenv", "getenv(\"INPUT\")", Some(3));
        b.ast_child(method, source);
        b.cpg.add_edge(source, src, EdgeKind::ReachingDef);
        let callee = b.method("strcpy", "strcpy", "char*(char*,char*)", None);
        b.contains(file_node, callee);
        b.cpg.add_edge(call, callee, EdgeKind::Call);
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
        assert_eq!(
            run(r#"cpg.call("strcpy").code"#),
            QueryResult::Strings(vec!["strcpy(dst, src)".to_string()])
        );
        assert_eq!(
            run(r#"cpg.method("main").call("getenv").code"#),
            QueryResult::Strings(vec!["getenv(\"INPUT\")".to_string()])
        );
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
            QueryResult::Strings(vec!["query.c".to_string(), "query.c".to_string()])
        );
    }

    #[test]
    fn rejects_invalid_and_post_projection_traversals() {
        assert!(QueryCompiler::compile(r#"cpg.call.name("[")"#).is_err());
        assert!(QueryCompiler::compile("cpg.call.name.argument").is_err());
        assert!(QueryCompiler::compile("cpg.noSuchNode").is_err());
    }

    #[test]
    fn supports_exact_negative_kind_numeric_and_where_filters() {
        assert_eq!(
            run(r#"cpg.call.nameExact("strcpy", "getenv").name"#),
            QueryResult::Strings(vec!["strcpy".to_string(), "getenv".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.nameNot("str.*").name"#),
            QueryResult::Strings(vec!["getenv".to_string()])
        );
        assert_eq!(run("cpg.method.ast.isCall.size"), QueryResult::Count(2));
        assert_eq!(
            run(r#"cpg.method.where(_.call.name("strcpy")).name"#),
            QueryResult::Strings(vec!["main".to_string()])
        );
        assert_eq!(
            run(r#"cpg.method.whereNot(_.call.name("strcpy")).name"#),
            QueryResult::Strings(vec!["strcpy".to_string()])
        );
        assert_eq!(run("cpg.call.lineNumberGt(3).size"), QueryResult::Count(1));
    }

    #[test]
    fn supports_call_out_and_reachable_by() {
        assert_eq!(
            run(r#"cpg.method.name("main").callOut.fullName"#),
            QueryResult::Strings(vec!["strcpy".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.name("strcpy").argument(2).reachableBy(cpg.call.name("getenv")).code"#),
            QueryResult::Strings(vec!["getenv(\"INPUT\")".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.name("strcpy").argument(1).reachableBy(cpg.call.name("getenv")).size"#),
            QueryResult::Count(0)
        );
    }

    #[test]
    fn supports_bounded_repeat_with_until_and_emit() {
        assert_eq!(
            run(r#"cpg.call.name("strcpy").repeat(_.astParent)(_.until(_.isMethod)).name"#),
            QueryResult::Strings(vec!["main".to_string()])
        );
        assert_eq!(
            run(r#"cpg.method.name("main").repeat(_.astChildren)(_.emit.times(1)).size"#),
            QueryResult::Count(3)
        );
        assert!(QueryCompiler::compile("cpg.call.repeat(_.astParent)(_.times(999999))").is_err());
    }

    #[test]
    fn supports_flow_paths_boolean_filters_and_standard_aliases() {
        assert_eq!(
            run(r#"cpg.call.and(_.name("strcpy"), _.argument(2)).name"#),
            QueryResult::Strings(vec!["strcpy".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.or(_.name("missing"), _.name("getenv")).name"#),
            QueryResult::Strings(vec!["getenv".to_string()])
        );
        assert_eq!(
            run(r#"cpg.call.filterNot(_.name("strcpy")).name"#),
            QueryResult::Strings(vec!["getenv".to_string()])
        );
        assert_eq!(
            run(r#"cpg.identifier("src").inCall.name"#),
            QueryResult::Strings(vec!["strcpy".to_string()])
        );
        assert_eq!(run("cpg.assignment.size"), QueryResult::Count(0));
        assert_eq!(
            run(r#"cpg.call("strcpy").argument(2).reachableByFlows(cpg.call("getenv")).p"#),
            QueryResult::Paths(vec![vec![NodeId(6), NodeId(5)]])
        );
    }

    #[test]
    fn supports_repeat_max_depth_and_predicated_emit() {
        assert_eq!(
            run(r#"cpg.method("main").repeat(_.astChildren)(_.emit(_.isCall).maxDepth(2)).name"#),
            QueryResult::Strings(vec!["strcpy".to_string(), "getenv".to_string()])
        );
    }

    #[test]
    fn committed_compatibility_catalog_compiles() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../acceptance/cpgql/catalog.json"))
                .expect("catalog JSON");
        let tiers = catalog["tiers"].as_array().expect("catalog tiers");
        let mut implemented_cases = 0;
        for tier in tiers.iter().filter(|tier| tier["status"] == "implemented") {
            let cases = tier["cases"].as_array().expect("catalog cases");
            implemented_cases += cases.len();
            for case in cases {
                let id = case["id"].as_str().expect("case id");
                let query = case["query"].as_str().expect("case query");
                QueryCompiler::compile(query).unwrap_or_else(|e| panic!("{id}: {e}"));
            }
        }
        assert!(
            implemented_cases >= 70,
            "implemented catalog must not silently shrink"
        );
    }
}
