use cpg_core::persist::ByteReader;
use cpg_core::{Cpg, NodeKind, OrderProperty};

#[test]
fn accepted_version_one_graphs_upgrade_without_changing_existing_payload() {
    let cases = [
        "duplicate_inherited_static",
        "duplicate_inline_before_static",
        "duplicate_literal_static",
        "duplicate_plain",
        "duplicate_same_file_location_order",
        "duplicate_static_late_prototype",
        "duplicate_supplied_macro_local",
        "unique_plain_and_static",
    ];
    for name in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cpg2-v1")
            .join(name)
            .join("accepted-v1.cpg");
        let old = std::fs::read(path).unwrap();
        assert_eq!(&old[..6], b"CPG2\x01\x00", "{name}");
        let graph = Cpg::from_bytes(&old).unwrap();
        assert!(
            graph.nodes().all(|n| graph.modifier_type_of(n).is_none()),
            "{name}"
        );
        assert!(
            graph.nodes().all(|n| graph.kind_of(n) != NodeKind::Binding),
            "{name}"
        );
        let upgraded = graph.to_bytes();
        assert_eq!(&upgraded[..6], b"CPG2\x04\x00", "{name}");
        // Check preserved checksum configuration and authoritative-layer flags.
        assert_eq!(&upgraded[6..12], &old[6..12], "{name}");

        const ENVELOPE: usize = 24;
        let mut reader = ByteReader::new(&old[ENVELOPE..]);
        let string_count = reader.u64().unwrap();
        for _ in 0..string_count {
            reader.bytes().unwrap();
        }
        let node_count = reader.u64().unwrap() as usize;
        // Kind, file, and the five original interned-string columns precede
        // the new modifier column. The fixed old writer payload is the oracle.
        let modifier_start = ENVELOPE + reader.position() + node_count * (1 + 4 + 5 * 4);
        let modifier_end = modifier_start + node_count * 4;
        // Version 3 appends four absent symbols, an absent column tag and an
        // unknown ORDER tag for every old node. The version 2 prefix remains.
        let extension_start = upgraded.len() - node_count * 18;
        assert_eq!(extension_start, old.len() + node_count * 4, "{name}");
        assert!(
            upgraded[extension_start..extension_start + node_count * 16]
                .iter()
                .all(|&b| b == 255),
            "{name}"
        );
        assert!(
            upgraded[extension_start + node_count * 16..]
                .iter()
                .all(|&b| b == 0),
            "{name}"
        );
        assert!(
            upgraded[modifier_start..modifier_end]
                .iter()
                .all(|&b| b == 255),
            "{name}"
        );
        let without_new_column = [
            &upgraded[ENVELOPE..modifier_start],
            &upgraded[modifier_end..extension_start],
        ]
        .concat();
        assert_eq!(
            without_new_column,
            old[ENVELOPE..],
            "{name}: original columns, edges and file table"
        );
        let reopened = Cpg::from_bytes(&upgraded).unwrap();
        assert_eq!(reopened.to_bytes(), upgraded, "{name}: second reopen");
        assert!(reopened
            .nodes()
            .all(|n| reopened.order_property_of(n) == OrderProperty::Unknown));
    }
}
