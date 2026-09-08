//! Macro argument copies and source locations from complete pinned Joern output.
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};

struct Case {
    name: &'static str,
    header: &'static str,
    source: &'static str,
    graph: &'static str,
    locations: &'static str,
}

macro_rules! case {
    ($name:literal) => {
        Case {
            name: $name,
            header: include_str!(concat!(
                "fixtures/header-macro-locations/cases/",
                $name,
                "/defs.h"
            )),
            source: include_str!(concat!(
                "fixtures/header-macro-locations/cases/",
                $name,
                "/locations.c"
            )),
            graph: include_str!(concat!(
                "fixtures/header-macro-locations/cases/",
                $name,
                "/expected.txt"
            )),
            locations: include_str!(concat!(
                "fixtures/header-macro-locations/cases/",
                $name,
                "/locations.tsv"
            )),
        }
    };
}

const CASES: &[Case] = &[
    case!("assignment_after_macro"),
    case!("constant_before_literal"),
    case!("multiline_call_actual"),
    case!("multiline_value"),
    case!("quoted_literals"),
];

fn build(case: &Case) -> Cpg {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[("defs.h", case.header), ("locations.c", case.source)]);
    project.cpg
}

fn ordered_ast(cpg: &Cpg, node: NodeId, nodes: &mut Vec<NodeId>) {
    nodes.push(node);
    let mut children: Vec<_> = cpg.out_kind(node, EdgeKind::Ast).collect();
    children.sort_by_key(|&child| (cpg.order_of(child), child));
    for child in children {
        ordered_ast(cpg, child, nodes);
    }
}

#[test]
fn header_macro_location_fixtures_match_complete_live_graphs() {
    for case in CASES {
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&build(case)),
            case.graph,
            "{}",
            case.name,
        );
    }
}

#[test]
fn macro_descendants_and_following_statements_match_live_lines() {
    for case in CASES {
        let cpg = build(case);
        let method = cpg.method_named("probe")[0];
        let mut nodes = Vec::new();
        ordered_ast(&cpg, method, &mut nodes);
        let expected: Vec<_> = case
            .locations
            .lines()
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .filter(|fields| fields[0] == "probe")
            .collect();
        assert_eq!(nodes.len(), expected.len(), "{}: AST rows", case.name);
        for (index, (&node, fields)) in nodes.iter().zip(expected).enumerate() {
            assert_eq!(fields[1].parse::<usize>().unwrap(), index);
            let line = fields[7].parse::<u32>().unwrap();
            assert_eq!(
                cpg.line_of(node),
                Some(line),
                "{}: probe#{index} {:?} {:?}",
                case.name,
                cpg.kind_of(node),
                cpg.code_of(node),
            );
        }
        assert!(nodes
            .iter()
            .any(|&node| cpg.kind_of(node) == NodeKind::Call));
    }
}
