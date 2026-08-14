//! The language contract.
//!
//! This is the abstraction Fraunhofer's CPG got right and Joern's `x2cpg` only
//! partially captures: a frontend is split into (1) a declarative [`Language`]
//! description — a name, a namespace delimiter, and a set of capability
//! [`LanguageTraits`] — and (2) a [`Frontend`] that maps parsed source onto the
//! shared builder primitives. Shared passes (symbol/call resolution, type
//! hierarchy) read the *traits*, not the language identity, so their logic is
//! written once and parameterised per language instead of re-implemented in
//! every frontend.

use cpg_core::{Cpg, FileId};

/// A tiny dependency-free `bitflags` so the workspace core stays std-only.
#[macro_export]
macro_rules! bitflags_lite {
    (
        $(#[$meta:meta])*
        pub struct $name:ident: $ty:ty {
            $($(#[$fmeta:meta])* const $flag:ident = $value:expr;)*
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
        pub struct $name { bits: $ty }
        impl $name {
            $($(#[$fmeta])* pub const $flag: $name = $name { bits: $value };)*
            pub const EMPTY: $name = $name { bits: 0 };
            pub fn contains(self, other: $name) -> bool {
                (self.bits & other.bits) == other.bits
            }
            pub fn bits(self) -> $ty { self.bits }
        }
        impl core::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name { $name { bits: self.bits | rhs.bits } }
        }
    };
}

bitflags_lite! {
    /// Capabilities a language has, consulted by shared passes to adapt their
    /// behaviour without hard-coding language names.
    pub struct LanguageTraits: u32 {
        const HAS_CLASSES           = 1 << 0;
        const HAS_GENERICS          = 1 << 1;
        const HAS_FUNCTION_POINTERS = 1 << 2;
        const HAS_DEFAULT_ARGS      = 1 << 3;
        /// Symbols can be used before declaration (hoisting / forward refs).
        const ALLOWS_FORWARD_REFS   = 1 << 4;
        /// Overloading: a name may resolve to several methods by signature.
        const HAS_OVERLOADING       = 1 << 5;
        /// Structural (duck) typing rather than nominal.
        const STRUCTURAL_TYPING     = 1 << 6;
    }
}

/// Declarative description of a programming language.
pub trait Language {
    fn name(&self) -> &'static str;
    /// e.g. `::` for C++, `.` for Java/Python.
    fn namespace_delimiter(&self) -> &'static str;
    fn traits(&self) -> LanguageTraits;
    /// File extensions (without the dot) this language claims.
    fn file_extensions(&self) -> &'static [&'static str];

    fn has(&self, t: LanguageTraits) -> bool {
        self.traits().contains(t)
    }
}

/// Outcome of parsing+building a single file.
pub struct BuildResult {
    pub file: FileId,
    pub methods_built: usize,
}

/// A frontend translates source text into CPG nodes for one file at a time.
///
/// Per-*file* granularity (not per-project) is deliberate: it is what makes the
/// frontend usable by the incremental driver, which rebuilds exactly the files
/// that changed.
pub trait Frontend {
    fn language(&self) -> &dyn Language;

    /// Build a whole-project graph when semantics require project-wide
    /// registries (for example C globals, preprocessor methods, and external
    /// operator stubs). The default keeps the parallel per-file path. A
    /// frontend returning `Some` owns only AST/schema construction; the driver
    /// still runs the shared production pass pipeline afterward.
    fn build_project(&mut self, _files: &[(&str, &str)]) -> Option<Cpg> {
        None
    }

    /// Parse `source` for `path` and emit its subgraph into `cpg`. The frontend
    /// must attribute every node it creates to the returned file's `FileId`
    /// (the builder enforces this) so the driver can later delete and rebuild
    /// just this file.
    fn build_file(&mut self, cpg: &mut Cpg, path: &str, source: &str) -> BuildResult;
}
