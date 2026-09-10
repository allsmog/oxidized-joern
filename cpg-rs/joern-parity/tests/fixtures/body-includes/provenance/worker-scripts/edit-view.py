from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/exact.rs');s=p.read_text()
s=s.replace('type TypeSites = HashMap<usize, TypeNameState>;','type TypeSites = HashMap<(usize, usize), TypeNameState>;')
s=s.replace('struct BodyMacroSites {', "struct BodyMacroSites<'tree> {")
s=s.replace('    definitions: Vec<MacroMetadata>,\n    expansions: Vec<(usize, usize)>,','    definitions: Vec<MacroMetadata>,\n    expansions: Vec<(usize, usize)>,\n    includes: HashMap<(usize, usize), usize>,\n    headers: Vec<IncludedBodyView<\'tree>>,')
i=s.index('/// An object callee')
s=s[:i]+'''/// One executed include occurrence, borrowing the supplied original tree. A
/// nested occurrence is reached through its parent view plus include node ID.
struct IncludedBodyView<'tree> {
    root: Node<'tree>,
    bytes: &'tree [u8],
    file: &'tree str,
    items: Vec<Node<'tree>>,
    changes: Vec<(usize, MacroState)>,
}

'''+s[i:]
s=s.replace('impl BodyMacroSites {', "impl<'tree> BodyMacroSites<'tree> {")
s=s.replace('            expansions: Vec::new(),','            expansions: Vec::new(),\n            includes: HashMap::new(),\n            headers: Vec::new(),')
i=s.index('    fn expansion(&mut self,')
s=s[:i]+'''    fn bytes_for<'a>(&'a self, view: usize, caller: &'a [u8]) -> &'a [u8] {
        if view == 0 { caller } else { self.headers[view - 1].bytes }
    }

    fn owns_view_node(&self, view: usize, mut node: Node) -> bool {
        while let Some(parent) = node.parent() { node = parent; }
        node.id() == if view == 0 { self.tree } else { self.headers[view - 1].root.id() }
    }

    fn included(&self, view: usize, node: Node, bytes: &[u8]) -> Option<(usize, &IncludedBodyView<'tree>)> {
        if !self.owns_view_node(view, node) { return None; }
        let owned = if view == 0 { self.owns(bytes) } else {
            let source = self.headers[view - 1].bytes;
            source.as_ptr() == bytes.as_ptr() && source.len() == bytes.len()
        };
        if !owned { return None; }
        let target = *self.includes.get(&(view, node.id()))?;
        Some((target, &self.headers[target - 1]))
    }

    fn record_view(&mut self, view: usize, bytes: &[u8], offset: usize, macros: &MacroState) {
        if view == 0 {
            self.record(bytes, offset, macros);
        } else {
            let changes = &mut self.headers[view - 1].changes;
            if changes.last().is_none_or(|(_, prior)| !Arc::ptr_eq(prior, macros)) {
                changes.push((offset, macros.clone()));
            }
        }
    }

    fn at_view(&self, view: usize, node: Node, bytes: &[u8]) -> Option<&MacroState> {
        if view == 0 { return self.at(node, bytes); }
        let header = &self.headers[view - 1];
        if header.bytes.as_ptr() != bytes.as_ptr() || header.bytes.len() != bytes.len()
            || !self.owns_view_node(view, node) { return None; }
        let index = header.changes.partition_point(|(offset, _)| *offset <= node.start_byte());
        index.checked_sub(1).map(|index| &header.changes[index].1)
    }

'''+s[i:]
s=s.replace("body_macro_sites: &'a BodyMacroSites,", "body_macro_sites: &'a BodyMacroSites<'a>,\n    source_view: usize,")
s=s.replace('macro_use_ids: HashMap<(usize, String, usize, String), usize>,','macro_use_ids: HashMap<(usize, usize, String, usize, String), usize>,')
s=s.replace('recovery_candidates: HashMap<usize, bool>,','recovery_candidates: HashMap<(usize, usize), bool>,')
s=s.replace('            body_macro_sites,\n            macro_uses:', '            body_macro_sites,\n            source_view: 0,\n            macro_uses:')
s=s.replace('        body_macro_sites: &empty_body_macro_sites,\n        macro_uses:', '        body_macro_sites: &empty_body_macro_sites,\n        source_view: 0,\n        macro_uses:')
s=s.replace('self.body_macro_sites.at(node, bytes)', 'self.body_macro_sites.at_view(self.source_view, node, bytes)')
s=s.replace('self.body_macro_sites.owns_node(site)', 'self.body_macro_sites.owns_view_node(self.source_view, site)')
s=s.replace('let key = (site.id(), name.to_string(), params.len(), ret.clone());','let key = (self.source_view, site.id(), name.to_string(), params.len(), ret.clone());')
s=s.replace('self.recovery_candidates.get(&node.id())','self.recovery_candidates.get(&(self.source_view, node.id()))')
s=s.replace('self.recovery_candidates.insert(node.id(), recovery);','self.recovery_candidates.insert((self.source_view, node.id()), recovery);')
s=s.replace('            .get(&node.id())\n            .cloned()\n            .unwrap_or_else(|| self.default_typedefs.clone())','            .get(&(self.source_view, node.id()))\n            .cloned()\n            .unwrap_or_else(|| self.default_typedefs.clone())')
# Collector views retain original source lifetimes.
s=s.replace('    body_macro_sites: BodyMacroSites,','    body_macro_sites: BodyMacroSites<\'tree>,')
s=s.replace("    root: Node<'tree>,\n    bytes: &[u8],\n    file: &str,\n    units: &[SourceUnit],\n) -> BodyMacroContext<'tree>","    root: Node<'tree>,\n    bytes: &'tree [u8],\n    file: &'tree str,\n    units: &'tree [SourceUnit],\n) -> BodyMacroContext<'tree>")
start=s.index('    #[allow(clippy::too_many_arguments)]\n    fn collect',s.index('fn body_macro_context'))
end=s.index('\nfn normalize_source_path',start)
part=s[start:end]
part=part.replace("        bytes: &[u8],\n        file: &str,\n        units: &[SourceUnit],", "        bytes: &'tree [u8],\n        file: &'tree str,\n        units: &'tree [SourceUnit],")
part=part.replace('        body_sites: &mut BodyMacroSites,\n        in_expansion: bool,','        body_sites: &mut BodyMacroSites<\'tree>,\n        in_expansion: bool,\n        source_view: usize,\n        header_top: bool,')
part=part.replace('    ) {\n        match node.kind()', '    ) {\n        if source_view != 0 { body_sites.record_view(source_view, bytes, node.start_byte(), macros); }\n        match node.kind()',1)
# Default recursive calls preserve view; named-child descent ends header-root status.
part=part.replace('                                in_expansion,\n                            );','                                in_expansion,\n                                source_view,\n                                header_top,\n                            );')
part=part.replace('                            in_expansion,\n                        );','                            in_expansion,\n                            source_view,\n                            header_top,\n                        );')
part=part.replace('                            in_expansion || outer_expansion,\n                        );','                            in_expansion || outer_expansion,\n                            source_view,\n                            false,\n                        );')
part=part.replace('            _ => {\n                let outer_expansion', '''            _ => {
                if source_view != 0 && header_top && matches!(node.kind(),
                    "declaration" | "type_definition" | "struct_specifier" | "union_specifier" | "enum_specifier" | "preproc_include") {
                    body_sites.headers[source_view - 1].items.push(node);
                }
                let outer_expansion''',1)
