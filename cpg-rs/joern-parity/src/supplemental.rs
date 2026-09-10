//! Lossless snapshot of the properties and edges represented by `Cpg`.
//!
//! This supplements the legacy canonical projection. It does not reconstruct
//! unstored Joern properties, default-property presence, or edge payloads.

use cpg_core::{Cpg, EdgeKind, Layer, NodeKind, OrderProperty};
use serde_json::{json, Map, Value};
use std::fmt::Write;

fn string(value: &str) -> Value {
    json!({"kind": "string", "value": value})
}

fn number(class: &str, value: impl ToString) -> Value {
    json!({"kind": "number", "class": class, "value": value.to_string()})
}

fn optional_string(properties: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        properties.insert(key.to_owned(), string(value));
    }
}

fn node_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "FILE",
        NodeKind::Namespace => "NAMESPACE",
        NodeKind::NamespaceBlock => "NAMESPACE_BLOCK",
        NodeKind::Type => "TYPE",
        NodeKind::TypeDecl => "TYPE_DECL",
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
        NodeKind::Member => "MEMBER",
        NodeKind::Method => "METHOD",
        NodeKind::MethodParameterIn => "METHOD_PARAMETER_IN",
        NodeKind::MethodParameterOut => "METHOD_PARAMETER_OUT",
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
        NodeKind::TypeRef => "TYPE_REF",
        NodeKind::JumpTarget => "JUMP_TARGET",
        NodeKind::Modifier => "MODIFIER",
        NodeKind::Unknown => "UNKNOWN",
    }
}

fn edge_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Ast => "AST",
        EdgeKind::Cfg => "CFG",
        EdgeKind::Call => "CALL",
        EdgeKind::Ref => "REF",
        EdgeKind::Ddg => "DDG",
        EdgeKind::Argument => "ARGUMENT",
        EdgeKind::Receiver => "RECEIVER",
        EdgeKind::Contains => "CONTAINS",
        EdgeKind::ReachingDef => "REACHING_DEF",
        EdgeKind::Condition => "CONDITION",
        EdgeKind::TrueBody => "TRUE_BODY",
        EdgeKind::FalseBody => "FALSE_BODY",
        EdgeKind::ForInit => "FOR_INIT",
        EdgeKind::ForUpdate => "FOR_UPDATE",
        EdgeKind::ForBody => "FOR_BODY",
        EdgeKind::DoBody => "DO_BODY",
        EdgeKind::EvalType => "EVAL_TYPE",
        EdgeKind::SourceFile => "SOURCE_FILE",
        EdgeKind::ParameterLink => "PARAMETER_LINK",
        EdgeKind::Binds => "BINDS",
        EdgeKind::Imports => "IMPORTS",
        EdgeKind::Dominate => "DOMINATE",
        EdgeKind::PostDominate => "POST_DOMINATE",
        EdgeKind::InheritsFrom => "INHERITS_FROM",
        EdgeKind::Capture => "CAPTURE",
    }
}

