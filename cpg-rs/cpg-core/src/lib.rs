//! `cpg-core` — the language-independent code property graph.
//!
//! Columnar, file-partitioned, mutable storage (see [`graph`]); a closed
//! schema ([`schema`]); shared construction primitives ([`builder`]); and a
//! small query surface ([`traversal`]). Frontends, passes, and dataflow all
//! build on these and nothing else.

pub mod builder;
pub mod graph;
pub mod intern;
pub mod schema;
pub mod traversal;

pub use builder::CpgBuilder;
pub use graph::{Cpg, FileId, HalfEdge, NodeId};
pub use intern::Sym;
pub use schema::{EdgeKind, Layer, NodeKind};
pub use traversal::Query;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_remove_file_is_incremental() {
        let mut cpg = Cpg::new();
        let f = cpg.file_id("a.c");
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            let file = b.file_node("a.c");
            let m = b.method("main", "main", "int(void)", Some(1));
            b.contains(file, m);
            let call = b.call("puts", "puts(\"hi\")", Some(2));
            b.contains(m, call);
        }
        assert_eq!(cpg.methods().len(), 1);
        assert_eq!(cpg.calls().len(), 1);
        let before = cpg.live_count();

        // Removing the file's subgraph reclaims exactly those nodes.
        cpg.remove_file(f);
        assert_eq!(cpg.methods().len(), 0);
        assert_eq!(cpg.calls().len(), 0);
        assert!(cpg.live_count() < before);

        // Rebuilding reuses the tombstoned slots (free list), so a long-lived
        // process editing one file repeatedly does not leak node ids.
        {
            let mut b = CpgBuilder::new(&mut cpg, f);
            let _ = b.method("main", "main", "int(void)", Some(1));
        }
        assert_eq!(cpg.methods().len(), 1);
    }
}
