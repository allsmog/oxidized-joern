//! Code embeddings (JoernVectors parity): bag-of-properties vectors per
//! node, feature-hashed, plus the edge list — the "Pattern-based
//! Vulnerability Discovery" chapter-3 representation the Scala tool emits.
//!
//! One JSON document:
//!   {"objects":  [node ids],
//!    "dimToFeature": {hash: "key:value"}        (only with --features)
//!    "vectors":  [{ "key:value": count, ... }]  (one per object, same order)
//!    "edges":    [{"src": id, "dst": id, "label": kind}]}
//!
//! Substructures per node match BagOfPropertiesForNodes: the node id, its
//! name / full_name / code properties (sorted by key), and its kind label.
//! The Scala tool hashes with MurmurHash3; the exact hash function is not
//! part of the format contract (dimToFeature is emitted precisely so
//! consumers never depend on it), so we use FNV-1a, which is deterministic
//! across runs and platforms with zero dependencies.

use cpg_core::Cpg;
use std::collections::BTreeMap;
use std::io::Write;

/// FNV-1a 32-bit: deterministic feature hashing (dimension = decimal string).
fn feature_hash(s: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h.to_string()
}

/// The (key, value) substructures of one node, in the Scala tool's order:
/// id first, property fields sorted by key, label last.
fn substructures(cpg: &Cpg, n: cpg_core::NodeId) -> Vec<(String, String)> {
    let mut props: Vec<(String, String)> = Vec::new();
    if let Some(v) = cpg.code_of(n) {
        props.push(("code".into(), v.to_string()));
    }
    if let Some(v) = cpg.full_name_of(n) {
        props.push(("full_name".into(), v.to_string()));
    }
    if let Some(v) = cpg.name_of(n) {
        props.push(("name".into(), v.to_string()));
    }
    let mut out = vec![("id".to_string(), n.0.to_string())];
    out.extend(props); // already key-sorted: code < full_name < name
    out.push(("label".to_string(), format!("{:?}", cpg.kind_of(n))));
    out
}

/// Write the whole embedding document. Streams — never materialises the
/// document as one string (sd-sized graphs run to millions of nodes).
pub fn write_vectors(cpg: &Cpg, dim_to_feature: bool, w: &mut impl Write) -> std::io::Result<()> {
    let nodes: Vec<cpg_core::NodeId> = cpg.nodes().collect();

    writeln!(w, "{{")?;
    writeln!(w, "\"objects\":")?;
    writeln!(w, "[")?;
    for (i, n) in nodes.iter().enumerate() {
        let sep = if i + 1 == nodes.len() { "" } else { "," };
        writeln!(w, "\"{}\"{sep}", n.0)?;
    }
    writeln!(w, "]")?;

    if dim_to_feature {
        let mut dims: BTreeMap<String, String> = BTreeMap::new();
        for &n in &nodes {
            for (k, v) in substructures(cpg, n) {
                let feature = format!("{k}:{v}");
                dims.insert(feature_hash(&feature), feature);
            }
        }
        writeln!(w, ",\"dimToFeature\": ")?;
        write!(w, "{}", serde_json::to_string(&dims).expect("serialize"))?;
        writeln!(w)?;
    }

    writeln!(w, ",\"vectors\":")?;
    writeln!(w, "[")?;
    for (i, &n) in nodes.iter().enumerate() {
        let mut counts: BTreeMap<String, f64> = BTreeMap::new();
        for (k, v) in substructures(cpg, n) {
            *counts.entry(format!("{k}:{v}")).or_insert(0.0) += 1.0;
        }
        let sep = if i + 1 == nodes.len() { "" } else { "," };
        writeln!(
            w,
            "{}{sep}",
            serde_json::to_string(&counts).expect("serialize")
        )?;
    }
    writeln!(w, "]")?;

    writeln!(w, ",\"edges\":")?;
    writeln!(w, "[")?;
    let mut first = true;
    for &n in &nodes {
        for e in cpg.out(n) {
            if !first {
                writeln!(w, ",")?;
            }
            first = false;
            write!(
                w,
                "{{\"src\":{},\"dst\":{},\"label\":\"{:?}\"}}",
                n.0, e.other.0, e.kind
            )?;
        }
    }
    writeln!(w)?;
    writeln!(w, "]")?;
    writeln!(w, "}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_hash_is_deterministic_and_distinct() {
        assert_eq!(feature_hash("name:main"), feature_hash("name:main"));
        assert_ne!(feature_hash("name:main"), feature_hash("name:free"));
    }

    #[test]
    fn vectors_document_is_valid_json_with_aligned_arrays() {
        use cpg_core::{EdgeKind, NodeKind};
        let mut cpg = Cpg::new();
        let f = cpg.file_id("a.c");
        let m = cpg.add_node(NodeKind::Method, f);
        let s = cpg.intern("main");
        cpg.set_name(m, s);
        let c = cpg.add_node(NodeKind::Call, f);
        let s = cpg.intern("free");
        cpg.set_name(c, s);
        cpg.add_edge(m, c, EdgeKind::Ast);

        let mut buf = Vec::new();
        write_vectors(&cpg, true, &mut buf).unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&buf).expect("valid json");
        let objects = doc["objects"].as_array().unwrap();
        let vectors = doc["vectors"].as_array().unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects.len(), vectors.len());
        assert_eq!(doc["edges"].as_array().unwrap().len(), 1);
        // Every vector dimension appears in dimToFeature.
        let dims = doc["dimToFeature"].as_object().unwrap();
        for v in vectors {
            for feature in v.as_object().unwrap().keys() {
                assert!(dims.values().any(|d| d.as_str() == Some(feature)));
            }
        }
        // The method's vector carries its name and label features.
        let mv = vectors[0].as_object().unwrap();
        assert!(mv.contains_key("name:main"));
        assert!(mv.contains_key("label:Method"));
    }
}
