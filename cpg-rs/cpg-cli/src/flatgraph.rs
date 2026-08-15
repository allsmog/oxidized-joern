//! Joern v4 Flatgraph (`cpg.bin`) interoperability.
//!
//! The wire format is the one used by Flatgraph 0.1.32 in the pinned Joern
//! v4.0.555 oracle: a 16-byte `FLT GRPH` header, independently Zstandard-
//! compressed column blocks, and a trailing JSON manifest.  This module does
//! not depend on a JVM and deliberately rejects malformed lengths/references.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, PropertyValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

const MAGIC: &[u8; 8] = b"FLT GRPH";
const HEADER_LEN: usize = 16;
const MAX_FLATGRAPH_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_BLOCK_BYTES: usize = 512 * 1024 * 1024;
const MAX_NODES: usize = 25_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Outline {
    #[serde(rename = "type")]
    typ: String,
    start_offset: u64,
    compressed_length: usize,
    decompressed_length: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeItem {
    node_label: String,
    nnodes: usize,
    deletions: Option<Vec<usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EdgeItem {
    node_label: String,
    edge_label: String,
    inout: u8,
    qty: Outline,
    neighbors: Outline,
    property: Option<Outline>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PropertyItem {
    node_label: String,
    property_label: String,
    qty: Outline,
    property: Outline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    nodes: Vec<NodeItem>,
    edges: Vec<EdgeItem>,
    properties: Vec<PropertyItem>,
    string_pool_length: Outline,
    string_pool_bytes: Outline,
}

#[derive(Clone, Debug)]
enum Column {
    Strings(Vec<Vec<Option<String>>>),
    Ints(Vec<Vec<i32>>),
    Bools(Vec<Vec<bool>>),
}

impl Column {
    fn empty_for(value: &PropertyValue, count: usize) -> Self {
        match value {
            PropertyValue::Strings(_) => Self::Strings(vec![Vec::new(); count]),
            PropertyValue::Ints(_) => Self::Ints(vec![Vec::new(); count]),
            PropertyValue::Bools(_) => Self::Bools(vec![Vec::new(); count]),
        }
    }

    fn has_values(&self) -> bool {
        match self {
            Self::Strings(values) => values.iter().any(|values| !values.is_empty()),
            Self::Ints(values) => values.iter().any(|values| !values.is_empty()),
            Self::Bools(values) => values.iter().any(|values| !values.is_empty()),
        }
    }

    fn replace_cell(
        &mut self,
        index: usize,
        value: Option<&PropertyValue>,
        cpg: &Cpg,
    ) -> Result<(), String> {
        match (self, value) {
            (Self::Strings(column), Some(PropertyValue::Strings(values))) => {
                column[index] = values
                    .iter()
                    .map(|value| value.map(|symbol| cpg.strings.resolve(symbol).to_string()))
                    .collect();
            }
            (Self::Ints(column), Some(PropertyValue::Ints(values))) => {
                column[index] = values.clone();
            }
            (Self::Bools(column), Some(PropertyValue::Bools(values))) => {
                column[index] = values.clone();
            }
            (Self::Strings(column), None) => column[index].clear(),
            (Self::Ints(column), None) => column[index].clear(),
            (Self::Bools(column), None) => column[index].clear(),
            (_, Some(_)) => return Err("inconsistent Flatgraph property storage type".to_string()),
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum DecodedColumn {
    Strings(Vec<Vec<Option<String>>>),
    Ints(Vec<Vec<i32>>),
    Bools(Vec<Vec<bool>>),
}

/// Export an internal CPG as a Joern v4 Flatgraph file.
pub fn export(cpg: &Cpg, language: &str, output: &Path) -> Result<(), String> {
    let bytes = encode(cpg, language)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("cannot create temporary Flatgraph: {e}"))?;
    use std::io::Write;
    temp.write_all(&bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| format!("cannot write Flatgraph: {e}"))?;
    temp.persist(output)
        .map_err(|e| format!("cannot publish {}: {}", output.display(), e.error))?;
    Ok(())
}

/// Import the Joern v4 schema into the native graph. Unknown future node/edge
/// labels are ignored; every cataloged v4 label and property is preserved,
/// and malformed data fails closed.
pub fn import(input: &Path) -> Result<Cpg, String> {
    let size = std::fs::metadata(input)
        .map_err(|e| format!("cannot inspect {}: {e}", input.display()))?
        .len();
    if size > MAX_FLATGRAPH_BYTES {
        return Err(format!(
            "Flatgraph is {size} bytes; maximum is {MAX_FLATGRAPH_BYTES}"
        ));
    }
    let bytes =
        std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    decode(&bytes)
}

/// Stable digest of the live Flatgraph contents. Manifest/block ordering,
/// compression, and string-pool indices are normalized away; node labels,
/// every property value, outbound edge, and target identity are included.
pub fn content_digest(input: &Path) -> Result<String, String> {
    let size = std::fs::metadata(input)
        .map_err(|e| format!("cannot inspect {}: {e}", input.display()))?
        .len();
    if size > MAX_FLATGRAPH_BYTES {
        return Err(format!(
            "Flatgraph is {size} bytes; maximum is {MAX_FLATGRAPH_BYTES}"
        ));
    }
    let bytes =
        std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    digest_bytes(&bytes)
}

pub fn content_lines(input: &Path) -> Result<Vec<String>, String> {
    let size = std::fs::metadata(input)
        .map_err(|e| format!("cannot inspect {}: {e}", input.display()))?
        .len();
    if size > MAX_FLATGRAPH_BYTES {
        return Err(format!(
            "Flatgraph is {size} bytes; maximum is {MAX_FLATGRAPH_BYTES}"
        ));
    }
    let bytes =
        std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    normalized_lines(&bytes)
}

fn digest_bytes(bytes: &[u8]) -> Result<String, String> {
    let lines = normalized_lines(bytes)?;
    let mut hasher = blake3::Hasher::new();
    for line in lines {
        hasher.update(&(line.len() as u64).to_le_bytes());
        hasher.update(line.as_bytes());
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn normalized_lines(bytes: &[u8]) -> Result<Vec<String>, String> {
    // Reuse the strict decoder first so the digest never blesses a malformed
    // reference, unsupported storage type, or out-of-bounds block.
    let _ = decode(bytes)?;
    let manifest_offset = u64::from_le_bytes(
        bytes
            .get(8..16)
            .ok_or("missing Flatgraph manifest offset")?
            .try_into()
            .expect("8-byte manifest offset"),
    );
    let manifest_offset =
        usize::try_from(manifest_offset).map_err(|_| "manifest offset overflow")?;
    let manifest: Manifest = serde_json::from_slice(
        bytes
            .get(manifest_offset..)
            .ok_or("manifest offset is outside Flatgraph")?,
    )
    .map_err(|e| format!("invalid Flatgraph manifest: {e}"))?;
    let string_pool = read_string_pool(bytes, &manifest)?;
    let mut properties = HashMap::new();
    for property in &manifest.properties {
        let node_count = manifest
            .nodes
            .iter()
            .find(|item| item.node_label == property.node_label)
            .map(|item| item.nnodes)
            .ok_or_else(|| format!("property for unknown node label {}", property.node_label))?;
        properties.insert(
            (property.node_label.clone(), property.property_label.clone()),
            decode_column(bytes, property, node_count, &string_pool)?,
        );
    }
    let mut lines = Vec::new();
    for item in &manifest.nodes {
        let deleted: std::collections::HashSet<usize> = item
            .deletions
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut property_labels: Vec<&str> = properties
            .keys()
            .filter(|(label, _)| label == &item.node_label)
            .map(|(_, property)| property.as_str())
            .collect();
        property_labels.sort_unstable();
        for seq in 0..item.nnodes {
            if deleted.contains(&seq) {
                continue;
            }
            lines.push(format!("N\t{}\t{seq}", item.node_label));
            for property_label in &property_labels {
                let column = &properties[&(item.node_label.clone(), (*property_label).to_string())];
                let value = match column {
                    DecodedColumn::Strings(values) => serde_json::to_string(&values[seq]),
                    DecodedColumn::Ints(values) => serde_json::to_string(&values[seq]),
                    DecodedColumn::Bools(values) => serde_json::to_string(&values[seq]),
                }
                .map_err(|e| format!("cannot normalize property: {e}"))?;
                lines.push(format!(
                    "P\t{}\t{seq}\t{property_label}\t{value}",
                    item.node_label
                ));
            }
        }
    }
    for edge in manifest.edges.iter().filter(|edge| edge.inout == 1) {
        let source_kind = manifest
            .nodes
            .iter()
            .position(|item| item.node_label == edge.node_label)
            .ok_or_else(|| format!("edge for unknown node label {}", edge.node_label))?;
        let offsets = decode_offsets(bytes, &edge.qty, manifest.nodes[source_kind].nnodes)?;
        let neighbors = decode_refs(bytes, &edge.neighbors)?;
        let edge_properties = edge
            .property
            .as_ref()
            .map(|outline| decode_edge_strings(bytes, outline, &string_pool, neighbors.len()))
            .transpose()?;
        for source_seq in 0..manifest.nodes[source_kind].nnodes {
            for (edge_offset, &reference) in slice_for(&neighbors, &offsets, source_seq)?
                .iter()
                .enumerate()
            {
                let target_kind = (reference >> 32) as usize;
                let target_seq = reference as u32 as usize;
                let target = manifest
                    .nodes
                    .get(target_kind)
                    .ok_or_else(|| format!("edge target kind {target_kind} is invalid"))?;
                if target_seq >= target.nnodes {
                    return Err(format!(
                        "edge target {}:{target_seq} is invalid",
                        target.node_label
                    ));
                }
                let property = edge_properties
                    .as_ref()
                    .and_then(|values| values[offsets[source_seq] + edge_offset].as_ref());
                let property = serde_json::to_string(&property)
                    .map_err(|e| format!("cannot normalize edge property: {e}"))?;
                lines.push(format!(
                    "E\t{}\t{source_seq}\t{}\t{}\t{target_seq}\t{property}",
                    edge.node_label, edge.edge_label, target.node_label
                ));
            }
        }
    }
    lines.sort_unstable();
    Ok(lines)
}

fn encode(cpg: &Cpg, language: &str) -> Result<Vec<u8>, String> {
    let kinds = node_kinds();
    let mut grouped = Vec::with_capacity(kinds.len());
    let mut refs = HashMap::new();
    for (kind_index, (_, kind)) in kinds.iter().enumerate() {
        let nodes: Vec<NodeId> = cpg.nodes().filter(|&n| cpg.kind_of(n) == *kind).collect();
        for (seq, &node) in nodes.iter().enumerate() {
            refs.insert(node, encode_ref(kind_index, seq)?);
        }
        grouped.push(nodes);
    }

    let mut output = vec![0_u8; HEADER_LEN];
    let mut properties = Vec::new();
    let mut pending_strings: Vec<(usize, Vec<Option<String>>)> = Vec::new();
    let mut pending_edge_strings: Vec<(usize, Vec<Option<String>>)> = Vec::new();
    let mut string_pool = Vec::<String>::new();
    let mut string_ids = HashMap::<String, i32>::new();

    for (kind_index, (label, kind)) in kinds.iter().enumerate() {
        for (property_label, column) in columns(cpg, &grouped[kind_index], *kind, language)? {
            let (qty, values) = encode_column(
                column,
                properties.len(),
                &mut output,
                &mut pending_strings,
                &mut string_pool,
                &mut string_ids,
            )?;
            properties.push(PropertyItem {
                node_label: (*label).to_string(),
                property_label,
                qty,
                property: values,
            });
        }
    }

    let mut edges = Vec::new();
    for (kind_index, (node_label, _)) in kinds.iter().enumerate() {
        let nodes = &grouped[kind_index];
        for edge_kind in EdgeKind::ALL {
            let Some(edge_label) = edge_label(edge_kind) else {
                continue;
            };
            for direction in [0_u8, 1_u8] {
                let mut offsets = Vec::with_capacity(nodes.len() + 1);
                let mut neighbors = Vec::new();
                let mut edge_properties = Vec::new();
                offsets.push(0_i32);
                for &node in nodes {
                    let adjacent: Box<dyn Iterator<Item = &cpg_core::HalfEdge>> = if direction == 1
                    {
                        Box::new(cpg.out(node).iter().filter(|edge| edge.kind == edge_kind))
                    } else {
                        Box::new(cpg.in_(node).iter().filter(|edge| edge.kind == edge_kind))
                    };
                    for adjacent in adjacent {
                        if let Some(reference) = refs.get(&adjacent.other) {
                            neighbors.push(*reference);
                            let property = adjacent
                                .property
                                .map(|symbol| cpg.strings.resolve(symbol).to_string());
                            if let Some(property) = &property {
                                if !string_ids.contains_key(property) {
                                    let id = i32::try_from(string_pool.len())
                                        .map_err(|_| "too many strings")?;
                                    string_ids.insert(property.clone(), id);
                                    string_pool.push(property.clone());
                                }
                            }
                            edge_properties.push(property);
                        }
                    }
                    offsets.push(i32::try_from(neighbors.len()).map_err(|_| "too many edges")?);
                }
                if neighbors.is_empty() {
                    continue;
                }
                let property = if edge_properties.iter().any(Option::is_some) {
                    pending_edge_strings.push((edges.len(), edge_properties));
                    Some(Outline {
                        typ: "string".to_string(),
                        start_offset: 0,
                        compressed_length: 0,
                        decompressed_length: 0,
                    })
                } else {
                    None
                };
                edges.push(EdgeItem {
                    node_label: (*node_label).to_string(),
                    edge_label: edge_label.to_string(),
                    inout: direction,
                    qty: append_block(&mut output, "int", &delta_encode(&offsets)?)?,
                    neighbors: append_block(&mut output, "ref", &u64_bytes(&neighbors))?,
                    property,
                });
            }
        }
    }

    // String property blocks are written after every string has been assigned
    // a stable pool index, matching Flatgraph's linked insertion-order pool.
    for (outline_index, strings) in pending_strings {
        let ids: Vec<i32> = strings
            .iter()
            .map(|value| value.as_ref().map(|value| string_ids[value]).unwrap_or(-1))
            .collect();
        let outline = append_block(&mut output, "string", &i32_bytes(&ids))?;
        properties[outline_index].property = outline;
    }
    for (outline_index, strings) in pending_edge_strings {
        let ids: Vec<i32> = strings
            .iter()
            .map(|value| value.as_ref().map(|value| string_ids[value]).unwrap_or(-1))
            .collect();
        let outline = append_block(&mut output, "string", &i32_bytes(&ids))?;
        edges[outline_index].property = Some(outline);
    }
    let lengths: Vec<i32> = string_pool
        .iter()
        .map(|value| i32::try_from(value.len()).map_err(|_| "string is too long"))
        .collect::<Result<_, _>>()?;
    let pool_bytes: Vec<u8> = string_pool
        .iter()
        .flat_map(|value| value.as_bytes().iter().copied())
        .collect();
    let string_pool_length = append_block(&mut output, "int", &i32_bytes(&lengths))?;
    let string_pool_bytes = append_block(&mut output, "byte", &pool_bytes)?;

    let manifest = Manifest {
        version: 0,
        nodes: kinds
            .iter()
            .zip(grouped.iter())
            .map(|((label, _), nodes)| NodeItem {
                node_label: (*label).to_string(),
                nnodes: nodes.len(),
                deletions: None,
            })
            .collect(),
        edges,
        properties,
        string_pool_length,
        string_pool_bytes,
    };
    let manifest_offset = u64::try_from(output.len()).map_err(|_| "Flatgraph too large")?;
    output[..8].copy_from_slice(MAGIC);
    output[8..16].copy_from_slice(&manifest_offset.to_le_bytes());
    output.extend_from_slice(b"\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n");
    output.extend_from_slice(
        serde_json::to_string(&manifest)
            .map_err(|e| format!("cannot encode Flatgraph manifest: {e}"))?
            .as_bytes(),
    );
    Ok(output)
}

fn decode(bytes: &[u8]) -> Result<Cpg, String> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return Err("not a Joern v4 Flatgraph (missing `FLT GRPH` header)".to_string());
    }
    let manifest_offset = u64::from_le_bytes(bytes[8..16].try_into().expect("header slice"));
    let manifest_offset =
        usize::try_from(manifest_offset).map_err(|_| "manifest offset overflow")?;
    if !(HEADER_LEN..bytes.len()).contains(&manifest_offset) {
        return Err(format!(
            "manifest offset {manifest_offset} is outside {}-byte file",
            bytes.len()
        ));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes[manifest_offset..])
        .map_err(|e| format!("invalid Flatgraph manifest: {e}"))?;
    if manifest.version != 0 {
        return Err(format!(
            "unsupported Flatgraph manifest version {}",
            manifest.version
        ));
    }
    let total_nodes = manifest.nodes.iter().try_fold(0_usize, |sum, item| {
        sum.checked_add(item.nnodes).ok_or("node count overflow")
    })?;
    if total_nodes > MAX_NODES {
        return Err(format!(
            "Flatgraph has {total_nodes} nodes; maximum is {MAX_NODES}"
        ));
    }
    let string_pool = read_string_pool(bytes, &manifest)?;
    let mut decoded_properties = HashMap::new();
    for property in &manifest.properties {
        let node_count = manifest
            .nodes
            .iter()
            .find(|item| item.node_label == property.node_label)
            .map(|item| item.nnodes)
            .ok_or_else(|| format!("property for unknown node label {}", property.node_label))?;
        let values = decode_column(bytes, property, node_count, &string_pool)?;
        decoded_properties.insert(
            (property.node_label.clone(), property.property_label.clone()),
            values,
        );
    }

    let mut cpg = Cpg::new();
    let mut file_ids = HashMap::new();
    if let Some(DecodedColumn::Strings(names)) =
        decoded_properties.get(&("FILE".to_string(), "NAME".to_string()))
    {
        for values in names {
            if let Some(name) = values.first().and_then(Option::as_ref) {
                let id = cpg.file_id(name);
                file_ids.insert(name.clone(), id);
            }
        }
    }
    let fallback_file = cpg.file_id("<joern-import>");
    let mut node_map: Vec<Vec<Option<NodeId>>> = Vec::with_capacity(manifest.nodes.len());
    for item in &manifest.nodes {
        let mapped_kind = node_kind(&item.node_label);
        let deleted: std::collections::HashSet<usize> = item
            .deletions
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut mapped = Vec::with_capacity(item.nnodes);
        for seq in 0..item.nnodes {
            if deleted.contains(&seq) || mapped_kind.is_none() {
                mapped.push(None);
                continue;
            }
            let kind = mapped_kind.expect("checked");
            let file_name = scalar_string(
                &decoded_properties,
                &item.node_label,
                if kind == NodeKind::File {
                    "NAME"
                } else {
                    "FILENAME"
                },
                seq,
            );
            let file = file_name
                .and_then(|name| file_ids.get(name).copied())
                .unwrap_or(fallback_file);
            let node = cpg.add_node(kind, file);
            let external_label = cpg.intern(&item.node_label);
            cpg.set_external_label(node, external_label);
            apply_properties(&mut cpg, node, &item.node_label, seq, &decoded_properties);
            apply_passthrough_properties(
                &mut cpg,
                node,
                &item.node_label,
                seq,
                &decoded_properties,
            )?;
            mapped.push(Some(node));
        }
        node_map.push(mapped);
    }

    for edge in manifest.edges.iter().filter(|edge| edge.inout == 1) {
        let Some(kind) = edge_kind(&edge.edge_label) else {
            continue;
        };
        let Some(source_kind) = manifest
            .nodes
            .iter()
            .position(|item| item.node_label == edge.node_label)
        else {
            return Err(format!("edge for unknown node label {}", edge.node_label));
        };
        let offsets = decode_offsets(bytes, &edge.qty, manifest.nodes[source_kind].nnodes)?;
        let neighbors = decode_refs(bytes, &edge.neighbors)?;
        let edge_properties = edge
            .property
            .as_ref()
            .map(|outline| decode_edge_strings(bytes, outline, &string_pool, neighbors.len()))
            .transpose()?;
        for source_seq in 0..manifest.nodes[source_kind].nnodes {
            let Some(source) = node_map[source_kind][source_seq] else {
                continue;
            };
            for (edge_offset, &reference) in slice_for(&neighbors, &offsets, source_seq)?
                .iter()
                .enumerate()
            {
                let target_kind = (reference >> 32) as usize;
                let target_seq = reference as u32 as usize;
                if let Some(Some(target)) = node_map
                    .get(target_kind)
                    .and_then(|nodes| nodes.get(target_seq))
                {
                    let property = edge_properties
                        .as_ref()
                        .and_then(|values| values[offsets[source_seq] + edge_offset].as_deref())
                        .map(|value| cpg.intern(value));
                    cpg.add_edge_with_property(source, *target, kind, property);
                }
            }
        }
    }
    Ok(cpg)
}

fn columns(
    cpg: &Cpg,
    nodes: &[NodeId],
    kind: NodeKind,
    language: &str,
) -> Result<Vec<(String, Column)>, String> {
    let mut result = Vec::new();
    let strings = |getter: fn(&Cpg, NodeId) -> Option<&str>| {
        Column::Strings(
            nodes
                .iter()
                .map(|&n| {
                    getter(cpg, n)
                        .map(|value| vec![Some(value.to_string())])
                        .unwrap_or_default()
                })
                .collect(),
        )
    };
    let add_string = |result: &mut Vec<(String, Column)>, name: &str, values: Column| {
        if values.has_values() {
            result.push((name.to_string(), values));
        }
    };
    add_string(&mut result, "NAME", strings(Cpg::name_of));
    add_string(&mut result, "FULL_NAME", strings(Cpg::full_name_of));
    add_string(&mut result, "CODE", strings(Cpg::code_of));
    add_string(
        &mut result,
        "TYPE_FULL_NAME",
        strings(Cpg::type_full_name_of),
    );
    add_string(&mut result, "SIGNATURE", strings(Cpg::signature_of));
    result.push((
        "ORDER".to_string(),
        Column::Ints(nodes.iter().map(|&n| vec![cpg.order_of(n)]).collect()),
    ));
    result.push((
        "ARGUMENT_INDEX".to_string(),
        Column::Ints(
            nodes
                .iter()
                .map(|&n| vec![cpg.argument_index_of(n)])
                .collect(),
        ),
    ));
    let lines: Vec<Vec<i32>> = nodes
        .iter()
        .map(|&n| {
            cpg.line_of(n)
                .and_then(|v| i32::try_from(v).ok())
                .into_iter()
                .collect()
        })
        .collect();
    if lines.iter().any(|values| !values.is_empty()) {
        result.push(("LINE_NUMBER".to_string(), Column::Ints(lines)));
    }
    if matches!(
        kind,
        NodeKind::Method | NodeKind::TypeDecl | NodeKind::NamespaceBlock
    ) {
        result.push((
            "FILENAME".to_string(),
            Column::Strings(
                nodes
                    .iter()
                    .map(|&n| {
                        cpg.path_of(cpg.file_of(n))
                            .map(|value| vec![Some(value.to_string())])
                            .unwrap_or_default()
                    })
                    .collect(),
            ),
        ));
    }
    match kind {
        NodeKind::MetaData => {
            result.push((
                "LANGUAGE".to_string(),
                Column::Strings(
                    nodes
                        .iter()
                        .map(|_| vec![Some(language.to_uppercase())])
                        .collect(),
                ),
            ));
            result.push((
                "VERSION".to_string(),
                Column::Strings(
                    nodes
                        .iter()
                        .map(|_| vec![Some("0.1".to_string())])
                        .collect(),
                ),
            ));
        }
        NodeKind::Method | NodeKind::TypeDecl => result.push((
            "IS_EXTERNAL".to_string(),
            Column::Bools(nodes.iter().map(|_| vec![false]).collect()),
        )),
        NodeKind::MethodParameterIn | NodeKind::MethodParameterOut => {
            result.push((
                "INDEX".to_string(),
                Column::Ints(nodes.iter().map(|&n| vec![cpg.order_of(n)]).collect()),
            ));
            result.push((
                "EVALUATION_STRATEGY".to_string(),
                Column::Strings(
                    nodes
                        .iter()
                        .map(|_| vec![Some("BY_VALUE".to_string())])
                        .collect(),
                ),
            ));
            result.push((
                "IS_VARIADIC".to_string(),
                Column::Bools(nodes.iter().map(|_| vec![false]).collect()),
            ));
        }
        NodeKind::MethodReturn => result.push((
            "EVALUATION_STRATEGY".to_string(),
            Column::Strings(
                nodes
                    .iter()
                    .map(|_| vec![Some("BY_VALUE".to_string())])
                    .collect(),
            ),
        )),
        NodeKind::Call => {
            result.push((
                "METHOD_FULL_NAME".to_string(),
                Column::Strings(
                    nodes
                        .iter()
                        .map(|&n| {
                            cpg.full_name_of(n)
                                .or_else(|| cpg.name_of(n))
                                .map(|value| vec![Some(value.to_string())])
                                .unwrap_or_default()
                        })
                        .collect(),
                ),
            ));
            result.push((
                "DISPATCH_TYPE".to_string(),
                Column::Strings(
                    nodes
                        .iter()
                        .map(|_| vec![Some("STATIC_DISPATCH".to_string())])
                        .collect(),
                ),
            ));
        }
        NodeKind::MethodRef => result.push((
            "METHOD_FULL_NAME".to_string(),
            Column::Strings(
                nodes
                    .iter()
                    .map(|&n| {
                        cpg.full_name_of(n)
                            .or_else(|| cpg.name_of(n))
                            .map(|value| vec![Some(value.to_string())])
                            .unwrap_or_default()
                    })
                    .collect(),
            ),
        )),
        NodeKind::Type => result.push((
            "TYPE_DECL_FULL_NAME".to_string(),
            Column::Strings(
                nodes
                    .iter()
                    .map(|&n| {
                        cpg.full_name_of(n)
                            .or_else(|| cpg.name_of(n))
                            .map(|value| vec![Some(value.to_string())])
                            .unwrap_or_default()
                    })
                    .collect(),
            ),
        )),
        _ => {}
    }
    result.retain(|(property, _)| property_valid(kind, property));
    let mut columns: std::collections::BTreeMap<String, Column> = result.into_iter().collect();
    for &node in nodes {
        for (&label, value) in cpg.passthrough_properties_of(node) {
            let label = cpg.strings.resolve(label).to_string();
            columns
                .entry(label)
                .or_insert_with(|| Column::empty_for(value, nodes.len()));
        }
    }
    for (label, column) in &mut columns {
        for (index, &node) in nodes.iter().enumerate() {
            let value = cpg
                .passthrough_properties_of(node)
                .iter()
                .find(|(symbol, _)| cpg.strings.resolve(**symbol) == label)
                .map(|(_, value)| value);
            // Imported nodes must reproduce the oracle's exact column
            // presence, including an absent mandatory-looking column. Native
            // nodes instead treat sparse properties as an overlay and retain
            // hot columns when there is no sparse value for that label.
            if cpg.external_label_of(node).is_some() || value.is_some() {
                column.replace_cell(index, value, cpg)?;
            }
        }
    }
    columns.retain(|label, column| {
        column.has_values()
            || nodes.iter().any(|&node| {
                cpg.passthrough_properties_of(node)
                    .keys()
                    .any(|symbol| cpg.strings.resolve(*symbol) == label)
            })
    });
    Ok(columns.into_iter().collect())
}

fn property_valid(kind: NodeKind, property: &str) -> bool {
    match property {
        "NAME" => matches!(
            kind,
            NodeKind::File
                | NodeKind::Namespace
                | NodeKind::NamespaceBlock
                | NodeKind::TypeDecl
                | NodeKind::Type
                | NodeKind::Member
                | NodeKind::Method
                | NodeKind::MethodParameterIn
                | NodeKind::MethodParameterOut
                | NodeKind::Call
                | NodeKind::Identifier
                | NodeKind::Local
                | NodeKind::FieldIdentifier
                | NodeKind::JumpTarget
                | NodeKind::Modifier
        ),
        "FULL_NAME" => matches!(
            kind,
            NodeKind::NamespaceBlock | NodeKind::TypeDecl | NodeKind::Type | NodeKind::Method
        ),
        "CODE" => !matches!(kind, NodeKind::Type | NodeKind::MetaData),
        "TYPE_FULL_NAME" => matches!(
            kind,
            NodeKind::Member
                | NodeKind::MethodParameterIn
                | NodeKind::MethodParameterOut
                | NodeKind::MethodReturn
                | NodeKind::Block
                | NodeKind::Call
                | NodeKind::Identifier
                | NodeKind::Literal
                | NodeKind::Local
                | NodeKind::MethodRef
                | NodeKind::TypeRef
                | NodeKind::Unknown
        ),
        "SIGNATURE" => matches!(kind, NodeKind::Method | NodeKind::Call),
        "ORDER" | "ARGUMENT_INDEX" | "LINE_NUMBER" => {
            !matches!(kind, NodeKind::Type | NodeKind::MetaData)
        }
        "FILENAME" => matches!(
            kind,
            NodeKind::Method | NodeKind::TypeDecl | NodeKind::NamespaceBlock
        ),
        "LANGUAGE" | "VERSION" => kind == NodeKind::MetaData,
        "IS_EXTERNAL" => matches!(kind, NodeKind::Method | NodeKind::TypeDecl),
        "INDEX" | "IS_VARIADIC" => matches!(
            kind,
            NodeKind::MethodParameterIn | NodeKind::MethodParameterOut
        ),
        "EVALUATION_STRATEGY" => matches!(
            kind,
            NodeKind::MethodParameterIn | NodeKind::MethodParameterOut | NodeKind::MethodReturn
        ),
        "METHOD_FULL_NAME" | "DISPATCH_TYPE" => {
            matches!(kind, NodeKind::Call | NodeKind::MethodRef)
        }
        "TYPE_DECL_FULL_NAME" => kind == NodeKind::Type,
        _ => false,
    }
}

fn encode_column(
    column: Column,
    property_index: usize,
    output: &mut Vec<u8>,
    pending_strings: &mut Vec<(usize, Vec<Option<String>>)>,
    string_pool: &mut Vec<String>,
    string_ids: &mut HashMap<String, i32>,
) -> Result<(Outline, Outline), String> {
    let mut offsets = Vec::new();
    offsets.push(0_i32);
    match column {
        Column::Strings(values) => {
            let mut flattened = Vec::new();
            for node_values in values {
                for value in node_values {
                    if let Some(value) = &value {
                        if !string_ids.contains_key(value) {
                            let id =
                                i32::try_from(string_pool.len()).map_err(|_| "too many strings")?;
                            string_ids.insert(value.clone(), id);
                            string_pool.push(value.clone());
                        }
                    }
                    flattened.push(value);
                }
                offsets.push(i32::try_from(flattened.len()).map_err(|_| "too many values")?);
            }
            let qty = append_block(output, "int", &delta_encode(&offsets)?)?;
            let placeholder = Outline {
                typ: "string".to_string(),
                start_offset: 0,
                compressed_length: 0,
                decompressed_length: 0,
            };
            pending_strings.push((property_index, flattened));
            Ok((qty, placeholder))
        }
        Column::Ints(values) => {
            let mut flattened = Vec::new();
            for node_values in values {
                flattened.extend(node_values);
                offsets.push(i32::try_from(flattened.len()).map_err(|_| "too many values")?);
            }
            Ok((
                append_block(output, "int", &delta_encode(&offsets)?)?,
                append_block(output, "int", &i32_bytes(&flattened))?,
            ))
        }
        Column::Bools(values) => {
            let mut flattened = Vec::new();
            for node_values in values {
                flattened.extend(node_values.into_iter().map(u8::from));
                offsets.push(i32::try_from(flattened.len()).map_err(|_| "too many values")?);
            }
            Ok((
                append_block(output, "int", &delta_encode(&offsets)?)?,
                append_block(output, "bool", &flattened)?,
            ))
        }
    }
}

fn append_block(output: &mut Vec<u8>, typ: &str, bytes: &[u8]) -> Result<Outline, String> {
    if bytes.len() > MAX_BLOCK_BYTES {
        return Err(format!(
            "Flatgraph block is {} bytes; maximum is {MAX_BLOCK_BYTES}",
            bytes.len()
        ));
    }
    let compressed = zstd::stream::encode_all(Cursor::new(bytes), 3)
        .map_err(|e| format!("Zstandard compression failed: {e}"))?;
    let start_offset = u64::try_from(output.len()).map_err(|_| "Flatgraph too large")?;
    output.extend_from_slice(&compressed);
    Ok(Outline {
        typ: typ.to_string(),
        start_offset,
        compressed_length: compressed.len(),
        decompressed_length: bytes.len(),
    })
}

fn read_block(bytes: &[u8], outline: &Outline) -> Result<Vec<u8>, String> {
    if outline.decompressed_length > MAX_BLOCK_BYTES {
        return Err(format!(
            "Flatgraph block declares {} bytes; maximum is {MAX_BLOCK_BYTES}",
            outline.decompressed_length
        ));
    }
    let start = usize::try_from(outline.start_offset).map_err(|_| "block offset overflow")?;
    let end = start
        .checked_add(outline.compressed_length)
        .ok_or("block range overflow")?;
    let compressed = bytes.get(start..end).ok_or("block range is outside file")?;
    let decoded = zstd::stream::decode_all(Cursor::new(compressed))
        .map_err(|e| format!("Zstandard decompression failed: {e}"))?;
    if decoded.len() != outline.decompressed_length {
        return Err(format!(
            "decompressed block length is {}, expected {}",
            decoded.len(),
            outline.decompressed_length
        ));
    }
    Ok(decoded)
}

fn read_string_pool(bytes: &[u8], manifest: &Manifest) -> Result<Vec<String>, String> {
    let lengths = decode_i32(&read_block(bytes, &manifest.string_pool_length)?)?;
    let pool = read_block(bytes, &manifest.string_pool_bytes)?;
    let mut cursor = 0_usize;
    let mut strings = Vec::with_capacity(lengths.len());
    for length in lengths {
        let length = usize::try_from(length).map_err(|_| "negative string length")?;
        let end = cursor.checked_add(length).ok_or("string pool overflow")?;
        let value = std::str::from_utf8(pool.get(cursor..end).ok_or("string outside pool")?)
            .map_err(|e| format!("invalid UTF-8 in string pool: {e}"))?;
        strings.push(value.to_string());
        cursor = end;
    }
    if cursor != pool.len() {
        return Err("unreferenced bytes at end of string pool".to_string());
    }
    Ok(strings)
}

fn decode_column(
    bytes: &[u8],
    property: &PropertyItem,
    node_count: usize,
    pool: &[String],
) -> Result<DecodedColumn, String> {
    let offsets = decode_offsets(bytes, &property.qty, node_count)?;
    match property.property.typ.as_str() {
        "string" => {
            let ids = decode_i32(&read_block(bytes, &property.property)?)?;
            let values: Result<Vec<Option<String>>, String> = ids
                .into_iter()
                .map(|id| {
                    if id == -1 {
                        return Ok(None);
                    }
                    usize::try_from(id)
                        .ok()
                        .and_then(|id| pool.get(id))
                        .cloned()
                        .map(Some)
                        .ok_or_else(|| format!("string pool index {id} is invalid"))
                })
                .collect();
            Ok(DecodedColumn::Strings(partition(values?, &offsets)?))
        }
        "int" => Ok(DecodedColumn::Ints(partition(
            decode_i32(&read_block(bytes, &property.property)?)?,
            &offsets,
        )?)),
        "bool" => Ok(DecodedColumn::Bools(partition(
            read_block(bytes, &property.property)?
                .into_iter()
                .map(|value| value != 0)
                .collect(),
            &offsets,
        )?)),
        other => Err(format!("unsupported property storage type `{other}`")),
    }
}

fn decode_offsets(
    bytes: &[u8],
    outline: &Outline,
    node_count: usize,
) -> Result<Vec<usize>, String> {
    if outline.typ != "int" {
        return Err(format!("quantity block has type `{}`", outline.typ));
    }
    let deltas = decode_i32(&read_block(bytes, outline)?)?;
    if deltas.len() != node_count + 1 {
        return Err(format!(
            "quantity block has {} entries; expected {}",
            deltas.len(),
            node_count + 1
        ));
    }
    let mut total = 0_usize;
    let mut offsets = Vec::with_capacity(deltas.len());
    for delta in deltas {
        offsets.push(total);
        total = total
            .checked_add(usize::try_from(delta).map_err(|_| "negative quantity")?)
            .ok_or("quantity overflow")?;
    }
    Ok(offsets)
}

fn decode_refs(bytes: &[u8], outline: &Outline) -> Result<Vec<u64>, String> {
    if outline.typ != "ref" {
        return Err(format!("neighbor block has type `{}`", outline.typ));
    }
    let bytes = read_block(bytes, outline)?;
    if !bytes.len().is_multiple_of(8) {
        return Err("reference block length is not divisible by 8".to_string());
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|chunk| u64::from_le_bytes(chunk.try_into().expect("8-byte chunk")))
        .collect())
}

fn decode_edge_strings(
    bytes: &[u8],
    outline: &Outline,
    pool: &[String],
    expected: usize,
) -> Result<Vec<Option<String>>, String> {
    if outline.typ != "string" {
        return Err(format!(
            "unsupported edge property storage type `{}`",
            outline.typ
        ));
    }
    let ids = decode_i32(&read_block(bytes, outline)?)?;
    if ids.len() != expected {
        return Err(format!(
            "edge property has {} values; expected {expected}",
            ids.len()
        ));
    }
    ids.into_iter()
        .map(|id| {
            if id == -1 {
                return Ok(None);
            }
            usize::try_from(id)
                .ok()
                .and_then(|id| pool.get(id))
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("string pool index {id} is invalid"))
        })
        .collect()
}

fn decode_i32(bytes: &[u8]) -> Result<Vec<i32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("integer block length is not divisible by 4".to_string());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| i32::from_le_bytes(chunk.try_into().expect("4-byte chunk")))
        .collect())
}

