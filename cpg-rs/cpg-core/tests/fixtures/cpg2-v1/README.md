These eight files were written by the accepted eleventh `cpg` binary at commit `c3a8c67e01354872b767b857c98252169bd5cca0`. They contain the actual CPG2 version 1 payload, before the modifier-type column and Binding/Binds storage extension.

`manifest.json` binds every copied graph, source input and persistence transcript. `accepted-run.json` preserves the complete original capture receipt, including its absolute provenance paths and baseline differences. Those paths are historical evidence; the test reads only relative fixture files. No executable is bundled or needed.

The backward-reader test checks that every old payload byte survives loading and version 2 serialization after removing only the newly added all-absent modifier column. It also checks a second reopen. These fixtures establish storage compatibility, not Joern identity parity; seven of their original canonical graphs remain nonexact.
