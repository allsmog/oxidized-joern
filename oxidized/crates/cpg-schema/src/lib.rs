use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../schema/cpg-schema.json"
));

pub fn schema() -> &'static CpgSchema {
    static SCHEMA: OnceLock<CpgSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(SCHEMA_JSON).expect("embedded schema/cpg-schema.json must parse")
    })
}

pub fn validate_fragment(fragment_json: &str) -> Vec<Violation> {
    let fragment = match serde_json::from_str::<Value>(fragment_json) {
        Ok(value) => value,
        Err(error) => {
            return vec![Violation::InvalidJson {
                message: error.to_string(),
            }];
        }
    };

    let Some(fragment) = fragment.as_object() else {
        return vec![Violation::InvalidFragmentShape {
            message: "fragment root must be a JSON object".to_string(),
        }];
    };

    let schema = schema();
    let mut violations = Vec::new();
    let mut nodes_by_id = BTreeMap::new();

    match fragment.get("nodes").and_then(Value::as_array) {
        Some(nodes) => {
            for (index, node) in nodes.iter().enumerate() {
                validate_node(index, node, schema, &mut nodes_by_id, &mut violations);
            }
        }
        None => violations.push(Violation::InvalidFragmentShape {
            message: "fragment.nodes must be an array".to_string(),
        }),
    }

    match fragment.get("edges").and_then(Value::as_array) {
        Some(edges) => {
            for (index, edge) in edges.iter().enumerate() {
                validate_edge(index, edge, schema, &nodes_by_id, &mut violations);
            }
        }
        None => violations.push(Violation::InvalidFragmentShape {
            message: "fragment.edges must be an array".to_string(),
        }),
    }

    violations
}

#[derive(Debug, Deserialize)]
pub struct CpgSchema {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "cpgVersion")]
    pub cpg_version: String,
    pub metadata: SchemaMetadata,
    pub nodes: BTreeMap<String, NodeSchema>,
    pub edges: BTreeMap<String, EdgeSchema>,
}

impl CpgSchema {
    pub fn node(&self, label: &str) -> Option<&NodeSchema> {
        self.nodes.get(label)
    }

    pub fn edge(&self, label: &str) -> Option<&EdgeSchema> {
        self.edges.get(label)
    }

    pub fn node_property(&self, node_label: &str, property_name: &str) -> Option<&PropertySchema> {
        self.node(node_label)?
            .properties
            .iter()
            .find(|property| property.name == property_name)
    }

    pub fn allows_edge_endpoint(&self, edge_label: &str, src_label: &str, dst_label: &str) -> bool {
        self.edge(edge_label)
            .is_some_and(|edge| edge.allows_endpoint(src_label, dst_label))
    }
}

#[derive(Debug, Deserialize)]
pub struct SchemaMetadata {
    #[serde(rename = "nodeCount")]
    pub node_count: u32,
    #[serde(rename = "edgeCount")]
    pub edge_count: u32,
    #[serde(rename = "propertyKindCount")]
    pub property_kind_count: u32,
    #[serde(rename = "normalNodePropertyCount")]
    pub normal_node_property_count: u32,
    pub source: String,
    #[serde(rename = "allowedEndpointSource")]
    pub allowed_endpoint_source: String,
    #[serde(rename = "generatedBy")]
    pub generated_by: String,
}

