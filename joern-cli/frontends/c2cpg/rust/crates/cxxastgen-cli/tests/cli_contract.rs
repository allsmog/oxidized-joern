use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

#[test]
fn prints_version() {
    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-version")
        .assert()
        .success()
        .stdout("v0.1.0\n");
}

#[test]
fn writes_one_json_document_per_cxx_input() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let include = temp.path().join("include");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&include).unwrap();
    fs::write(input.join("main.c"), "int main() {\n  return VALUE;\n}\n").unwrap();
    fs::write(input.join("ignored.txt"), "not c").unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-include")
        .arg(&include)
        .arg("-define")
        .arg("VALUE=7")
        .arg("-skip-function-bodies")
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    assert_eq!(document["schemaVersion"], 1);
    assert_eq!(document["backend"], "oxidized-cxxastgen");
    assert_eq!(document["language"], "c");
    assert_eq!(document["sourceLines"], 3);
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["VALUE=7"])
    );
    assert_eq!(document["options"]["skipFunctionBodies"], true);
    assert_eq!(document["options"]["importHeaderDeclarations"], false);
    assert!(document["options"]["includePaths"][0]
        .as_str()
        .unwrap()
        .ends_with("/include"));
    assert!(!out.join("ignored.txt.json").exists());
}

#[test]
fn reports_unmapped_node_kinds_on_stderr_only() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    // Unsupported variadic marker inside an object-like macro still falls back
    // to the legacy branch handling.
    fs::write(
        input.join("main.c"),
        "#define A __VA_ARGS__\n#if A\nint f() { return 1; }\n#else\nint f() { return 0; }\n#endif\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("cxxastgen:")
            && stderr.contains("unmapped node(s):")
            && stderr.contains("preproc_macro_stringize_or_variadic"),
        "expected unmapped summary on stderr, got: {stderr:?}"
    );

    // The summary must never reach stdout or the emitted JSON document.
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("unmapped node(s)"),
        "stdout was: {stdout:?}"
    );
    let document = fs::read_to_string(out.join("main.c.json")).unwrap();
    assert!(
        !document.contains("unmapped node(s)"),
        "JSON document leaked the summary"
    );
    // The JSON is still valid.
    let _: Value = serde_json::from_str(&document).unwrap();
}

#[test]
fn token_paste_macro_conditions_emit_selected_branch_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "#define A foo ## bar\nint f() {\n#if A\n  return 1;\n#else\n  return 0;\n#endif\n}\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "token paste should be expanded, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let return_statement = &document["declarations"][1]["body"][0];
    assert_eq!(return_statement["kind"], "return");
    assert_eq!(return_statement["expression"]["kind"], "literal");
    assert_eq!(return_statement["expression"]["value"], "0");
}

#[test]
fn gnu_asm_expression_emits_unknown_expression_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int f() { int x = asm(\"nop\"); return x; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "GNU asm expression should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let initializer = &document["declarations"][0]["body"][0]["initializer"];
    assert_eq!(initializer["kind"], "unknown");
    assert_eq!(initializer["code"], "asm(\"nop\")");
}

#[test]
fn requires_expression_emits_unknown_initializer_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.cpp"),
        "template <typename T> bool f() { bool ok = requires(T t) { t + 1; }; return ok; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "requires expression should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.cpp.json")).unwrap()).unwrap();
    let initializer = &document["declarations"][0]["body"][0]["initializer"];
    assert_eq!(initializer["kind"], "unknown");
    assert_eq!(initializer["code"], "requires(T t) { t + 1; }");
    assert_eq!(document["declarations"][0]["body"][1]["kind"], "return");
}

#[test]
fn return_requires_expression_emits_unknown_return_expression_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.cpp"),
        "template <typename T> bool f() { return requires(T t) { t + 1; }; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "return requires expression should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.cpp.json")).unwrap()).unwrap();
    let return_statement = &document["declarations"][0]["body"][0];
    assert_eq!(return_statement["kind"], "return");
    assert_eq!(return_statement["expression"]["kind"], "unknown");
    assert_eq!(
        return_statement["expression"]["code"],
        "requires(T t) { t + 1; }"
    );
}

#[test]
fn static_assert_emits_operator_call_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.cpp"),
        "void foo(){ int a = 0; static_assert ( a == 0 , \"not 0!\"); }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "static_assert should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.cpp.json")).unwrap()).unwrap();
    let expression = &document["declarations"][0]["body"][1]["expression"];
    assert_eq!(expression["kind"], "call");
    assert_eq!(expression["name"], "<operator>.staticAssert");
    assert_eq!(expression["code"], "static_assert ( a == 0 , \"not 0!\");");
    assert_eq!(expression["arguments"][0]["kind"], "binary");
    assert_eq!(expression["arguments"][0]["operator"], "==");
    assert_eq!(expression["arguments"][1]["kind"], "literal");
    assert_eq!(expression["arguments"][1]["value"], "\"not 0!\"");
}

