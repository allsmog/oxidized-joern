//! Complete canonical graphs plus separately observed include-reference components.
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, OrderProperty};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CASES: [&str; 6] = [
    "duplicate_supplied_macro_local",
    "inactive_body_include",
    "repeated_direct_include",
    "tiny_fixedtables_include",
    "tiny_fixedtables_inline",
    "unresolved_include_pair",
];

fn case_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/include-references/cases")
        .join(name)
}

fn build(name: &str) -> Cpg {
    let mut sources: Vec<_> = std::fs::read_dir(case_root(name).join("input"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(path).unwrap(),
            )
        })
        .collect();
    sources.sort();
    let borrowed: Vec<_> = sources
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&borrowed);
    project.cpg
}

#[test]
fn include_cases_retain_all_six_complete_canonical_graphs() {
    for name in CASES {
        let graph = build(name);
        let expected = std::fs::read_to_string(case_root(name).join("expected.txt")).unwrap();
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&graph),
            expected,
            "{name}"
        );
    }
}

fn properties(cpg: &Cpg, node: NodeId) -> Value {
    let mut values = Map::new();
    // Include every represented optional string column, so an unintended value
    // cannot silently replace an absent Joern property on either selected kind.
    for (name, value) in [
        ("NAME", cpg.name_of(node)),
        ("FULL_NAME", cpg.full_name_of(node)),
        ("CODE", cpg.code_of(node)),
        ("TYPE_FULL_NAME", cpg.type_full_name_of(node)),
        ("SIGNATURE", cpg.signature_of(node)),
        ("MODIFIER_TYPE", cpg.modifier_type_of(node)),
        ("IMPORTED_ENTITY", cpg.imported_entity_of(node)),
        ("IMPORTED_AS", cpg.imported_as_of(node)),
        ("DEPENDENCY_GROUP_ID", cpg.dependency_group_id_of(node)),
        ("VERSION", cpg.version_of(node)),
    ] {
        if let Some(value) = value {
            values.insert(name.into(), json!(value));
        }
    }
    if let Some(line) = cpg.line_of(node) {
        assert!(i32::try_from(line).is_ok(), "observed LINE_NUMBER domain");
        values.insert("LINE_NUMBER".into(), json!(line));
    }
    if let Some(column) = cpg.column_number_of(node) {
        values.insert("COLUMN_NUMBER".into(), json!(column));
    }
    match cpg.order_property_of(node) {
        OrderProperty::Present(order) => {
            values.insert("ORDER".into(), json!(order));
        }
        OrderProperty::Absent => {}
        OrderProperty::Unknown => panic!("include ORDER presence must be explicit"),
    }
    Value::Object(values)
}

fn observed_components(cpg: &Cpg) -> Value {
    let dependencies: BTreeSet<_> = cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Dependency)
        .collect();
    let mut seen_dependencies = BTreeSet::new();
    let mut pairs = Vec::new();
    for imported in cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Import)
    {
        let parents: Vec<_> = cpg.in_kind(imported, EdgeKind::Ast).collect();
        let targets: Vec<_> = cpg.out_kind(imported, EdgeKind::Imports).collect();
        assert_eq!(parents.len(), 1, "each observed import has one AST owner");
        assert_eq!(targets.len(), 1, "each observed import has one dependency");
        let owner = parents[0];
        let dependency = targets[0];
        assert_eq!(cpg.kind_of(owner), NodeKind::NamespaceBlock);
        assert_eq!(cpg.name_of(owner), Some("<global>"));
        assert_eq!(cpg.kind_of(dependency), NodeKind::Dependency);
        assert!(
            seen_dependencies.insert(dependency),
            "distinct occurrence dependency"
        );
        let endpoint = |node| {
            if node == owner {
                "owner"
            } else if node == imported {
                "import"
            } else if node == dependency {
                "dependency"
            } else {
                panic!("unexpected include-component endpoint: {node:?}");
            }
        };
        let mut incident = Vec::new();
        // Visit every stored out-edge once. No deduplication or expected-label
        // filtering: extra edges and repeated occurrences must fail this gate.
        for source in cpg.nodes() {
            for edge in cpg.out(source) {
                if source == imported
                    || source == dependency
                    || edge.other == imported
                    || edge.other == dependency
                {
                    let label = match edge.kind {
                        EdgeKind::Ast => "AST",
                        EdgeKind::Imports => "IMPORTS",
                        other => panic!("unexpected include incident edge {other:?}"),
                    };
                    incident.push(json!([endpoint(source), label, endpoint(edge.other)]));
                }
            }
        }
        incident.sort_by_key(Value::to_string);
        pairs.push(json!({
            "owner": {
                "NAME": cpg.name_of(owner).unwrap(),
                "FULL_NAME": cpg.full_name_of(owner).unwrap(),
                // This is an explicit physical-owner bridge, not a claim that
                // the generic file partition stores every FILENAME property.
                "FILENAME": cpg.path_of(cpg.file_of(owner)).unwrap(),
            },
            "import": properties(cpg, imported),
            "dependency": properties(cpg, dependency),
            "incidentEdges": incident,
        }));
    }
    assert_eq!(
        dependencies, seen_dependencies,
        "no orphan or collapsed dependencies"
    );
    pairs.sort_by_key(Value::to_string);
    Value::Array(pairs)
}

fn check_metadata(cpg: &Cpg, expected: &Value, name: &str) {
    assert_eq!(observed_components(cpg), expected["pairs"], "{name}");
    for row in expected["ownerGlobalTypeDeclOrders"].as_array().unwrap() {
        let full_name = row["fullName"].as_str().unwrap();
        let nodes: Vec<_> = cpg
            .nodes()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::TypeDecl
                    && cpg.name_of(node) == Some("<global>")
                    && cpg.full_name_of(node) == Some(full_name)
            })
            .collect();
        assert_eq!(nodes.len(), 1, "{name}: physical global type declaration");
        let node = nodes[0];
        assert_eq!(cpg.path_of(cpg.file_of(node)), row["file"].as_str());
        assert_eq!(
            i64::from(cpg.order_of(node)),
            row["denseOrder"].as_i64().unwrap()
        );
        // Legacy dense ORDER is checked without inferring stored presence.
    }
}

#[test]
fn complete_observed_include_components_survive_project_pipeline_and_save_load() {
    for name in CASES {
        let graph = build(name);
        let expected: Value = serde_json::from_slice(
            &std::fs::read(case_root(name).join("expected-include-metadata.json")).unwrap(),
        )
        .unwrap();
        check_metadata(&graph, &expected, name);
        let canonical = cpg_lang_c::import::canonical_dump(&graph);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("includes.cpg");
        graph.save(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..6], b"CPG2\x03\x00");
        let reopened = Cpg::load(path.to_str().unwrap()).unwrap();
        check_metadata(&reopened, &expected, &format!("{name}: reopened"));
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&reopened),
            canonical,
            "{name}: canonical storage"
        );
        assert_eq!(
            std::fs::read(path).unwrap(),
            bytes,
            "load must not rewrite its store"
        );
    }
}