#[derive(Debug, Deserialize)]
pub struct NodeSchema {
    pub properties: Vec<PropertySchema>,
    #[serde(rename = "allowedOutEdges")]
    pub allowed_out_edges: BTreeMap<String, Vec<String>>,
    #[serde(rename = "allowedInEdges")]
    pub allowed_in_edges: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct PropertySchema {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: PropertyType,
    pub cardinality: Cardinality,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyType {
    Bool,
    String,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Ref,
    Nothing,
}

impl PropertyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::String => "string",
            Self::Byte => "byte",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
            Self::Ref => "ref",
            Self::Nothing => "nothing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cardinality {
    One,
    Optional,
    Multi,
    None,
}

impl Cardinality {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Optional => "optional",
            Self::Multi => "multi",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EdgeSchema {
    #[serde(rename = "srcLabels")]
    pub src_labels: Vec<String>,
    #[serde(rename = "dstLabels")]
    pub dst_labels: Vec<String>,
    #[serde(rename = "allowedEndpoints")]
    pub allowed_endpoints: Vec<AllowedEndpoint>,
}

impl EdgeSchema {
    pub fn allows_endpoint(&self, src_label: &str, dst_label: &str) -> bool {
        self.allowed_endpoints
            .iter()
            .any(|endpoint| endpoint.src == src_label && endpoint.dst == dst_label)
    }
}

#[derive(Debug, Deserialize)]
pub struct AllowedEndpoint {
    pub src: String,
    pub dst: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    InvalidJson {
        message: String,
    },
    InvalidFragmentShape {
        message: String,
    },
    DuplicateNodeId {
        node_id: String,
    },
    UnknownNodeLabel {
        node_id: String,
        label: String,
    },
    UnknownProperty {
        node_id: String,
        label: String,
        property: String,
    },
    InvalidPropertyType {
        node_id: String,
        label: String,
        property: String,
        expected_type: String,
        expected_cardinality: String,
        actual: String,
    },
    UnknownEdgeLabel {
        edge_index: usize,
        label: String,
    },
    UnknownEdgeEndpoint {
        edge_index: usize,
        endpoint: EdgeEndpoint,
        node_id: String,
    },
    IllegalEdgeEndpoint {
        edge_index: usize,
        label: String,
        src_id: String,
        src_label: String,
        dst_id: String,
        dst_label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeEndpoint {
    Src,
    Dst,
}

#[derive(Debug, Clone)]
struct FragmentNode {
    label: String,
}

fn validate_node(
    index: usize,
    node: &Value,
    schema: &CpgSchema,
    nodes_by_id: &mut BTreeMap<String, FragmentNode>,
    violations: &mut Vec<Violation>,
) {
    let Some(node_object) = node.as_object() else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("nodes[{index}] must be an object"),
        });
        return;
    };

    let Some(node_id) = node_object.get("id").and_then(normalize_id) else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("nodes[{index}].id must be a string or integer"),
        });
        return;
    };

    let Some(label) = node_object.get("label").and_then(Value::as_str) else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("nodes[{index}].label must be a string"),
        });
        return;
    };
    let label = label.to_string();

    if nodes_by_id
        .insert(
            node_id.clone(),
            FragmentNode {
                label: label.clone(),
            },
        )
        .is_some()
    {
        violations.push(Violation::DuplicateNodeId {
            node_id: node_id.clone(),
        });
    }

    let node_schema = match schema.node(&label) {
        Some(node_schema) => node_schema,
        None => {
            violations.push(Violation::UnknownNodeLabel { node_id, label });
            return;
        }
    };

    let Some(properties_value) = node_object.get("properties") else {
        return;
    };

    let Some(properties) = properties_value.as_object() else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("nodes[{index}].properties must be an object"),
        });
        return;
    };

    let known_properties = node_schema
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect::<BTreeSet<_>>();

    for (property_name, property_value) in properties {
        let Some(property_schema) = node_schema
            .properties
            .iter()
            .find(|property| property.name == *property_name)
        else {
            violations.push(Violation::UnknownProperty {
                node_id: node_id.clone(),
                label: label.clone(),
                property: property_name.clone(),
            });
            continue;
        };

        if !property_value_matches(property_value, property_schema) {
            violations.push(Violation::InvalidPropertyType {
                node_id: node_id.clone(),
                label: label.clone(),
                property: property_name.clone(),
                expected_type: property_schema.value_type.as_str().to_string(),
                expected_cardinality: property_schema.cardinality.as_str().to_string(),
                actual: actual_value_shape(property_value),
            });
        }
    }

    debug_assert!(known_properties.len() == node_schema.properties.len());
}