/// Serialize every live node and outgoing edge occurrence from the actual
/// graph. IDs are local to this graph; independent producers need an occurrence
/// bridge, not an assumption that their IDs agree.
pub fn snapshot(cpg: &Cpg) -> String {
    let mut output = String::new();
    let nodes: Vec<_> = cpg.nodes().collect();
    let edge_count: usize = nodes.iter().map(|&node| cpg.out(node).len()).sum();
    let mut files = cpg.files();
    files.sort_by_key(|file| file.0);
    let layers: Vec<_> = [
        Layer::Ast,
        Layer::SymbolRef,
        Layer::CallGraph,
        Layer::Cfg,
        Layer::Ddg,
        Layer::Summaries,
    ]
    .into_iter()
    .filter(|&layer| cpg.is_layer_authoritative(layer))
    .map(|layer| format!("{layer:?}"))
    .collect();
    writeln!(
        output,
        "{}",
        json!({
            "record": "BEGIN", "schemaVersion": 2, "producer": "cpg-core",
            "nodeCount": nodes.len().to_string(), "edgeCount": edge_count.to_string(),
            "propertyCoverage": "represented columns only",
            "numericClasses": "Rust storage types; no Java runtime-class inference",
            "unknownPropertyPresence": ["ARGUMENT_INDEX"],
            "orderPropertyPresence": "per-node storage.orderPropertyPresence: unknown, absent, or present",
            "unsupportedCoordinateStorage": ["LINE_NUMBER_END", "COLUMN_NUMBER_END", "OFFSET", "OFFSET_END"],
            "edgePropertyStorage": "unsupported",
            "filePartitionIsNotFilenameProperty": true,
            "authoritativeLayers": layers,
            "filePartitions": files.iter().map(|&file| json!({"id": file.0.to_string(), "path": cpg.path_of(file)})).collect::<Vec<_>>()
        })
    ).unwrap();
    for node in nodes {
        let kind = cpg.kind_of(node);
        let mut properties = Map::new();
        optional_string(
            &mut properties,
            if kind == NodeKind::MetaData {
                "LANGUAGE"
            } else {
                "NAME"
            },
            cpg.name_of(node),
        );
        let method_full_name = matches!(
            kind,
            NodeKind::Call | NodeKind::MethodRef | NodeKind::Binding
        );
        optional_string(
            &mut properties,
            if method_full_name {
                "METHOD_FULL_NAME"
            } else {
                "FULL_NAME"
            },
            if method_full_name {
                cpg.method_full_name_of(node)
            } else {
                cpg.full_name_of(node)
            },
        );
        optional_string(&mut properties, "CODE", cpg.code_of(node));
        optional_string(
            &mut properties,
            "TYPE_FULL_NAME",
            cpg.type_full_name_of(node),
        );
        optional_string(
            &mut properties,
            if kind == NodeKind::Type {
                "TYPE_DECL_FULL_NAME"
            } else {
                "SIGNATURE"
            },
            cpg.signature_of(node),
        );
        optional_string(&mut properties, "MODIFIER_TYPE", cpg.modifier_type_of(node));
        optional_string(
            &mut properties,
            "IMPORTED_ENTITY",
            cpg.imported_entity_of(node),
        );
        optional_string(&mut properties, "IMPORTED_AS", cpg.imported_as_of(node));
        optional_string(
            &mut properties,
            "DEPENDENCY_GROUP_ID",
            cpg.dependency_group_id_of(node),
        );
        optional_string(&mut properties, "VERSION", cpg.version_of(node));
        if let Some(line) = cpg.line_of(node) {
            properties.insert("LINE_NUMBER".into(), number("rust.u32", line));
        }
        if let Some(column) = cpg.column_number_of(node) {
            properties.insert("COLUMN_NUMBER".into(), number("rust.i32", column));
        }
        let order_presence = match cpg.order_property_of(node) {
            OrderProperty::Unknown => "unknown",
            OrderProperty::Absent => "absent",
            OrderProperty::Present(value) => {
                properties.insert("ORDER".into(), number("rust.i32", value));
                "present"
            }
        };
        writeln!(
            output,
            "{}",
            json!({
                "record": "NODE", "id": node.0.to_string(), "label": node_label(kind),
                "properties": properties,
                "storage": {
                    "kind": kind.to_u8().to_string(),
                    "filePartitionId": cpg.file_of(node).0.to_string(),
                    "order": number("rust.i32", cpg.order_of(node)),
                    "orderPropertyPresence": order_presence,
                    "argumentIndex": number("rust.i32", cpg.argument_index_of(node))
                }
            })
        )
        .unwrap();
        // Preserve duplicates. Sorting within one adjacency list changes only
        // serialization order; no set or endpoint map collapses occurrences.
        let mut edges = cpg.out(node).to_vec();
        edges.sort_by_key(|edge| (edge.kind.to_u8(), edge.other.0));
        for edge in edges {
            writeln!(
                output,
                "{}",
                json!({
                    "record": "EDGE", "source": node.0.to_string(),
                    "destination": edge.other.0.to_string(), "label": edge_label(edge.kind),
                    "storageKind": edge.kind.to_u8().to_string(),
                    "propertyStorage": "unsupported"
                })
            )
            .unwrap();
        }
    }
    writeln!(output, "{}", json!({"record": "END", "nodeCount": cpg.live_count().to_string(), "edgeCount": edge_count.to_string()})).unwrap();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(snapshot: &str) -> Vec<Value> {
        snapshot
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }

    #[test]
    fn supplemental_preserves_properties_missingness_and_parallel_edges_after_reopen() {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("source path.c");
        let decl = cpg.add_node(NodeKind::TypeDecl, file);
        let binding = cpg.add_node(NodeKind::Binding, file);
        let method = cpg.add_node(NodeKind::Method, file);
        let modifier = cpg.add_node(NodeKind::Modifier, file);
        let empty_modifier = cpg.add_node(NodeKind::Modifier, file);
        let absent_modifier = cpg.add_node(NodeKind::Modifier, file);
        let materialized_type = cpg.add_node(NodeKind::Type, file);
        let metadata = cpg.add_node(NodeKind::MetaData, file);
        let text = "helper<duplicate>0\n雪\r\0\u{2028}";
        let sym = cpg.intern(text);
        cpg.set_method_full_name(binding, sym);
        cpg.set_full_name(method, sym);
        cpg.set_name(binding, sym);
        let sym = cpg.intern("int(int)");
        cpg.set_signature(binding, sym);
        let sym = cpg.intern("owner");
        cpg.set_signature(materialized_type, sym);
        let sym = cpg.intern("NEWC");
        cpg.set_name(metadata, sym);
        let sym = cpg.intern("STATIC");
        cpg.set_modifier_type(modifier, sym);
        let sym = cpg.intern("");
        cpg.set_modifier_type(empty_modifier, sym);
        cpg.set_order(modifier, 3);
        cpg.set_line(modifier, u32::MAX - 1);
        cpg.set_argument_index(binding, i32::MIN);
        for _ in 0..2 {
            cpg.add_edge(decl, binding, EdgeKind::Binds);
            cpg.add_edge(binding, method, EdgeKind::Ref);
        }
        cpg.add_edge(method, modifier, EdgeKind::Ast);
        cpg.mark_layer_authoritative(Layer::SymbolRef);
        let first = snapshot(&cpg);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("graph.cpg");
        cpg.save(path.to_str().unwrap()).unwrap();
        let reopened = Cpg::load(path.to_str().unwrap()).unwrap();
        assert_eq!(snapshot(&reopened), first);
        let rows = rows(&first);
        let node = |id: cpg_core::NodeId| {
            let expected_id = id.0.to_string();
            rows.iter()
                .find(|r| r["record"] == "NODE" && r["id"].as_str() == Some(expected_id.as_str()))
                .unwrap()
        };
        assert_eq!(
            node(binding)["properties"]["METHOD_FULL_NAME"]["value"],
            text
        );
        assert!(node(binding)["properties"].get("FULL_NAME").is_none());
        assert!(node(binding)["properties"].get("CODE").is_none());
        assert_eq!(node(method)["properties"]["FULL_NAME"]["value"], text);
        assert_eq!(
            node(modifier)["properties"]["MODIFIER_TYPE"]["value"],
            "STATIC"
        );
        assert_eq!(
            node(empty_modifier)["properties"]["MODIFIER_TYPE"]["value"],
            ""
        );
        assert!(node(absent_modifier)["properties"]
            .get("MODIFIER_TYPE")
            .is_none());
        assert_eq!(
            node(modifier)["properties"]["LINE_NUMBER"],
            number("rust.u32", u32::MAX - 1)
        );
        assert_eq!(node(modifier)["storage"]["order"], number("rust.i32", 3));
        assert!(node(modifier)["properties"].get("ORDER").is_none());
        assert!(node(modifier)["properties"].get("FILENAME").is_none());
        assert!(node(modifier)["properties"].get("COLUMN_NUMBER").is_none());
        assert_eq!(
            node(binding)["storage"]["argumentIndex"],
            number("rust.i32", i32::MIN)
        );
        assert_eq!(
            node(materialized_type)["properties"]["TYPE_DECL_FULL_NAME"]["value"],
            "owner"
        );
        assert!(node(materialized_type)["properties"]
            .get("SIGNATURE")
            .is_none());
        assert_eq!(node(metadata)["properties"]["LANGUAGE"]["value"], "NEWC");
        assert!(node(metadata)["properties"].get("NAME").is_none());
        assert_eq!(
            rows.iter()
                .filter(|r| r["record"] == "EDGE" && r["label"] == "BINDS")
                .count(),
            2
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["record"] == "EDGE" && r["label"] == "REF")
                .count(),
            2
        );
        assert!(rows
            .iter()
            .filter(|r| r["record"] == "EDGE")
            .all(|r| r.get("property").is_none() && r["propertyStorage"] == "unsupported"));
        assert_eq!(rows[0]["nodeCount"], "8");
        assert_eq!(rows.last().unwrap()["edgeCount"], "5");
    }

    #[test]
    fn supplemental_retains_include_strings_signed_coordinates_and_order_presence() {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("caller.c");
        let import = cpg.add_node(NodeKind::Import, file);
        let dependency = cpg.add_node(NodeKind::Dependency, file);
        let legacy = cpg.add_node(NodeKind::Unknown, file);
        let cleared = cpg.add_node(NodeKind::Import, file);
        let text = "api\n\r\0雪\u{2028}.h";
        let symbol = cpg.intern(text);
        cpg.set_imported_entity(import, symbol);
        let symbol = cpg.intern("");
        cpg.set_imported_as(import, symbol);
        let symbol = cpg.intern("group");
        cpg.set_dependency_group_id(dependency, symbol);
        let symbol = cpg.intern("include");
        cpg.set_version(dependency, symbol);
        cpg.set_column_number(import, i32::MIN);
        cpg.set_order_property(import, i32::MAX);
        cpg.set_order(dependency, 19);
        cpg.clear_order_property(dependency);
        cpg.set_order(legacy, -7);
        cpg.set_column_number(cleared, i32::MAX);
        cpg.clear_column_number(cleared);
        cpg.set_order_property(cleared, 0);
        cpg.clear_order_property(cleared);
        cpg.add_edge(import, dependency, EdgeKind::Imports);
        cpg.add_edge(import, dependency, EdgeKind::Imports);
        let original = snapshot(&cpg);
        let reopened = Cpg::from_bytes(&cpg.to_bytes()).unwrap();
        assert_eq!(snapshot(&reopened), original);
        let rows = rows(&original);
        let node = |id: cpg_core::NodeId| {
            let id = id.0.to_string();
            rows.iter()
                .find(|row| row["record"] == "NODE" && row["id"].as_str() == Some(id.as_str()))
                .unwrap()
        };
        assert_eq!(rows[0]["schemaVersion"], 2);
        assert_eq!(node(import)["label"], "IMPORT");
        assert_eq!(node(import)["properties"]["IMPORTED_ENTITY"], string(text));
        assert_eq!(node(import)["properties"]["IMPORTED_AS"], string(""));
        assert_eq!(
            node(import)["properties"]["COLUMN_NUMBER"],
            number("rust.i32", i32::MIN)
        );
        assert_eq!(
            node(import)["properties"]["ORDER"],
            number("rust.i32", i32::MAX)
        );
        assert_eq!(node(import)["storage"]["orderPropertyPresence"], "present");
        assert!(node(import)["properties"].get("NAME").is_none());
        assert!(node(import)["properties"].get("FILENAME").is_none());
        assert_eq!(node(dependency)["label"], "DEPENDENCY");
        assert_eq!(
            node(dependency)["properties"]["DEPENDENCY_GROUP_ID"],
            string("group")
        );
        assert_eq!(node(dependency)["properties"]["VERSION"], string("include"));
        assert_eq!(node(dependency)["storage"]["order"], number("rust.i32", 19));
        assert_eq!(
            node(dependency)["storage"]["orderPropertyPresence"],
            "absent"
        );
        assert_eq!(node(legacy)["storage"]["orderPropertyPresence"], "unknown");
        for id in [dependency, legacy, cleared] {
            assert!(node(id)["properties"].get("ORDER").is_none());
            assert!(node(id)["properties"].get("COLUMN_NUMBER").is_none());
        }
        assert_eq!(
            rows.iter()
                .filter(|row| row["record"] == "EDGE" && row["label"] == "IMPORTS")
                .count(),
            2
        );
    }

    #[test]
    fn supplemental_formats_in_memory_line_sentinel_without_persistence() {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("main.c");
        let modifier = cpg.add_node(NodeKind::Modifier, file);
        cpg.set_line(modifier, u32::MAX);
        let rows = rows(&snapshot(&cpg));
        let node = rows.iter().find(|r| r["record"] == "NODE").unwrap();
        assert_eq!(
            node["properties"]["LINE_NUMBER"],
            number("rust.u32", u32::MAX)
        );
        // The existing persisted format reserves this value for absence.
        // Serializing the actual in-memory property does not imply it can save.
    }

    #[test]
    fn supplemental_standard_pipeline_snapshot_survives_persistence() {
        let cpg = crate::production::build_graph(&[(
            "main.c".into(),
            "static int helper(int x) { return x; }\nint entry(int x) { return helper(x); }\n"
                .into(),
        )]);
        let canonical = cpg_lang_c::import::canonical_dump(&cpg);
        let original = snapshot(&cpg);
        let reopened = Cpg::from_bytes(&cpg.to_bytes()).unwrap();
        assert_eq!(snapshot(&reopened), original);
        assert_eq!(cpg_lang_c::import::canonical_dump(&reopened), canonical);
        assert!(cpg.is_layer_authoritative(Layer::SymbolRef));
        assert!(reopened.is_layer_authoritative(Layer::SymbolRef));
        let values = rows(&original);
        assert_eq!(values[0]["nodeCount"], cpg.live_count().to_string());
        assert_eq!(
            values.iter().filter(|v| v["record"] == "NODE").count(),
            cpg.live_count()
        );
    }
}
