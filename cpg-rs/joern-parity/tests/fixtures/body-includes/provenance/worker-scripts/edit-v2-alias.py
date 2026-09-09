from pathlib import Path
p=Path('cpg-rs/cpg-lang-c/src/exact.rs');s=p.read_text()
old='''                    let full = if count == 0 {
                        name.clone()
                    } else {
                        format!("{name}<duplicate>{}", count - 1)
                    };
                    self.type_alias_full_names.insert('''
new='''                    let next = if count == 0 {
                        name.clone()
                    } else {
                        format!("{name}<duplicate>{}", count - 1)
                    };
                    // The project registry reserves standalone header aliases
                    // before body emission. An earlier include occurrence uses
                    // that first identity; advance only the still-unemitted
                    // reservation for this exact physical header declarator.
                    let reserved = (self.source_view > 0).then(|| {
                        (self.body_macro_sites.headers[self.source_view - 1].root.id(), 0, alias.id())
                    }).and_then(|key| {
                        let full = self.type_alias_full_names.get(&key)?.clone();
                        (!self.placements.contains_key(&format!("TD:{full}")))
                            .then_some((key, full))
                    });
                    let full = if let Some((key, full)) = reserved {
                        if let Some(entry) = self.type_declarations.iter_mut().find(|entry| entry.0 == full) {
                            entry.0 = next.clone();
                        }
                        self.type_alias_full_names.insert(key, next);
                        full
                    } else {
                        next
                    };
                    self.type_alias_full_names.insert('''
assert old in s;s=s.replace(old,new,1)
old='''                        let name = type_binding_name(alias, b);
                        self.line(
                            2,
                            "TYPE_DECL",
                            P {
                                name: Some(name.clone()),
                                code: Some(esc(text(n, b))),
                                full: Some(name),'''
new='''                        let name = type_binding_name(alias, b);
                        let full = self.type_alias_full_names
                            .get(&(self.body_macro_sites.tree, 0, alias.id()))
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        self.line(
                            2,
                            "TYPE_DECL",
                            P {
                                name: Some(name.clone()),
                                code: Some(esc(text(n, b))),
                                full: Some(full),'''
assert old in s;s=s.replace(old,new,1)
p.write_text(s)
