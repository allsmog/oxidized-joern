//! The language-independent CPG schema.
//!
//! Every frontend, regardless of source language, maps onto this fixed set of
//! node and edge kinds. Keeping the schema closed (an enum rather than open
//! string labels) is what lets the storage layer stay columnar and lets shared
//! passes reason about any language's graph uniformly — the single most
//! important consolidation lever versus per-frontend node vocabularies.

/// Kinds of CPG node. A deliberately small, language-independent vocabulary.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum NodeKind {
    /// A source file. Root of a file's AST subtree; the unit of incrementality.
    File,
    /// A named scope (package / namespace / module).
    Namespace,
    /// A type/class/struct declaration.
    TypeDecl,
    /// A field/member of a type.
    Member,
    /// A function or method.
    Method,
    /// A formal input parameter.
    MethodParameterIn,
    /// The (synthetic) return slot of a method.
    MethodReturn,
    /// A lexical block `{ ... }`.
    Block,
    /// A call site (function call, operator, etc.).
    Call,
    /// A use of an identifier (variable reference).
    Identifier,
    /// A literal constant.
    Literal,
    /// A local variable declaration.
    Local,
    /// A field access selector (the `.x` in `a.x`).
    FieldIdentifier,
    /// A control structure (if/while/for/switch ...).
    ControlStructure,
    /// A return statement.
    Return,
    /// A reference to a method (function pointer / first-class function).
    MethodRef,
    /// Anything a frontend could not classify; keeps the graph total.
    Unknown,
}

impl NodeKind {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(b: u8) -> Option<NodeKind> {
        use NodeKind::*;
        Some(match b {
            0 => File,
            1 => Namespace,
            2 => TypeDecl,
            3 => Member,
            4 => Method,
            5 => MethodParameterIn,
            6 => MethodReturn,
            7 => Block,
            8 => Call,
            9 => Identifier,
            10 => Literal,
            11 => Local,
            12 => FieldIdentifier,
            13 => ControlStructure,
            14 => Return,
            15 => MethodRef,
            16 => Unknown,
            _ => return None,
        })
    }
}

/// Kinds of directed edge between nodes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum EdgeKind {
    /// Abstract syntax tree containment (parent -> child).
    Ast,
    /// Intra-procedural control flow (node -> successor).
    Cfg,
    /// Call site -> resolved method.
    Call,
    /// Identifier/argument -> its declaration (param/local).
    Ref,
    /// Data dependence (def -> use), produced by dataflow passes.
    Ddg,
    /// Call -> argument expression.
    Argument,
    /// Call -> receiver/base expression.
    Receiver,
    /// Structural containment (file -> method, method -> block, ...).
    Contains,
    /// Reaching-definition edge (def -> use), the Joern REACHING_DEF
    /// equivalent produced by the reaching-def pass over the CFG. Kept
    /// distinct from [`EdgeKind::Ddg`] so coarser data-dependence layers can
    /// coexist with the precise gen/kill result.
    ReachingDef,
}

impl EdgeKind {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(b: u8) -> Option<EdgeKind> {
        EdgeKind::ALL.get(b as usize).copied()
    }

    pub const ALL: [EdgeKind; 9] = [
        EdgeKind::Ast,
        EdgeKind::Cfg,
        EdgeKind::Call,
        EdgeKind::Ref,
        EdgeKind::Ddg,
        EdgeKind::Argument,
        EdgeKind::Receiver,
        EdgeKind::Contains,
        EdgeKind::ReachingDef,
    ];
}

/// Analysis layers, used by the pass framework to track read/write
/// dependencies. A pass declares which layers it consumes and produces so the
/// scheduler can (a) order passes and (b) re-run only the layers invalidated by
/// a file change — the foundation of incremental analysis.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Layer {
    /// Raw AST + structural edges, produced directly by a frontend.
    Ast,
    /// Resolved symbol references (Ref edges).
    SymbolRef,
    /// Resolved call targets (Call edges).
    CallGraph,
    /// Control flow (Cfg edges).
    Cfg,
    /// Data dependence (Ddg edges).
    Ddg,
    /// Per-method dataflow summaries.
    Summaries,
}
