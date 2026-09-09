//! Complete canonical references and observed finalized macro binding components.
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, OrderProperty};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CASES: [&str; 4] = [
    "function_like_repeated_use",
    "local_object_used_unused",
    "repeated_direct_include",
    "shared_header_two_callers",
];

fn case_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/macro-bindings/cases")
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
fn macro_binding_cases_retain_all_four_complete_canonical_graphs() {
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

fn binding_properties(cpg: &Cpg, node: NodeId, is_macro: bool) -> Value {
    let mut properties = Map::new();
    // Every represented optional string column participates. Binding's shared
    // full_name storage denotes METHOD_FULL_NAME, never a second FULL_NAME.
    for (name, value) in [
        ("NAME", cpg.name_of(node)),
        ("METHOD_FULL_NAME", cpg.method_full_name_of(node)),
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
            properties.insert(name.into(), json!(value));
        }
    }
    if let Some(line) = cpg.line_of(node) {
        properties.insert("LINE_NUMBER".into(), json!(line));
    }
    if let Some(column) = cpg.column_number_of(node) {
        properties.insert("COLUMN_NUMBER".into(), json!(column));
    }
    if is_macro {
        assert_eq!(cpg.order_property_of(node), OrderProperty::Absent);
    } else {
        // Preserve the accepted ordinary binding storage contract. Unknown is
        // not an observation that Joern ORDER is absent.
        assert_eq!(cpg.order_property_of(node), OrderProperty::Unknown);
    }
    // Check dense storage separately without inventing Joern properties from
    // default values. ARGUMENT_INDEX presence is not represented by the core.
    assert_eq!(cpg.order_of(node), 0);
    assert_eq!(cpg.argument_index_of(node), -1);
    Value::Object(properties)
}

fn observed_components(cpg: &Cpg) -> Value {
    let macro_methods: BTreeSet<_> = cpg
        .nodes()
        .filter(|&node| {
            cpg.kind_of(node) == NodeKind::Method
                && cpg
                    .code_of(node)
                    .is_some_and(|code| code.starts_with("#define"))
        })
        .collect();
    let mut seen_macros = BTreeSet::new();
    let mut components = Vec::new();
    // Ordinary bindings remain in the comparison. Do not filter newly emitted
    // rows merely because the old canonical projection omits Binding nodes.
    for binding in cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Binding)
    {
        let owners: Vec<_> = cpg.in_kind(binding, EdgeKind::Binds).collect();
        let targets: Vec<_> = cpg.out_kind(binding, EdgeKind::Ref).collect();
        assert_eq!(owners.len(), 1);
        assert_eq!(targets.len(), 1);
        let owner = owners[0];
        let target = targets[0];
        assert_eq!(cpg.kind_of(owner), NodeKind::TypeDecl);
        assert_eq!(cpg.kind_of(target), NodeKind::Method);
        assert_eq!(cpg.file_of(binding), cpg.file_of(target));
        let is_macro = macro_methods.contains(&target);
        if is_macro {
            assert!(
                seen_macros.insert(target),
                "one binding per finalized macro stub"
            );
            assert_eq!(cpg.name_of(owner), Some("<global>"));
        }
        let endpoint = |node| {
            if node == owner {
                "owner"
            } else if node == binding {
                "binding"
            } else if node == target {
                "method"
            } else {
                panic!("unexpected Binding incident endpoint: {node:?}");
            }
        };
        let mut incident = Vec::new();
        // Visit every edge occurrence once without deduplication or filtering
        // to expected labels. Core edges have no property-payload storage; the
        // raw reference separately preserves their observed null payloads.
        for source in cpg.nodes() {
            for edge in cpg.out(source) {
                if source == binding || edge.other == binding {
                    let label = match edge.kind {
                        EdgeKind::Binds => "BINDS",
                        EdgeKind::Ref => "REF",
                        other => panic!("unexpected Binding incident edge: {other:?}"),
                    };
                    incident.push(json!([endpoint(source), label, endpoint(edge.other)]));
                }
            }
        }
        incident.sort_by_key(Value::to_string);
        components.push(json!({
            "macro": is_macro,
            "binding": binding_properties(cpg, binding, is_macro),
            "owner": {
                "NAME": cpg.name_of(owner).unwrap(),
                "FULL_NAME": cpg.full_name_of(owner).unwrap(),
                // An explicit physical-owner bridge, not a Binding FILENAME.
                "FILENAME": cpg.path_of(cpg.file_of(owner)).unwrap(),
            },
            "method": {
                "NAME": cpg.name_of(target).unwrap(),
                "FULL_NAME": cpg.full_name_of(target).unwrap(),
                "CODE": cpg.code_of(target).unwrap(),
                "SIGNATURE": cpg.signature_of(target).unwrap(),
                "FILENAME": cpg.path_of(cpg.file_of(target)).unwrap(),
            },
            "incomingCallCount": cpg.in_kind(target, EdgeKind::Call).count(),
            "incidentEdges": incident,
        }));
    }
    assert_eq!(
        macro_methods, seen_macros,
        "no macro stub lacks its binding"
    );
    components.sort_by_key(Value::to_string);
    Value::Array(components)
}

#[test]
fn complete_observed_macro_bindings_survive_pipeline_and_save_load() {
    for name in CASES {
        let graph = build(name);
        let expected: Value = serde_json::from_slice(
            &std::fs::read(case_root(name).join("expected-binding-metadata.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(observed_components(&graph), expected["bindings"], "{name}");
        let canonical = cpg_lang_c::import::canonical_dump(&graph);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("macro-bindings.cpg");
        graph.save(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..6], b"CPG2\x03\x00");
        let reopened = Cpg::load(path.to_str().unwrap()).unwrap();
        assert_eq!(
            observed_components(&reopened),
            expected["bindings"],
            "{name}: reopened"
        );
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&reopened),
            canonical,
            "{name}: canonical storage"
        );
        assert_eq!(
            reopened.to_bytes(),
            bytes,
            "{name}: complete storage round trip"
        );
        assert_eq!(
            std::fs::read(path).unwrap(),
            bytes,
            "load must not rewrite its store"
        );
    }
}