needle='''                            let mut header_items = Vec::new();
                            let mut header_states = HashMap::new();'''
part=part.replace(needle,needle+'''
                            let header_view = if in_body {
                                body_sites.headers.push(IncludedBodyView {
                                    root: unit.tree.root_node(), bytes: unit.src.as_bytes(), file: &unit.file,
                                    items: Vec::new(), changes: vec![(0, macros.clone())],
                                });
                                let view = body_sites.headers.len();
                                body_sites.includes.insert((source_view, node.id()), view);
                                view
                            } else { source_view };''')
# Specific included-unit call has greater indentation than other recursive calls.
part=part.replace('                                    in_expansion,\n                                );','                                    in_expansion,\n                                    header_view,\n                                    in_body,\n                                );')
part=part.replace('body_sites.record(bytes, node.end_byte(), macros);','body_sites.record_view(source_view, bytes, node.end_byte(), macros);')
part=part.replace('            &mut body_macro_sites,\n            false,\n        );','            &mut body_macro_sites,\n            false,\n            0,\n            false,\n        );')
s=s[:start]+part+s[end:]
# File-owned imports count every syntactic caller include, independently of activity.
old='''            let n = translation_unit_items(u.tree.root_node(), u.src.as_bytes())
                .iter()
                .filter(|f| f.kind() == "preproc_include")
                .count();'''
new='''            let mut pending = vec![u.tree.root_node()];
            let mut n = 0;
            while let Some(node) = pending.pop() {
                if node.kind() == "preproc_include" { n += 1; }
                pending.extend(translation_unit_children(node, u.src.as_bytes()));
            }'''
assert old in s;s=s.replace(old,new)
p.write_text(s)
