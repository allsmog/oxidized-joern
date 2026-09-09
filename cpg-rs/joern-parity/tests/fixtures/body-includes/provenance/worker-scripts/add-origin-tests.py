from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/import.rs');s=p.read_text()
needle='''    #[test]
    fn source_lines_keep_lua_sweep2old_in_its_own_method()'''
new='''    #[test]
    fn included_initializer_uses_header_lines_without_consuming_caller_calls() {
        use cpg_frontend::Frontend;
        // Full pinned include/inline observations separately retain columns;
        // the shared Cpg currently exposes line coordinates only.
        for included in [true, false] {
            let main = if included {
                "int consume(int value);\\nint probe(void) {\\n#include \\\"api.h\\\"\\n    consume(7);\\n    return included;\\n}\\n"
            } else {
                "int consume(int value);\\nint probe(void) {\\n    int included = consume(7);\\n    consume(7);\\n    return included;\\n}\\n"
            };
            let mut sources = vec![("locations.c", main)];
            if included {
                sources.push(("api.h", "int included = consume(7);\\n"));
            }
            let cpg = crate::CFrontend::new().build_project(&sources).unwrap();
            let nodes = method_nodes(&cpg, "probe");
            let initializer_line = if included { 1 } else { 3 };
            let local = nodes.iter().copied().find(|&node| {
                cpg.kind_of(node) == NodeKind::Local && cpg.name_of(node) == Some("included")
            }).unwrap();
            assert_eq!(cpg.line_of(local), Some(initializer_line));
            assert_eq!(cpg.type_full_name_of(local), Some("int"));
            let mut call_lines: Vec<_> = nodes.iter().copied().filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.name_of(node) == Some("consume")
            }).map(|node| cpg.line_of(node).unwrap()).collect();
            call_lines.sort_unstable();
            assert_eq!(call_lines, [initializer_line, 4]);
            let ret = nodes.iter().copied().find(|&node| cpg.kind_of(node) == NodeKind::Return).unwrap();
            assert_eq!(cpg.line_of(ret), Some(5));
            for node in nodes.iter().copied().filter(|&node| {
                matches!(cpg.kind_of(node), NodeKind::Local | NodeKind::Call | NodeKind::Identifier | NodeKind::Literal)
            }) {
                assert_eq!(cpg.out_kind(node, EdgeKind::SourceFile).count(), 0);
            }
        }
    }

    #[test]
    fn source_lines_keep_lua_sweep2old_in_its_own_method()'''
assert needle in s;s=s.replace(needle,new,1);p.write_text(s)