fn partition<T: Clone>(values: Vec<T>, offsets: &[usize]) -> Result<Vec<Vec<T>>, String> {
    (0..offsets.len() - 1)
        .map(|index| slice_for(&values, offsets, index).map(<[T]>::to_vec))
        .collect()
}

fn slice_for<'a, T>(values: &'a [T], offsets: &[usize], index: usize) -> Result<&'a [T], String> {
    let start = offsets[index];
    let end = offsets[index + 1];
    if start > end {
        return Err("decreasing property/edge offsets".to_string());
    }
    values.get(start..end).ok_or_else(|| {
        format!(
            "property/edge range {start}..{end} exceeds {} values",
            values.len()
        )
    })
}

fn scalar_string<'a>(
    properties: &'a HashMap<(String, String), DecodedColumn>,
    node_label: &str,
    property_label: &str,
    seq: usize,
) -> Option<&'a str> {
    match properties.get(&(node_label.to_string(), property_label.to_string()))? {
        DecodedColumn::Strings(values) => values
            .get(seq)?
            .first()
            .and_then(Option::as_ref)
            .map(String::as_str),
        _ => None,
    }
}

fn scalar_int(
    properties: &HashMap<(String, String), DecodedColumn>,
    node_label: &str,
    property_label: &str,
    seq: usize,
) -> Option<i32> {
    match properties.get(&(node_label.to_string(), property_label.to_string()))? {
        DecodedColumn::Ints(values) => values.get(seq)?.first().copied(),
        _ => None,
    }
}

