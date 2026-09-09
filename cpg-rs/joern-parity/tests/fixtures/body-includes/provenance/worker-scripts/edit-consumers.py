from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/exact.rs');s=p.read_text()
# Type state uses a per-occurrence node key and splices included roots in the
# current lexical type scope, without treating a header as a compound block.
a=s.index('fn collect_type_sites(');z=s.index('\nimpl Ctx',a);part=s[a:z]
part=part.replace('    macro_sites: Option<&BodyMacroSites>,\n) -> TypeSites', '    macro_sites: Option<&BodyMacroSites>,\n    source_view: usize,\n) -> TypeSites')
part=part.replace('        macro_sites: Option<&BodyMacroSites>,\n    ) {','        macro_sites: Option<&BodyMacroSites>,\n        source_view: usize,\n    ) {')
part=part.replace('sites.at(node, bytes)','sites.at_view(source_view, node, bytes)')
part=part.replace('sites.insert(node.id(), types.clone());','sites.insert((source_view, node.id()), types.clone());')
part=part.replace('sites, macros, macro_sites);','sites, macros, macro_sites, source_view);')
part=part.replace('        match node.kind() {','''        if let Some((view, header)) = macro_sites.and_then(|sites| sites.included(source_view, node, bytes)) {
            for item in &header.items {
                visit(*item, header.bytes, types, sites, macros, macro_sites, view);
            }
            return;
        }
        match node.kind() {''',1)
part=part.replace('        macro_sites,\n    );','        macro_sites,\n        source_view,\n    );')
s=s[:a]+part+s[z:]
s=s.replace('                Some(body_macro_sites),\n            ));','                Some(body_macro_sites),\n                0,\n            ));')
s=s.replace('collect_type_sites(root, bytes, types.clone(), &self.macros, None)','collect_type_sites(root, bytes, types.clone(), &self.macros, None, self.source_view)')
# Declaration-name discovery consumes the same include view.
a=s.index('fn collect_decl_names(');z=s.index('// --- tree helpers ---',a);part=s[a:z]
part=part.replace('    macro_sites: Option<&BodyMacroSites>,\n    out:', '    macro_sites: Option<&BodyMacroSites>,\n    source_view: usize,\n    out:')
part=part.replace('sites.at(n, b)', 'sites.at_view(source_view, n, b)')
part=part.replace('    if matches!(\n        n.kind(),', '''    if let Some((view, header)) = macro_sites.and_then(|sites| sites.included(source_view, n, b)) {
        for item in &header.items {
            collect_decl_names(*item, header.bytes, macros, macro_sites, view, out);
        }
        return;
    }
    if matches!(
        n.kind(),''',1)
part=part.replace('macros, macro_sites, out)', 'macros, macro_sites, source_view, out)')
s=s[:a]+part+s[z:]
s=s.replace('            Some(self.body_macro_sites),\n            &mut shadowed,', '            Some(self.body_macro_sites),\n            self.source_view,\n            &mut shadowed,')
s=s.replace('                        None,\n                        &mut expansion_shadowed,', '                        None,\n                        self.source_view,\n                        &mut expansion_shadowed,')
# Prototype discovery returns source/view identity, not a bare header Node read
# against the caller byte buffer.
a=s.index("fn prototype_declarations<'a>(");z=s.index('/// (name, return type',a)
s=s[:a]+'''fn prototype_declarations<'a>(
    root: Node<'a>,
    b: &'a [u8],
    macro_sites: &BodyMacroSites<'a>,
    source_view: usize,
) -> Vec<(Node<'a>, usize)> {
    if let Some((view, header)) = macro_sites.included(source_view, root, b) {
        return header.items.iter().flat_map(|node| prototype_declarations(*node, header.bytes, macro_sites, view)).collect();
    }
    if !prototype_headers(root, b).is_empty() {
        return vec![(root, source_view)];
    }
    // File-scope inactive declarations remain retained; active body prototypes
    // use the recorded original-source macro state.
    let children = if matches!(root.kind(), "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef" | "preproc_else") {
        macro_sites.at_view(source_view, root, b)
            .map(|macros| kept_preproc_children(root, b, macros))
            .unwrap_or_else(|| named_children(root))
    } else { named_children(root) };
    children.into_iter().flat_map(|child| prototype_declarations(child, b, macro_sites, source_view)).collect()
}

'''+s[z:]
s=s.replace('prototype_declarations(node, bytes, &context.body_macro_sites)', 'prototype_declarations(node, bytes, &context.body_macro_sites, 0)')
s=s.replace('        for declaration in declarations {\n            let macros = context', '        for (declaration, source_view) in declarations {\n            let bytes = context.body_macro_sites.bytes_for(source_view, bytes);\n            let macros = context')
s=s.replace('context.body_macro_sites.at(declaration, bytes)', 'context.body_macro_sites.at_view(source_view, declaration, bytes)')
# Macro ownership and state switch, with no C scope boundary.
needle='''        let previous = self
            .source_macros_at(n, b)'''
insert='''        let views = self.body_macro_sites;
        if let Some((view, header)) = views.included(self.source_view, n, b) {
            let previous_view = std::mem::replace(&mut self.source_view, view);
            for item in &header.items {
                self.emit_stmt(*item, header.bytes, order, depth);
            }
            self.source_view = previous_view;
            return;
        }
        let previous = self
            .source_macros_at(n, b)'''
a=s.index('    fn emit_stmt(&mut self,');z=s.index('    fn emit_stmt_in_context',a);part=s[a:z];assert needle in part;part=part.replace(needle,insert,1);s=s[:a]+part+s[z:]
needle='''        while let Some(n) = stack.pop() {
            if let Some(macros) = self.source_macros_at(n, b).cloned()'''
insert='''        while let Some(n) = stack.pop() {
            let views = self.body_macro_sites;
            if let Some((view, header)) = views.included(self.source_view, n, b) {
                let previous_view = std::mem::replace(&mut self.source_view, view);
                for item in &header.items {
                    self.walk_phantoms(*item, header.bytes, shadowed, seen);
                }
                self.source_view = previous_view;
                continue;
            }
            if let Some(macros) = self.source_macros_at(n, b).cloned()'''
assert needle in s;s=s.replace(needle,insert,1)
# Local typedef identity is occurrence-specific; physical filename is separate
# from invoking-TU ownership. Duplicate-name ordering remains to be measured.
s=s.replace('let mut type_alias_full_names: HashMap<usize, String>', 'let mut type_alias_full_names: HashMap<(usize, usize, usize), String>')
s=s.replace('type_alias_full_names.insert(alias.id(), tag.clone());','type_alias_full_names.insert((u.tree.root_node().id(), 0, alias.id()), tag.clone());')
s=s.replace("type_alias_full_names: &'a mut HashMap<usize, String>","type_alias_full_names: &'a mut HashMap<(usize, usize, usize), String>")
s=s.replace('self.type_alias_full_names.get(&alias.id())','self.type_alias_full_names.get(&(self.body_macro_sites.tree, self.source_view, alias.id()))')
s=s.replace('self.type_alias_full_names.insert(alias.id(), full.clone());','self.type_alias_full_names.insert((self.body_macro_sites.tree, self.source_view, alias.id()), full.clone());')
a=s.index('    fn emit_local_typedef(');z=s.index('    fn sizeof_identifier',a);part=s[a:z]
part=part.replace('                        self.file.clone(),','                        if self.source_view == 0 { self.file.clone() } else { self.body_macro_sites.headers[self.source_view - 1].file.to_string() },')
s=s[:a]+part+s[z:]
p.write_text(s)
