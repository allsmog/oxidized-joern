use cpg_core::persist::ByteReader;
use cpg_core::{Cpg, NodeKind, OrderProperty};

#[test]
fn accepted_version_two_graphs_keep_the_complete_payload_prefix() {
    for name in [
        "duplicate_supplied_macro_local",
        "inactive_body_include",
        "repeated_direct_include",
        "tiny_fixedtables_include",
        "tiny_fixedtables_inline",
        "unresolved_include_pair",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cpg2-v2")
            .join(name)
            .join("accepted-v2.cpg");
        let old = std::fs::read(path).unwrap();
        assert_eq!(&old[..6], b"CPG2\x02\x00", "{name}");
        let graph = Cpg::from_bytes(&old).unwrap();
        for node in graph.nodes() {
            assert!(!matches!(
                graph.kind_of(node),
                NodeKind::Import | NodeKind::Dependency
            ));
            assert_eq!(graph.imported_entity_of(node), None);
            assert_eq!(graph.imported_as_of(node), None);
            assert_eq!(graph.dependency_group_id_of(node), None);
            assert_eq!(graph.version_of(node), None);
            assert_eq!(graph.column_number_of(node), None);
            assert_eq!(graph.order_property_of(node), OrderProperty::Unknown);
        }
        let upgraded = graph.to_bytes();
        assert_eq!(&upgraded[..6], b"CPG2\x04\x00", "{name}");
        assert_eq!(
            &upgraded[6..12],
            &old[6..12],
            "{name}: checksum config/layers"
        );
        const ENVELOPE: usize = 24;
        assert_eq!(
            &upgraded[ENVELOPE..old.len()],
            &old[ENVELOPE..],
            "{name}: complete original payload"
        );
        let mut reader = ByteReader::new(&old[ENVELOPE..]);
        let strings = reader.u64().unwrap();
        for _ in 0..strings {
            reader.bytes().unwrap();
        }
        let nodes = reader.u64().unwrap() as usize;
        assert_eq!(upgraded.len(), old.len() + nodes * 18, "{name}");
        assert!(upgraded[old.len()..old.len() + nodes * 16]
            .iter()
            .all(|&b| b == 255));
        assert!(upgraded[old.len() + nodes * 16..].iter().all(|&b| b == 0));
        let reopened = Cpg::from_bytes(&upgraded).unwrap();
        assert_eq!(
            reopened.to_bytes(),
            upgraded,
            "{name}: deterministic reopen"
        );
    }
}