fn scalar_bool(
    properties: &HashMap<(String, String), DecodedColumn>,
    node_label: &str,
    property_label: &str,
    seq: usize,
) -> Option<bool> {
    match properties.get(&(node_label.to_string(), property_label.to_string()))? {
        DecodedColumn::Bools(values) => values.get(seq)?.first().copied(),
        _ => None,
    }
}

fn apply_properties(
    cpg: &mut Cpg,
    node: NodeId,
    label: &str,
    seq: usize,
    properties: &HashMap<(String, String), DecodedColumn>,
) {
    for (property, setter) in [
        ("NAME", Cpg::set_name as fn(&mut Cpg, NodeId, cpg_core::Sym)),
        ("FULL_NAME", Cpg::set_full_name),
        ("CODE", Cpg::set_code),
        ("TYPE_FULL_NAME", Cpg::set_type_full_name),
        ("SIGNATURE", Cpg::set_signature),
    ] {
        if let Some(value) = scalar_string(properties, label, property, seq) {
            let symbol = cpg.intern(value);
            setter(cpg, node, symbol);
        }
    }
    if let Some(value) = scalar_int(properties, label, "LINE_NUMBER", seq)
        .and_then(|value| u32::try_from(value).ok())
    {
        cpg.set_line(node, value);
    }
    if let Some(value) = scalar_int(properties, label, "ORDER", seq) {
        cpg.set_order(node, value);
    }
    if let Some(value) = scalar_int(properties, label, "ARGUMENT_INDEX", seq)
        .or_else(|| scalar_int(properties, label, "INDEX", seq))
    {
        cpg.set_argument_index(node, value);
    }
    let _ = scalar_bool(properties, label, "IS_EXTERNAL", seq);
}

