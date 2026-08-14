use std::fs;
use std::process::Command;

#[test]
fn reports_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_jimpleastgen"))
        .arg("-version")
        .output()
        .expect("run jimpleastgen");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn extracts_minimal_class_file() {
    let input = tempfile::tempdir().expect("input dir");
    let out = tempfile::tempdir().expect("out dir");
    let class_path = input.path().join("Foo.class");
    fs::write(&class_path, minimal_class("pkg/Foo")).expect("write class");

    let output = Command::new(env!("CARGO_BIN_EXE_jimpleastgen"))
        .arg("-out")
        .arg(out.path())
        .arg(input.path())
        .output()
        .expect("run jimpleastgen");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.path().join("pkg").join("Foo.class").is_file());
    assert!(out.path().join("manifest.json").is_file());
}

fn minimal_class(internal_name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&52u16.to_be_bytes());
    bytes.extend_from_slice(&3u16.to_be_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&(internal_name.len() as u16).to_be_bytes());
    bytes.extend_from_slice(internal_name.as_bytes());
    bytes.push(7);
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&0x0021u16.to_be_bytes());
    bytes.extend_from_slice(&2u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes
}
