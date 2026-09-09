from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/exact.rs');s=p.read_text()
s=s.replace('''        if !self.owns_view_node(view, node) {
            return None;
        }
        let owned''','''        let target = *self.includes.get(&(view, node.id()))?;
        if !self.owns_view_node(view, node) {
            return None;
        }
        let owned''',1)
s=s.replace('''        let target = *self.includes.get(&(view, node.id()))?;
        Some((target, &self.headers[target - 1]))''','''        Some((target, &self.headers[target - 1]))''',1)
s=s.replace('object_decl_suffix(declarator, source.as_bytes(), false)','object_decl_suffix(declarator, source.as_bytes(), false, &MacroState::default())')
old='''            self.macro_method_files
                .entry(full.clone())
                .or_insert_with(|| self.file.clone());
            self.used_macros.entry(full.clone()).or_insert_with(|| {
                (
                    metadata.name.clone(),
                    metadata.directive.clone(),
                    use_.arity,
                    use_.ret.clone(),
                )
            });'''
new='''            self.register_macro_method(
                &full,
                &metadata.name,
                &metadata.directive,
                use_.arity,
                &use_.ret,
            );'''
assert old in s;s=s.replace(old,new,1)
old='''            self.macro_method_files
                .entry(full.clone())
                .or_insert_with(|| self.file.clone());
            self.used_macros.entry(full.clone()).or_insert((
                name.to_string(),
                directive.clone(),
                params.len(),
                ret.clone(),
            ));'''
assert old in s;s=s.replace(old,'''            self.register_macro_method(&full, name, &directive, params.len(), &ret);''',1)
needle='''impl Ctx<'_> {
    fn resolve_macro_metadata'''
helper='''impl Ctx<'_> {
    /// C2Cpg merges the source-pass declarations into the later .h pass.
    /// Select the complete MethodInfo together: a generated header-pass entry
    /// wins over a source-pass entry, and each class keeps its first entry.
    fn register_macro_method(
        &mut self,
        full: &str,
        name: &str,
        directive: &str,
        arity: usize,
        ret: &str,
    ) {
        let header_pass = |file: &str| {
            file.get(file.len().saturating_sub(2)..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".h"))
        };
        let replace = self.macro_method_files.get(full).is_none_or(|prior| {
            header_pass(&self.file) && !header_pass(prior)
        });
        if replace {
            self.macro_method_files.insert(full.to_string(), self.file.clone());
            self.used_macros.insert(
                full.to_string(),
                (name.to_string(), directive.to_string(), arity, ret.to_string()),
            );
        }
    }

    fn resolve_macro_metadata'''
assert needle in s;s=s.replace(needle,helper,1)
p.write_text(s)