#[test]
fn namespace_alias_emits_alias_and_expands_qualified_constructor_names() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.cpp"),
        "namespace A { class Foo { public: static int make() { return 2; } }; int qux = 1; int fn() { return 3; } }\nnamespace B = A;\nauto f = B::Foo();\nvoid use() { namespace C = A; auto local = C::Foo(); }\nint read() { return B::qux; }\nint call_free() { return B::fn(); }\nint call_static() { return B::Foo::make(); }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "namespace alias should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.cpp.json")).unwrap()).unwrap();
    assert_eq!(document["declarations"][1]["kind"], "namespaceAlias");
    assert_eq!(document["declarations"][1]["name"], "B");
    assert_eq!(document["declarations"][1]["target"], "A");

    let initializer = &document["declarations"][2]["initializer"];
    assert_eq!(initializer["kind"], "call");
    assert_eq!(initializer["name"], "A::Foo");
    assert_eq!(initializer["code"], "B::Foo()");

    let local_alias = &document["declarations"][3]["body"][0];
    assert_eq!(local_alias["kind"], "namespaceAlias");
    assert_eq!(local_alias["name"], "C");
    assert_eq!(local_alias["target"], "A");
    let local_initializer = &document["declarations"][3]["body"][1]["initializer"];
    assert_eq!(local_initializer["kind"], "call");
    assert_eq!(local_initializer["name"], "A::Foo");
    assert_eq!(local_initializer["code"], "C::Foo()");

    let read_return = &document["declarations"][4]["body"][0]["expression"];
    assert_eq!(read_return["kind"], "fieldAccess");
    assert_eq!(read_return["code"], "B::qux");
    assert_eq!(read_return["field"], "qux");
    assert_eq!(read_return["base"]["kind"], "identifier");
    assert_eq!(read_return["base"]["name"], "B");

    let free_call = &document["declarations"][5]["body"][0]["expression"];
    assert_eq!(free_call["kind"], "call");
    assert_eq!(free_call["name"], "A::fn");
    assert_eq!(free_call["code"], "B::fn()");
    assert_eq!(free_call["resolvedMethodFullName"], "A.fn:int()");
    assert_eq!(free_call["resolvedSignature"], "int()");

    let static_call = &document["declarations"][6]["body"][0]["expression"];
    assert_eq!(static_call["kind"], "call");
    assert_eq!(static_call["name"], "A::Foo::make");
    assert_eq!(static_call["code"], "B::Foo::make()");
    assert_eq!(static_call["resolvedMethodFullName"], "A.Foo.make:int()");
    assert_eq!(static_call["resolvedSignature"], "int()");
}

#[test]
fn seh_leave_statement_emits_unknown_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "void f() { __try { g(); __leave; h(); } __finally { cleanup(); } }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "__leave should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let try_statement = &document["declarations"][0]["body"][0];
    assert_eq!(try_statement["kind"], "try");
    let body = try_statement["body"].as_array().unwrap();
    assert_eq!(body.len(), 4);
    assert_eq!(body[1]["kind"], "unknown");
    assert_eq!(body[1]["code"], "__leave");
    let call_names: Vec<_> = body
        .iter()
        .filter_map(|statement| statement["expression"]["name"].as_str())
        .collect();
    assert_eq!(call_names, vec!["g", "h", "cleanup"]);
}

#[test]
fn extension_expression_emits_child_expression_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int f(int x) { int y = __extension__ (x + 1); return __extension__ y; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "__extension__ should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let body = document["declarations"][0]["body"].as_array().unwrap();
    assert_eq!(body[0]["kind"], "localDecl");
    assert_eq!(body[0]["initializer"]["kind"], "binary");
    assert_eq!(body[0]["initializer"]["operator"], "+");
    assert_eq!(body[0]["initializer"]["left"]["name"], "x");
    assert_eq!(body[0]["initializer"]["right"]["value"], "1");
    assert_eq!(body[1]["kind"], "return");
    assert_eq!(body[1]["expression"]["kind"], "identifier");
    assert_eq!(body[1]["expression"]["name"], "y");
}

#[test]
fn generic_selection_emits_mapped_call_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int f(int x) { return _Generic(x, int: 1, default: 0); }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "_Generic should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let expression = &document["declarations"][0]["body"][0]["expression"];
    assert_eq!(expression["kind"], "call");
    assert_eq!(expression["name"], "_Generic");
    assert_eq!(expression["callee"]["kind"], "identifier");
    assert_eq!(expression["callee"]["name"], "_Generic");

    let arguments = expression["arguments"].as_array().unwrap();
    assert_eq!(arguments.len(), 3);
    assert_eq!(arguments[0]["kind"], "identifier");
    assert_eq!(arguments[0]["name"], "x");
    assert_eq!(arguments[1]["kind"], "literal");
    assert_eq!(arguments[1]["value"], "1");
    assert_eq!(arguments[2]["kind"], "literal");
    assert_eq!(arguments[2]["value"], "0");
    assert!(
        arguments
            .iter()
            .all(|argument| argument["name"] != "int: 1" && argument["name"] != "default: 0"),
        "association labels leaked as fake identifiers: {arguments:?}"
    );
}