fn apply_passthrough_properties(
    cpg: &mut Cpg,
    node: NodeId,
    node_label: &str,
    seq: usize,
    properties: &HashMap<(String, String), DecodedColumn>,
) -> Result<(), String> {
    for ((candidate_label, property_label), column) in properties {
        if candidate_label != node_label {
            continue;
        }
        let value = match column {
            DecodedColumn::Strings(values) => {
                let values = values
                    .get(seq)
                    .ok_or("Flatgraph string property row is missing")?
                    .iter()
                    .map(|value| value.as_deref().map(|value| cpg.intern(value)))
                    .collect();
                PropertyValue::Strings(values)
            }
            DecodedColumn::Ints(values) => PropertyValue::Ints(
                values
                    .get(seq)
                    .ok_or("Flatgraph integer property row is missing")?
                    .clone(),
            ),
            DecodedColumn::Bools(values) => PropertyValue::Bools(
                values
                    .get(seq)
                    .ok_or("Flatgraph boolean property row is missing")?
                    .clone(),
            ),
        };
        let property_label = cpg.intern(property_label);
        cpg.set_passthrough_property(node, property_label, value);
    }
    Ok(())
}

fn delta_encode(offsets: &[i32]) -> Result<Vec<u8>, String> {
    let mut deltas = Vec::with_capacity(offsets.len());
    for pair in offsets.windows(2) {
        deltas.push(
            pair[1]
                .checked_sub(pair[0])
                .ok_or("offset delta overflow")?,
        );
    }
    deltas.push(0);
    Ok(i32_bytes(&deltas))
}

