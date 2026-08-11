//! End-to-end MCP session against the real binary over stdio: initialize
//! handshake, tool listing, a cached build, an entry-driven scan with the
//! coverage report, a glob flow query, and the methodology resource.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const SRC: &str = r#"
#include <stdlib.h>
void doit(char *c) { system(c); }
int main() {
    char *cmd = getenv("CMD");
    doit(cmd);
    return 0;
}
"#;

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cpg-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn mcp_session_covers_the_iris_loop() {
    let root = scratch("root");
    let cache = scratch("cache");
    std::fs::create_dir_all(root.join("svc")).unwrap();
    std::fs::write(root.join("svc/app.c"), SRC).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_cpg"))
        .args(["mcp", "--root", &root.to_string_lossy()])
        .env("CPG_CACHE", &cache)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cpg mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let mut send = |v: Value| {
        stdin.write_all(v.to_string().as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    };
    let mut recv = || -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read response");
        serde_json::from_str(&line).expect("valid json-rpc")
    };

    // -- initialize handshake
    send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"}}}));
    let init = recv();
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "cpg");
    // notification: must produce no response
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    // -- tools/list
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = recv();
    assert_eq!(tools["id"], 2, "notification must not have been answered");
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "build_cpg",
        "scan",
        "flow",
        "taint",
        "slice",
        "apis",
        "merge",
        "list_rules",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }

    // -- build_cpg
    send(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "build_cpg", "arguments": {"path": "svc"}}}));
    let built = recv();
    assert_eq!(built["result"]["isError"], false);
    let text: Value =
        serde_json::from_str(built["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["lang"], "c");
    assert!(text["methods"].as_u64().unwrap() >= 2);

    // -- scan with the built-in pack: getenv -> system must surface,
    //    and the coverage report must be attached
    send(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "scan", "arguments": {"path": "svc"}}}));
    let scanned = recv();
    assert_eq!(scanned["result"]["isError"], false);
    let scan: Value =
        serde_json::from_str(scanned["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(scan["totalFindings"].as_u64().unwrap() >= 1, "{scan}");
    assert!(scan["coverage"].as_str().unwrap().contains("coverage:"));

    // -- flow glob query finds the interprocedural chain
    send(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": {"name": "flow", "arguments":
            {"path": "svc", "source_glob": "getenv", "sink_glob": "sys*"}}}));
    let flowed = recv();
    let flow: Value =
        serde_json::from_str(flowed["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let findings = flow["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f["method"] == "main" && f["sink"] == "system"),
        "getenv->doit->system: {findings:?}"
    );

    // -- iris pack selection through the scan tool
    send(json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
        "params": {"name": "scan", "arguments": {"path": "svc", "rules": "iris:jvm-exec"}}}));
    let iris = recv();
    assert_eq!(iris["result"]["isError"], false, "{iris}");

    // -- methodology resource
    send(
        json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read",
        "params": {"uri": "iris://methodology"}}),
    );
    let doc = recv();
    assert!(doc["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("IRIS"));

    // -- unknown method -> -32601
    send(json!({"jsonrpc": "2.0", "id": 8, "method": "prompts/list"}));
    let err = recv();
    assert_eq!(err["error"]["code"], -32601);

    drop(stdin);
    let status = child.wait().expect("server exits when stdin closes");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cache);
}
