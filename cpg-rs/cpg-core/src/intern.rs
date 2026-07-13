//! String interning.
//!
//! Type names, method full-names and `code` strings repeat heavily across a
//! large code base (every `int` parameter carries the same `type_full_name`).
//! Interning collapses those to a single `u32` per distinct string, which is
//! the dominant memory win when a graph holds hundreds of millions of property
//! values. Resolved strings are immutable for the lifetime of the graph, so a
//! `Sym` is a stable handle.

use std::collections::HashMap;

/// Handle to an interned string. Cheap to copy, store columnar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Sym(pub u32);

#[derive(Default)]
pub struct Interner {
    by_text: HashMap<Box<str>, Sym>,
    texts: Vec<Box<str>>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> Sym {
        if let Some(&sym) = self.by_text.get(s) {
            return sym;
        }
        let sym = Sym(self.texts.len() as u32);
        let boxed: Box<str> = s.into();
        self.texts.push(boxed.clone());
        self.by_text.insert(boxed, sym);
        sym
    }

    pub fn resolve(&self, sym: Sym) -> &str {
        &self.texts[sym.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.texts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedups_equal_strings() {
        let mut i = Interner::new();
        let a = i.intern("int");
        let b = i.intern("int");
        let c = i.intern("char");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(i.len(), 2);
        assert_eq!(i.resolve(a), "int");
    }
}
