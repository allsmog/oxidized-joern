use serde_json::{json, Value};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

fn fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "cpg-query-cli-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.c"),
        "int main(int argc) { char dst[8]; strcpy(dst, getenv(\"X\")); return argc; }\n",
    )
    .unwrap();
    root
}

#[test]
fn query_command_executes_property_and_count_traversals() {
    let root = fixture();
    let binary = env!("CARGO_BIN_EXE_cpg");

    let output = Command::new(binary)
        .args([
            "query",
            &root.to_string_lossy(),
            "--lang",
            "c",
            "--query",
            r#"cpg.call.name("strcpy").argument(2).code"#,
        ])
        .output()
        .expect("run cpg query");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value, json!(["getenv(\"X\")"]));

    let output = Command::new(binary)
        .args([
            "query",
            &root.to_string_lossy(),
            "--lang",
            "c",
            "--query",
            r#"cpg.call.name("getenv|strcpy").size"#,
        ])
        .output()
        .expect("run count query");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(2)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn query_command_rejects_unknown_steps() {
    let root = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_cpg"))
        .args([
            "query",
            &root.to_string_lossy(),
            "--lang",
            "c",
            "--query",
            "cpg.call.noSuchStep",
        ])
        .output()
        .expect("run invalid query");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported step"));
    let _ = std::fs::remove_dir_all(root);
}
