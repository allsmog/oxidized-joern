# Preserved work integration — 2026-09-09

Repository: `allsmog/oxidized-joern`. Integration PR: #82.

The integration combines master `b879cb8b8` with the latest preserved C parity
head `733e1974e`, then reconciles the overlapping feature branches listed below.
Earlier implementations are covered by the latest source-aware frontend and
its retained regression suites; their older whole-file snapshots must not
replace later accepted fixes. All portable fixture paths are retained. The
planning snapshot adds eleven previously uncommitted documents; their audit
statuses are explicitly historical.

The C corpus is the union of both lines, including `macro_advanced.c`,
`pointers.c`, and `preprocessor.c`. Fresh Joern v4.0.555 output matches all
334 comparison blocks: 6,022 nonblank AST records, 579 scaffolding records,
10,752 structural edges, and 4,188 reaching-definition facts. The reference
was generated independently by `oracle.sc`, then compared before replacing
`oracle_all.txt`; no native output was used to generate the reference.

Integration resolutions:

- CPG2 v4 retains native v1 tags and sparse properties, parity v2 modifier
  columns, and parity v3 include tags/properties. Historical v3 Import/Imports
  tags are decoded explicitly; they differ from native master tags.
- Explicit compiler inputs select active top-level declarations. Default C
  parsing keeps CDT's inactive declaration headers with empty bodies.
- Macro substitution supports stringification, token pasting and variadic
  arguments while preserving literal and Unicode tokens. Function-like
  preprocessor conditions share the source-position macro environment.
- Native scanner dependencies exclude conservative unknown-call sibling
  argument links; stored Joern graph facts retain their measured semantics.
  Sanitizer-aware copy-length and all labeled scanner controls remain gated.
- The dominance pass now stores immediate parents instead of all dominator
  sets, avoiding quadratic storage on the larger C graphs. Cyclic CFGs are
  compared with the prior set algorithm; a 10,000-node chain guards scale.
- Native positional parameter queries omit index-zero implicit parameters;
  raw AST and CPGQL traversals retain the Joern schema nodes.
- CPGQL, Flatgraph, language gates, expanded scanner rules, dominance and
  compiler inputs from master remain part of the required release contract.

## Preserved branch coverage

| PR | Branch | Preserved tip |
|---|---|---|
| #55 | `codex/astra-parity-sprint` | `733e1974e3bc95fac46394b746f2a4c73adb7234` |
| #56 | `codex/closeout-plans-20260909` | `e14b0627e6a43c0b69bcdc859597610685787df7` |
| #57 | `codex/astra-body-includes` | `b45160887e7aa6037eb06c9e7fd4bf18846f9908` |
| #58 | `codex/astra-body-macro-state` | `85777144fe79f2c4a5afb93bf06c1cac5a977d8d` |
| #59 | `codex/astra-body-preprocessor` | `de07c63b37654303a858d779697b82183243b313` |
| #60 | `codex/astra-capture-memory` | `21d36fc9fba5d59730c312c42612fecd253bdbf1` |
| #61 | `codex/astra-control-parity` | `5b5ab394926645783c4ed38f4e3af8934d8bd857` |
| #62 | `codex/astra-declaration-macros` | `0ccc8f18a2ee1e179bb558f0c51b7fb38f1701cf` |
| #63 | `codex/astra-field-macros` | `b2c417be031091b97e040b2c3a452875ab7ae408` |
| #64 | `codex/astra-c-format-positions` | `583da1524416d4b9f454627bba1f7dd7d52dd5f7` |
| #65 | `codex/astra-fourth-arrays-combined-probe` | `a7ccd8b0bdd37773fb02af26f5b10cf44ccc5312` |
| #66 | `codex/astra-function-identities` | `7ee03c0e4e87931b1237feb36833611bc1d69b03` |
| #67 | `codex/astra-fullname-spaces` | `bdf2f687b1f24e3ed064cdea0d2200cc8bd9c613` |
| #68 | `codex/astra-include-references` | `41cb8b32f28f53d0b862d571f8c779759a879f52` |
| #69 | `codex/astra-macro-arguments` | `96de09c093748f80667589fac0ac48e6dfe3257d` |
| #70 | `codex/astra-macro-bindings` | `47b37e24ac097fffc4dda40b152295a6952aeaed` |
| #71 | `codex/astra-macro-cast-context` | `0041ab1cde11c49f8edeed46c341391fd2d927c0` |
| #72 | `codex/astra-macro-locations` | `0044e4f14922d9c595f42fda66a4fa32dc7dbc70` |
| #73 | `codex/astra-macro-regression-tests` | `2d4521095fb51aa71b75ebadd463c1d3a191920b` |
| #74 | `codex/astra-nested-cast-parity` | `9ae10666ff0f4c75a59bed1643ab48c0941e76ba` |
| #75 | `codex/astra-declaration-repair` | `d767fde1249cf9fa2b0187557d14f2ff7f120e48` |
| #76 | `codex/astra-primitive-members` | `acafe5f6ac0608bbecbee43ec7d38b487ce3d676` |
| #77 | `codex/astra-statement-macros` | `d1feced9ea44d42f68fb60538a06b2b121faa041` |
| #78 | `codex/astra-stream-export` | `939c26eb2d48e98d928eb39097b2f65ed58de50f` |
| #79 | `codex/astra-typedef-aggregates` | `54227455aef920e037712210d13b6f7865b383e5` |
| #80 | `codex/astra-typedef-existence` | `8c3b4d4968de92a775752fd82027ec136395d3ab` |
| #81 | `codex/closeout-inactive-declaration-stash-20260909` | `0de55fb7b8782f8de1e6095a073632bc15fb22ab` |

PRs #3 and #4 have no patch-unique commits relative to the integration history
(`git cherry`); their work was already incorporated before this consolidation.
Dependency bot updates are separate from the preserved local work.

Historical local Git history, original branches, binary diffs, stash, and ignored
working artifacts were preserved before checkout consolidation under
`/Users/shayaunnejad/worktree-archive/2026-09-09-joern/`. The archive and remote
branches preserve the original experiments as well as the integrated result.

## Real-project baseline reconciliation

The two C inputs still pass repeated build/save/load/export/scan determinism
and clean-versus-incremental equivalence. The integrated graphs contain 70,607
zlib nodes and 121,857 Lua nodes. Added macro expansion behavior and metadata,
native dominance edges, and CPG2 v4 change their hashes. Zlib's RSS ceiling is
640 MiB (formerly 512), allowing the measured 518.9 MiB combined workflow;
its 20-second build ceiling is unchanged. Lua keeps 30 seconds / 1536 MiB;
measured peak RSS is 734.8 MiB.

Lua now yields three policy findings with the 17-rule catalogue: environment
input to `fopen` through `handle_luainit`, `tmpnam` in `os_tmpname`, and `sprintf`
in `PrintConstant`. Each corresponds to retained source and the existing rule
policy. These are scanner regression expectations, not verified vulnerability
claims. Zlib remains at zero findings. The 16 non-C projects change only their
CPG2 binary hashes: nodes, structural edges, JSON, queries and SARIF are unchanged.

The native-source CPGQL fixture now has 119 nodes, including the added metadata.
Its updated digest is independently gated by the complete live CPGQL run:
108 source results, 37 populated-schema results and 18 error classifications.
