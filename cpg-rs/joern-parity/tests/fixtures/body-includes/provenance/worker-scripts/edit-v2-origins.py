from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/exact.rs');s=p.read_text()
s=s.replace('''pub fn canonical_dump_sources(sources: &[(String, String)]) -> String {
    let mut parser''','''pub fn canonical_dump_sources(sources: &[(String, String)]) -> String {
    canonical_dump_sources_with_origins(sources).0
}

/// Included declarations keep the caller AST ownership while their line
/// coordinates come from the original header span. This internal side channel
/// does not change the canonical projection or add graph file properties.
pub(crate) struct IncludedSourceOrigin {
    pub block: String,
    pub nodes: std::ops::Range<usize>,
    pub file: String,
    pub span: std::ops::Range<usize>,
}

pub(crate) fn canonical_dump_sources_with_origins(
    sources: &[(String, String)],
) -> (String, Vec<IncludedSourceOrigin>) {
    let mut included_origins = Vec::new();
    let mut parser''',1)
s=s.replace('''            source_view: 0,
            macro_uses:''','''            source_view: 0,
            included_origins: &mut included_origins,
            macro_uses:''',1)
s=s.replace('''        source_view: 0,
        macro_uses:''','''        source_view: 0,
        included_origins: &mut included_origins,
        macro_uses:''',1)
s=s.replace('''    out
}

fn count_members''','''    (out, included_origins)
}

fn count_members''',1)
s=s.replace('''    source_view: usize,
    macro_uses: Vec<MacroUse>,''','''    source_view: usize,
    included_origins: &'a mut Vec<IncludedSourceOrigin>,
    macro_uses: Vec<MacroUse>,''',1)
s=s.replace('''            for item in &header.items {
                self.emit_stmt(*item, header.bytes, order, depth);
            }
            self.source_view = previous_view;''','''            for item in &header.items {
                let start = self.line_no;
                self.emit_stmt(*item, header.bytes, order, depth);
                // A nested include records its own physical spans. Only direct
                // header declarations contribute new roots at this occurrence.
                if item.kind() != "preproc_include" && start < self.line_no {
                    self.included_origins.push(IncludedSourceOrigin {
                        block: self.block.clone(),
                        nodes: start..self.line_no,
                        file: header.file.to_string(),
                        span: item.byte_range(),
                    });
                }
            }
            self.source_view = previous_view;''',1)
p.write_text(s)
p=Path('cpg-rs/cpg-lang-c/src/lib.rs');s=p.read_text();old='''        let dump = exact::canonical_dump_sources(&sources);
        Some(import::graph_from_canonical_dump(&dump, &sources))''';assert old in s;s=s.replace(old,'''        let (dump, origins) = exact::canonical_dump_sources_with_origins(&sources);
        Some(import::graph_from_canonical_dump_with_origins(&dump, &sources, &origins))''',1);p.write_text(s)
p=Path('cpg-rs/cpg-lang-c/src/import.rs');s=p.read_text()
s=s.replace('''pub fn graph_from_canonical_dump(dump: &str, sources: &[(String, String)]) -> Cpg {
    let mut raw_nodes''','''pub fn graph_from_canonical_dump(dump: &str, sources: &[(String, String)]) -> Cpg {
    graph_from_canonical_dump_with_origins(dump, sources, &[])
}

pub(crate) fn graph_from_canonical_dump_with_origins(
    dump: &str,
    sources: &[(String, String)],
    origins: &[crate::exact::IncludedSourceOrigin],
) -> Cpg {
    let mut raw_nodes''',1)
old='''    // All raw properties and file ownership have now been copied into the graph.
    drop(raw_nodes);'''
new='''    // Resolve only declaration roots while the address table is live. The
    // descendant ranges remain in the graph, so no per-node origin map survives
    // into location matching. Duplicate method views resolve to the same IDs.
    let mut included_roots = HashMap::new();
    for origin in origins {
        let first = resolve_address(&address_to_raw, &format!("{}#{}", origin.block, origin.nodes.start));
        let depth = raw_nodes[first].depth;
        for ordinal in origin.nodes.clone() {
            let raw = resolve_address(&address_to_raw, &format!("{}#{ordinal}", origin.block));
            if raw_nodes[raw].depth == depth {
                included_roots.entry(raw_to_node[raw]).or_insert(origin);
            }
        }
    }

    // All raw properties and file ownership have now been copied into the graph.
    drop(raw_nodes);'''
assert old in s;s=s.replace(old,new,1)
s=s.replace('''    assign_source_lines(&mut cpg, sources);''','''    assign_source_lines(&mut cpg, sources, &included_roots);''',1)
s=s.replace('''fn assign_source_lines(cpg: &mut Cpg, sources: &[(String, String)]) {''','''fn assign_source_lines(
    cpg: &mut Cpg,
    sources: &[(String, String)],
    included_roots: &HashMap<NodeId, &crate::exact::IncludedSourceOrigin>,
) {
    let foreign_roots: HashSet<_> = included_roots.keys().copied().collect();''',1)
s=s.replace('''                tokens[range.start].line,
            );
        }
        // Global declarations''','''                tokens[range.start].line,
                &foreign_roots,
            );
        }
        // The header's physical tokens locate included declaration roots after
        // caller searches have skipped them. Header nodes keep their caller AST
        // owner and acquire no invented FILENAME or SOURCE_FILE properties.
        for (&root, origin) in included_roots {
            if origin.file != *path {
                continue;
            }
            let start = tokens.partition_point(|token| token.start < origin.span.start);
            let end = tokens.partition_point(|token| token.start < origin.span.end);
            if start < end {
                locate_ast(cpg, root, &tokens, start..end, tokens[start].line, &foreign_roots);
            }
        }
        // Global declarations''',1)
s=s.replace('''                    locate_ast(cpg, method, &global_tokens, 0..global_tokens.len(), 1);''','''                    locate_ast(cpg, method, &global_tokens, 0..global_tokens.len(), 1, &foreign_roots);''',1)
s=s.replace('''    inherited_line: u32,
) -> usize {''','''    inherited_line: u32,
    foreign_roots: &HashSet<NodeId>,
) -> usize {''',1)
s=s.replace('''    for child in children {
        if cpg.kind_of(child) == NodeKind::Method {
            continue;
        }
        // A normalized identifier''','''    for child in children {
        if cpg.kind_of(child) == NodeKind::Method || foreign_roots.contains(&child) {
            continue;
        }
        // A normalized identifier''',1)
s=s.replace('''            declaration_span.unwrap_or(next..child_range.end),
            line,
        );''','''            declaration_span.unwrap_or(next..child_range.end),
            line,
            foreign_roots,
        );''',1)
p.write_text(s)