fn i32_bytes(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn u64_bytes(values: &[u64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn encode_ref(kind: usize, seq: usize) -> Result<u64, String> {
    let kind = u32::try_from(kind).map_err(|_| "too many node kinds")?;
    let seq = u32::try_from(seq).map_err(|_| "too many nodes of one kind")?;
    Ok((u64::from(kind) << 32) | u64::from(seq))
}

fn node_kinds() -> &'static [(&'static str, NodeKind)] {
    &[
        ("ANNOTATION", NodeKind::Annotation),
        ("ANNOTATION_LITERAL", NodeKind::AnnotationLiteral),
        ("ANNOTATION_PARAMETER", NodeKind::AnnotationParameter),
        (
            "ANNOTATION_PARAMETER_ASSIGN",
            NodeKind::AnnotationParameterAssign,
        ),
        ("ARRAY_INITIALIZER", NodeKind::ArrayInitializer),
        ("BINDING", NodeKind::Binding),
        ("BLOCK", NodeKind::Block),
        ("CALL", NodeKind::Call),
        ("CLOSURE_BINDING", NodeKind::ClosureBinding),
        ("COMMENT", NodeKind::Comment),
        ("CONFIG_FILE", NodeKind::ConfigFile),
        ("CONTROL_STRUCTURE", NodeKind::ControlStructure),
        ("DEPENDENCY", NodeKind::Dependency),
        ("FIELD_IDENTIFIER", NodeKind::FieldIdentifier),
        ("FILE", NodeKind::File),
        ("FINDING", NodeKind::Finding),
        ("IDENTIFIER", NodeKind::Identifier),
        ("IMPORT", NodeKind::Import),
        ("JUMP_LABEL", NodeKind::JumpLabel),
        ("JUMP_TARGET", NodeKind::JumpTarget),
        ("KEY_VALUE_PAIR", NodeKind::KeyValuePair),
        ("LITERAL", NodeKind::Literal),
        ("LOCAL", NodeKind::Local),
        ("MEMBER", NodeKind::Member),
        ("META_DATA", NodeKind::MetaData),
        ("METHOD", NodeKind::Method),
        ("METHOD_PARAMETER_IN", NodeKind::MethodParameterIn),
        ("METHOD_PARAMETER_OUT", NodeKind::MethodParameterOut),
        ("METHOD_REF", NodeKind::MethodRef),
        ("METHOD_RETURN", NodeKind::MethodReturn),
        ("MODIFIER", NodeKind::Modifier),
        ("NAMESPACE", NodeKind::Namespace),
        ("NAMESPACE_BLOCK", NodeKind::NamespaceBlock),
        ("RETURN", NodeKind::Return),
        ("TAG", NodeKind::Tag),
        ("TAG_NODE_PAIR", NodeKind::TagNodePair),
        ("TEMPLATE_DOM", NodeKind::TemplateDom),
        ("TYPE", NodeKind::Type),
        ("TYPE_ARGUMENT", NodeKind::TypeArgument),
        ("TYPE_DECL", NodeKind::TypeDecl),
        ("TYPE_REF", NodeKind::TypeRef),
        // The native schema keeps a total fallback. It is appended so all
        // canonical Joern v4 labels retain their original reference indices.
        ("UNKNOWN", NodeKind::Unknown),
    ]
}

fn node_kind(label: &str) -> Option<NodeKind> {
    node_kinds()
        .iter()
        .find_map(|(candidate, kind)| (*candidate == label).then_some(*kind))
}

fn edge_label(kind: EdgeKind) -> Option<&'static str> {
    Some(match kind {
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
        EdgeKind::EvalType => "EVAL_TYPE",
        EdgeKind::SourceFile => "SOURCE_FILE",
        EdgeKind::ParameterLink => "PARAMETER_LINK",
        EdgeKind::Binds => "BINDS",
        EdgeKind::Dominate => "DOMINATE",
        EdgeKind::PostDominate => "POST_DOMINATE",
        EdgeKind::InheritsFrom => "INHERITS_FROM",
        EdgeKind::Capture => "CAPTURE",
        EdgeKind::TrueBody
        | EdgeKind::FalseBody
        | EdgeKind::ForInit
        | EdgeKind::ForUpdate
        | EdgeKind::ForBody
        | EdgeKind::DoBody => return None,
    })
}