fn validate_edge(
    index: usize,
    edge: &Value,
    schema: &CpgSchema,
    nodes_by_id: &BTreeMap<String, FragmentNode>,
    violations: &mut Vec<Violation>,
) {
    let Some(edge_object) = edge.as_object() else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("edges[{index}] must be an object"),
        });
        return;
    };

    let Some(src_id) = edge_object.get("src").and_then(normalize_id) else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("edges[{index}].src must be a string or integer"),
        });
        return;
    };
    let Some(dst_id) = edge_object.get("dst").and_then(normalize_id) else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("edges[{index}].dst must be a string or integer"),
        });
        return;
    };
    let Some(label) = edge_object.get("label").and_then(Value::as_str) else {
        violations.push(Violation::InvalidFragmentShape {
            message: format!("edges[{index}].label must be a string"),
        });
        return;
    };
    let label = label.to_string();

    if schema.edge(&label).is_none() {
        violations.push(Violation::UnknownEdgeLabel {
            edge_index: index,
            label: label.clone(),
        });
    }

    let Some(src_node) = nodes_by_id.get(&src_id) else {
        violations.push(Violation::UnknownEdgeEndpoint {
            edge_index: index,
            endpoint: EdgeEndpoint::Src,
            node_id: src_id,
        });
        return;
    };
    let Some(dst_node) = nodes_by_id.get(&dst_id) else {
        violations.push(Violation::UnknownEdgeEndpoint {
            edge_index: index,
            endpoint: EdgeEndpoint::Dst,
            node_id: dst_id,
        });
        return;
    };

    if schema.edge(&label).is_some()
        && schema.node(&src_node.label).is_some()
        && schema.node(&dst_node.label).is_some()
        && !schema.allows_edge_endpoint(&label, &src_node.label, &dst_node.label)
    {
        violations.push(Violation::IllegalEdgeEndpoint {
            edge_index: index,
            label,
            src_id,
            src_label: src_node.label.clone(),
            dst_id,
            dst_label: dst_node.label.clone(),
        });
    }
}

fn normalize_id(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) if value.is_i64() || value.is_u64() => Some(value.to_string()),
        _ => None,
    }
}

fn property_value_matches(value: &Value, property: &PropertySchema) -> bool {
    match property.cardinality {
        Cardinality::One => scalar_type_matches(value, &property.value_type),
        Cardinality::Optional => {
            value.is_null() || scalar_type_matches(value, &property.value_type)
        }
        Cardinality::Multi => value.as_array().is_some_and(|values| {
            values
                .iter()
                .all(|value| scalar_type_matches(value, &property.value_type))
        }),
        Cardinality::None => value.is_null(),
    }
}

fn scalar_type_matches(value: &Value, value_type: &PropertyType) -> bool {
    match value_type {
        PropertyType::Bool => value.is_boolean(),
        PropertyType::String | PropertyType::Ref => value.is_string(),
        PropertyType::Byte | PropertyType::Short | PropertyType::Int | PropertyType::Long => {
            value.as_i64().is_some() || value.as_u64().is_some()
        }
        PropertyType::Float | PropertyType::Double => value.is_number(),
        PropertyType::Nothing => value.is_null(),
    }
}

