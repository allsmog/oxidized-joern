//! Complete canonical references and separately observed identity metadata.
use cpg_core::{Cpg, EdgeKind, NodeKind};
use std::path::{Path, PathBuf};

const CASES: [&str; 8] = [
    "duplicate_inherited_static",
    "duplicate_inline_before_static",
    "duplicate_literal_static",
    "duplicate_plain",
    "duplicate_same_file_location_order",
    "duplicate_static_late_prototype",
    "duplicate_supplied_macro_local",
    "unique_plain_and_static",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn build(name: &str) -> Cpg {
    let family = if CASES.contains(&name) {
        "function-identities"
    } else {
        "primitive-members"
    };
    let root = fixtures().join(family).join("cases").join(name);
    let mut sources: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "c" || ext == "h"))
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
fn duplicate_function_identities_match_complete_live_graphs() {
    for name in CASES {
        let cpg = build(name);
        let expected = std::fs::read_to_string(
            fixtures()
                .join("function-identities/cases")
                .join(name)
                .join("expected.txt"),
        )
        .unwrap();
        assert_eq!(cpg_lang_c::import::canonical_dump(&cpg), expected, "{name}");
    }
}

fn stored_metadata(cpg: &Cpg) -> String {
    let present = |value: Option<&str>| value.unwrap_or("<absent>").to_owned();
    let mut rows = Vec::new();
    for node in cpg.nodes() {
        match cpg.kind_of(node) {
            NodeKind::Binding => {
                let parents: Vec<_> = cpg.in_kind(node, EdgeKind::Binds).collect();
                let targets: Vec<_> = cpg.out_kind(node, EdgeKind::Ref).collect();
                assert_eq!(parents.len(), 1);
                assert_eq!(targets.len(), 1);
                assert_eq!(cpg.kind_of(parents[0]), NodeKind::TypeDecl);
                assert_eq!(cpg.kind_of(targets[0]), NodeKind::Method);
                assert!(cpg.code_of(node).is_none());
                assert!(cpg.type_full_name_of(node).is_none());
                assert!(cpg.line_of(node).is_none());
                assert_eq!(cpg.out_kind(node, EdgeKind::SourceFile).count(), 0);
                rows.push(format!(
                    "BINDING|{}|{}|{}|{}|{}",
                    present(cpg.name_of(node)),
                    present(cpg.method_full_name_of(node)),
                    present(cpg.signature_of(node)),
                    present(cpg.full_name_of(parents[0])),
                    present(cpg.full_name_of(targets[0]))
                ));
            }
            NodeKind::Modifier => {
                let parents: Vec<_> = cpg.in_kind(node, EdgeKind::Ast).collect();
                assert_eq!(parents.len(), 1);
                rows.push(format!(
                    "MODIFIER|{}|{}|{}|{}",
                    present(cpg.full_name_of(parents[0])),
                    cpg.order_of(node),
                    present(cpg.modifier_type_of(node)),
                    cpg.line_of(node)
                        .map_or("<absent>".into(), |line| line.to_string())
                ));
            }
            NodeKind::MethodRef => {
                let targets: Vec<_> = cpg.out_kind(node, EdgeKind::Ref).collect();
                assert_eq!(targets.len(), 1);
                let mut owner = node;
                while cpg.kind_of(owner) != NodeKind::Method {
                    let parents: Vec<_> = cpg.in_kind(owner, EdgeKind::Ast).collect();
                    assert_eq!(parents.len(), 1);
                    owner = parents[0];
                }
                rows.push(format!(
                    "METHOD_REF|{}|{}|{}|{}|{}|{}|{}",
                    present(cpg.code_of(node)),
                    present(cpg.method_full_name_of(node)),
                    present(cpg.type_full_name_of(node)),
                    present(cpg.full_name_of(targets[0])),
                    present(cpg.full_name_of(owner)),
                    cpg.order_of(node),
                    cpg.line_of(node)
                        .map_or("<absent>".into(), |line| line.to_string()),
                ));
            }
            _ => {}
        }
    }
    rows.sort();
    rows.into_iter().map(|row| row + "\n").collect()
}

#[test]
fn observed_bindings_modifiers_and_references_survive_pipeline_and_storage() {
    // Complete raw supplements retain unsupported columns, end locations and
    // IMPORT/DEPENDENCY nodes. This comparison covers only the stated stored
    // properties and all their binding/reference endpoint occurrences.
    for name in CASES.into_iter().chain([
        "tiny_fixedtables_include",
        "tiny_fixedtables_inline",
        "unsigned_short_array",
    ]) {
        let cpg = build(name);
        let expected = std::fs::read_to_string(
            fixtures()
                .join("function-identities/supplement")
                .join(name)
                .join("stored-metadata-v2.txt"),
        )
        .unwrap();
        assert_eq!(stored_metadata(&cpg), expected, "{name}");
        let canonical = cpg_lang_c::import::canonical_dump(&cpg);
        let bytes = cpg.to_bytes();
        assert_eq!(&bytes[..6], b"CPG2\x03\x00");
        let reopened = Cpg::from_bytes(&bytes).unwrap();
        assert_eq!(stored_metadata(&reopened), expected, "{name}: reopened");
        assert_eq!(
            cpg_lang_c::import::canonical_dump(&reopened),
            canonical,
            "{name}: canonical storage"
        );
    }
}
