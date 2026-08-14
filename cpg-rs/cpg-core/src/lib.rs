//! `cpg-core` — the language-independent code property graph.
//!
//! Columnar, file-partitioned, mutable storage (see [`graph`]); a closed
//! schema ([`schema`]); shared construction primitives ([`builder`]); and a
//! small query surface ([`traversal`]). Frontends, passes, and dataflow all
//! build on these and nothing else.

pub mod builder;
pub mod freeze;
pub mod graph;
pub mod intern;
pub mod persist;
pub mod schema;
pub mod segments;
pub mod traversal;

pub use builder::CpgBuilder;
pub use freeze::{Freeze, FrozenCpg};
pub use graph::{Cpg, FileId, HalfEdge, NodeId, PropertyValue};
pub use intern::Sym;
pub use schema::{EdgeKind, Layer, NodeKind};
pub use segments::{SegmentDescriptor, SegmentDigest, SegmentKey, SegmentManifest};
pub use traversal::Query;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_remove_file_is_incremental() {
        let mut cpg = Cpg::new();
        let f = cpg.file_id("a.c");
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("a.c");
            let m = b.method("main", "main", "int(void)", Some(1));
            b.contains(file, m);
            let call = b.call("puts", "puts(\"hi\")", Some(2));
            b.contains(m, call);
        }
        assert_eq!(cpg.methods().len(), 1);
        assert_eq!(cpg.calls().len(), 1);
        let before = cpg.live_count();

        // Removing the file's subgraph reclaims exactly those nodes.
        cpg.remove_file(f);
        assert_eq!(cpg.methods().len(), 0);
        assert_eq!(cpg.calls().len(), 0);
        assert!(cpg.live_count() < before);

        // Rebuilding reuses the tombstoned slots (free list), so a long-lived
        // process editing one file repeatedly does not leak node ids.
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            let _ = b.method("main", "main", "int(void)", Some(1));
        }
        assert_eq!(cpg.methods().len(), 1);
    }

    #[test]
    fn persistence_round_trips() {
        let mut cpg = Cpg::new();
        let f = cpg.file_id("a.c");
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("a.c");
            let m = b.method("main", "main", "int(void)", Some(1));
            b.contains(file, m);
            let p = b.parameter("argc", "int", 1);
            b.ast_child(m, p);
            let call = b.call("puts", "puts(\"hi\")", Some(2));
            b.contains(m, call);
            let lit = b.literal("\"hi\"", Some(2));
            b.add_argument(call, lit, 1);
        }
        // Create a tombstone so the free-list path is exercised too.
        let g = cpg.file_id("gone.c");
        {
            let mut b = CpgBuilder::new(&mut cpg, g);
            let _ = b.method("dead", "dead", "()", None);
        }
        cpg.remove_file(g);

        let call = cpg.calls_named("puts")[0];
        let external_label = cpg.intern("CALL");
        let property_label = cpg.intern("DYNAMIC_TYPE_HINT_FULL_NAME");
        let property_value = cpg.intern("java.lang.String");
        cpg.set_external_label(call, external_label);
        cpg.set_passthrough_property(
            call,
            property_label,
            PropertyValue::Strings(vec![Some(property_value)]),
        );

        let bytes = cpg.to_bytes();
        let restored = Cpg::from_bytes(&bytes).expect("decode");

        assert_eq!(restored.live_count(), cpg.live_count());
        assert_eq!(restored.methods().len(), 1);
        let m = restored.method_named("main")[0];
        assert_eq!(restored.parameters_of(m).len(), 1);
        let call = restored.calls_named("puts")[0];
        assert_eq!(restored.external_label_of(call), Some("CALL"));
        let passthrough = restored.passthrough_properties_of(call);
        let (_, value) = passthrough.iter().next().expect("passthrough property");
        assert!(matches!(value, PropertyValue::Strings(values) if values.len() == 1));
        assert_eq!(restored.arguments_of(call).len(), 1);
        // In-edges were rebuilt: the literal knows its incident Argument edge.
        let lit = restored.arguments_of(call)[0];
        assert_eq!(restored.in_kind(lit, EdgeKind::Argument).count(), 1);
        assert_eq!(restored.path_of(f), Some("a.c"));
    }
}