fn actual_value_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
            "integer".to_string()
        }
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(values) => {
            let element_shapes = values
                .iter()
                .map(actual_value_shape)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("|");
            format!("array[{element_shapes}]")
        }
        Value::Object(_) => "object".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_schema() {
        let schema = schema();
        assert_eq!(schema.schema_version, 1);
        assert_eq!(schema.cpg_version, "1.7.70");
        assert!(schema.node("METHOD").is_some());
        assert!(schema.edge("AST").is_some());

        let full_name = schema.node_property("METHOD", "FULL_NAME").unwrap();
        assert_eq!(full_name.value_type, PropertyType::String);
        assert_eq!(full_name.cardinality, Cardinality::One);
        assert!(schema.allows_edge_endpoint("AST", "METHOD", "BLOCK"));
    }

    #[test]
    fn accepts_a_schema_clean_fragment() {
        assert!(validate_fragment(valid_fragment()).is_empty());
    }

    #[test]
    fn reports_unknown_node_labels() {
        let violations = validate_fragment(
            r#"{
              "nodes": [{"id": 1, "label": "NOT_A_NODE", "properties": {}}],
              "edges": []
            }"#,
        );

        assert!(matches!(
            violations.as_slice(),
            [Violation::UnknownNodeLabel { label, .. }] if label == "NOT_A_NODE"
        ));
    }

    #[test]
    fn reports_unknown_properties() {
        let violations = validate_fragment(
            r#"{
              "nodes": [{"id": 1, "label": "METHOD", "properties": {"NO_SUCH_PROPERTY": "x"}}],
              "edges": []
            }"#,
        );

        assert!(matches!(
            violations.as_slice(),
            [Violation::UnknownProperty { property, .. }] if property == "NO_SUCH_PROPERTY"
        ));
    }

    #[test]
    fn reports_mistyped_properties() {
        let violations = validate_fragment(
            r#"{
              "nodes": [{"id": 1, "label": "METHOD", "properties": {"FULL_NAME": 7}}],
              "edges": []
            }"#,
        );

        assert!(matches!(
            violations.as_slice(),
            [Violation::InvalidPropertyType { property, expected_type, actual, .. }]
                if property == "FULL_NAME" && expected_type == "string" && actual == "integer"
        ));
    }

    #[test]
    fn reports_illegal_edge_endpoints() {
        let violations = validate_fragment(
            r#"{
              "nodes": [
                {"id": 1, "label": "METHOD", "properties": {}},
                {"id": 2, "label": "BLOCK", "properties": {}}
              ],
              "edges": [{"src": 1, "dst": 2, "label": "EVAL_TYPE"}]
            }"#,
        );

        assert!(matches!(
            violations.as_slice(),
            [Violation::IllegalEdgeEndpoint { label, src_label, dst_label, .. }]
                if label == "EVAL_TYPE" && src_label == "METHOD" && dst_label == "BLOCK"
        ));
    }

    #[test]
    fn reports_unknown_edge_labels_and_missing_endpoints() {
        let violations = validate_fragment(
            r#"{
              "nodes": [{"id": 1, "label": "METHOD", "properties": {}}],
              "edges": [{"src": 1, "dst": 2, "label": "NO_EDGE"}]
            }"#,
        );

        assert!(violations.iter().any(|violation| {
            matches!(violation, Violation::UnknownEdgeLabel { label, .. } if label == "NO_EDGE")
        }));
        assert!(violations.iter().any(|violation| {
            matches!(violation, Violation::UnknownEdgeEndpoint {
                endpoint: EdgeEndpoint::Dst,
                node_id,
                ..
            } if node_id == "2")
        }));
    }

    fn valid_fragment() -> &'static str {
        r#"{
          "nodes": [
            {
              "id": 1,
              "label": "METHOD",
              "properties": {
                "AST_PARENT_FULL_NAME": "example",
                "AST_PARENT_TYPE": "NAMESPACE_BLOCK",
                "CODE": "fn main() {}",
                "FILENAME": "main.rs",
                "FULL_NAME": "example::main",
                "GENERIC_SIGNATURE": "",
                "IS_EXTERNAL": false,
                "NAME": "main",
                "ORDER": 1,
                "SIGNATURE": "()"
              }
            },
            {
              "id": 2,
              "label": "BLOCK",
              "properties": {
                "ARGUMENT_INDEX": 1,
                "CODE": "{}",
                "ORDER": 1,
                "TYPE_FULL_NAME": "ANY"
              }
            }
          ],
          "edges": [{"src": 1, "dst": 2, "label": "AST"}]
        }"#
    }
}