#[test]
fn nested_function_emits_function_decl_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int f(int x) { int g(int y) { return x + y; } return g(1); }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "nested functions should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let body = document["declarations"][0]["body"].as_array().unwrap();
    assert_eq!(body[0]["kind"], "functionDecl");
    assert_eq!(body[0]["function"]["name"], "g");
    assert_eq!(body[0]["function"]["signature"], "int(int)");
    assert_eq!(body[0]["function"]["body"][0]["kind"], "return");
    assert_eq!(
        body[0]["function"]["body"][0]["expression"]["operator"],
        "+"
    );
    assert_eq!(body[1]["kind"], "return");
    assert_eq!(body[1]["expression"]["kind"], "call");
    assert_eq!(body[1]["expression"]["name"], "g");
    assert_eq!(
        body[1]["expression"]["resolvedMethodFullName"],
        "g:int(int)"
    );
}

#[test]
fn gnu_asm_emits_unknown_without_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "void f() { asm(\"paddh %0, %1, %2\\n\\t\" : \"=f\" (x) : \"f\" (y), \"f\" (z)); }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "asm should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let statement = &document["declarations"][0]["body"][0];
    assert_eq!(statement["kind"], "unknown");
    assert!(statement["code"].as_str().unwrap().starts_with("asm("));
}

#[test]
fn statement_expression_emits_mapped_block_expression() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int main() { int x = ({ int y = 1; y; }); return x; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "statement expression should be fully mapped, got: {stderr:?}"
    );

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let initializer = &document["declarations"][0]["body"][0]["initializer"];
    assert_eq!(initializer["kind"], "statementExpression");
    assert_eq!(initializer["body"][0]["kind"], "localDecl");
    assert_eq!(initializer["body"][1]["kind"], "expression");
}

#[test]
fn fully_mapped_source_emits_no_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int add(int a, int b) { int total = a + b; return total; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "did not expect an unmapped summary, got: {stderr:?}"
    );
}

#[test]
fn applies_exclude_regex() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("keep.c"), "int keep() { return 1; }\n").unwrap();
    fs::write(input.join("skip.c"), "int skip() { return 0; }\n").unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-exclude")
        .arg("skip\\.c$")
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    assert!(out.join("keep.c.json").exists());
    assert!(!out.join("skip.c.json").exists());
}

#[test]
fn uses_compile_database_sources_and_options() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("project");
    let src = input.join("src");
    let include = input.join("include");
    let system_include = input.join("system-include");
    let cli_include = input.join("cli-include");
    let out = temp.path().join("out");
    let compile_database = input.join("compile_commands.json");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&include).unwrap();
    fs::create_dir_all(&system_include).unwrap();
    fs::create_dir_all(&cli_include).unwrap();
    fs::write(src.join("main.c"), "int main() { return DB_DEFINE; }\n").unwrap();
    fs::write(src.join("not_in_database.c"), "int stray() { return 0; }\n").unwrap();
    fs::write(
        &compile_database,
        serde_json::to_string_pretty(&serde_json::json!([
            {
                "directory": input,
                "file": "src/main.c",
                "arguments": [
                    "cc",
                    "-I",
                    "include",
                    "-isystem",
                    "system-include",
                    "-DDB_DEFINE=1",
                    "/DMSVC_DEFINE",
                    "-c",
                    "src/main.c"
                ]
            }
        ]))
        .unwrap(),
    )
    .unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-include")
        .arg(&cli_include)
        .arg("-define")
        .arg("CLI_DEFINE=1")
        .arg("-compilation-database")
        .arg(&compile_database)
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    assert!(out.join("src/main.c.json").exists());
    assert!(!out.join("src/not_in_database.c.json").exists());

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("src/main.c.json")).unwrap()).unwrap();
    let include_paths = document["options"]["includePaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/cli-include")));
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/include")));
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/system-include")));
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["CLI_DEFINE=1", "DB_DEFINE=1", "MSVC_DEFINE"])
    );
    assert!(document["options"]["compilationDatabase"]
        .as_str()
        .unwrap()
        .ends_with("/compile_commands.json"));
    assert_eq!(document["options"]["importHeaderDeclarations"], true);
}

#[test]
fn parses_compile_database_command_lines() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("project");
    let include = input.join("quoted include");
    let out = temp.path().join("out");
    let compile_database = input.join("compile_commands.json");
    let source = input.join("main.c");
    fs::create_dir_all(&include).unwrap();
    fs::write(&source, "int main() { return QUOTED_DEFINE; }\n").unwrap();
    fs::write(
        &compile_database,
        serde_json::to_string_pretty(&serde_json::json!([
            {
                "directory": input,
                "file": "main.c",
                "command": "cc -I 'quoted include' -DQUOTED_DEFINE=1 -c main.c"
            }
        ]))
        .unwrap(),
    )
    .unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-compilation-database")
        .arg(&compile_database)
        .arg("-out")
        .arg(&out)
        .arg(&source)
        .assert()
        .success();

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let include_paths = document["options"]["includePaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/quoted include")));
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["QUOTED_DEFINE=1"])
    );
}