fn edge_kind(label: &str) -> Option<EdgeKind> {
    EdgeKind::ALL
        .into_iter()
        .find(|&kind| edge_label(kind) == Some(label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::{CpgBuilder, Query};

    fn fixture() -> Cpg {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("flat.c");
        let mut b = CpgBuilder::new(&mut cpg, file);
        let file_node = b.file_node("flat.c");
        let method = b.method("main", "main", "int()", Some(1));
        b.contains(file_node, method);
        let call = b.call("puts", "puts(\"ok\")", Some(2));
        b.ast_child(method, call);
        let literal = b.literal("\"ok\"", Some(2));
        b.add_argument(call, literal, 1);
        cpg.add_edge(method, call, EdgeKind::Cfg);
        cpg
    }

    #[test]
    fn native_flatgraph_round_trip_preserves_supported_graph() {
        let source = fixture();
        let bytes = encode(&source, "c").expect("encode");
        assert_eq!(&bytes[..8], MAGIC);
        let restored = decode(&bytes).expect("decode");
        assert_eq!(restored.method_named("main").len(), 1);
        let call = restored.calls_named("puts")[0];
        assert_eq!(restored.code_of(call), Some("puts(\"ok\")"));
        assert_eq!(restored.arguments_of(call).len(), 1);
        assert_eq!(
            restored
                .out_kind(restored.method_named("main")[0], EdgeKind::Cfg)
                .count(),
            1
        );
        let reencoded = encode(&restored, "c").expect("reencode");
        assert_eq!(
            digest_bytes(&bytes).expect("first digest"),
            digest_bytes(&reencoded).expect("second digest")
        );
    }

    #[test]
    fn native_sparse_properties_overlay_without_erasing_hot_columns() {
        let mut source = fixture();
        let call = source.calls_named("puts")[0];
        let label = source.intern("DYNAMIC_TYPE_HINT_FULL_NAME");
        let value = source.intern("fixture.Dynamic");
        source.set_passthrough_property(call, label, PropertyValue::Strings(vec![Some(value)]));

        let restored = decode(&encode(&source, "c").expect("encode")).expect("decode");
        let call = restored.calls_named("puts")[0];
        assert_eq!(restored.name_of(call), Some("puts"));
        assert_eq!(restored.code_of(call), Some("puts(\"ok\")"));
        let PropertyValue::Strings(values) = restored
            .passthrough_property_named(call, "DYNAMIC_TYPE_HINT_FULL_NAME")
            .expect("sparse property")
        else {
            panic!("wrong sparse property type");
        };
        assert_eq!(
            values
                .iter()
                .filter_map(|value| value.map(|symbol| restored.strings.resolve(symbol)))
                .collect::<Vec<_>>(),
            vec!["fixture.Dynamic"]
        );
    }

    #[test]
    fn malformed_header_and_offset_fail_closed() {
        assert!(decode(b"not a graph").is_err());
        let mut bytes = encode(&fixture(), "c").unwrap();
        bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }
}
