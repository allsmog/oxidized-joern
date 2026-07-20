# IRIS: a self-contained CPG security-scanning methodology

IRIS is a loop for finding taint-style vulnerabilities in an unfamiliar
codebase using only the `cpg` binary. Everything it needs is compiled in:
language frontends, the taint engine, the default rule packs, the named IRIS
packs (`cpg rules`), and this document. It has no dependency on any specific
repository, path, or build system — point it at a directory of source files.

The loop is: **build → infer a spec → scan entry-driven → certify coverage →
triage with kill mechanisms → widen (persistence, cross-service) → repeat.**

## 0. Vocabulary

- **Spec / rule pack** — a JSON document of rules: `sources` (calls returning
  attacker data), `sinks` (calls dangerous with attacker data, `name@argpos`
  to pin the dangerous argument, `@recv` for receiver position, `@out<k>` for
  out-parameter sources), `sanitizers` (neutralise taint), `entryMethods`,
  `sourceIdents` (identifiers tainted at every read, e.g. Flask's `request`),
  `authz` and `confiners` (advisory annotations, never suppression). See
  `cpg-cli/src/rules.rs` for the full schema; unknown keys are ignored so
  packs are forward-compatible.
- **Entry-driven scan** — instead of source calls, treat every parameter of
  selected methods as attacker-controlled: `--entry <name>`, `--entry-glob`,
  or automatically from IDL (`--rpc-sources <proto dir>`,
  `--thrift-sources <thrift dir>`).
- **Kill mechanism** — the recorded, reusable reason a finding class is a
  false positive (sanitizer on path, authz-dominated, confined placement,
  test-only caller, constant data). Triage produces kill mechanisms, not just
  verdicts, so the next scan of the same family starts ahead.

## 1. Build

```
cpg build <dir> -o app.cpg --lang <L> --exclude /vendor/ --exclude /test/ ...
```

- Exclude vendored, generated, and test code — they only add noise and build
  time. (The `x` front does this automatically per language.)
- Language is per-CPG; a polyglot service is several CPGs merged later.

## 2. Infer a spec

On a codebase you do not know, do not guess sinks — inventory them:

```
cpg apis <dir>|--load app.cpg [--min-count N]
```

This lists external APIs the code actually calls, ranked by use. From it (a
human or an LLM) pick: the execution/SQL/file/URL sinks that exist *here*,
the escaping helpers that are candidate sanitizers, and the authorization
call names. Write the pack; keep it small and distinctive — generic names
(`Get`, `run`) drown the report. Curated packs for codebase families this
methodology has been applied to ship in the binary: `cpg rules`,
`--rules iris:<name>`. They are examples to copy, not defaults.

## 3. Scan entry-driven

```
cpg scan --load app.cpg --lang <L> --rules spec.json \
    --rpc-sources <proto dir> --thrift-sources <idl dir> \
    [--entry <method>]... [-o out.sarif] [--flows-json flows.json]
```

- The trust boundary defines the entries: RPC handlers, HTTP controllers,
  message consumers. IDL directories are the cheapest honest source of them.
- Findings carry annotations rather than being suppressed: `authz-dominated@`
  / `authz-partial@`, `confined@line:name`, sanitizer hits, and guard info.
  Triage orders on annotations; only sanitizers actually remove flows.

## 4. Certify the zeros (coverage)

A scan that prints `0 findings` is only meaningful with evidence the scan
engaged. The coverage report (stderr) states: which `--entry` strings matched
nothing (typo detector), entries that matched but produced no findings, the
unresolved-call percentage, and spec names that matched no call in the graph.
A spec whose sink names never occur is a wrong spec, not a clean codebase.

## 5. Triage → kill mechanisms

For each finding: walk the reported path, decide, and record the *mechanism*:

- **True positive** — minimize to source line + sink line + missing guard.
- **False positive** — name the kill: which sanitizer/authz/confiner/shape
  argument kills the whole class, then encode it back into the pack
  (`sanitizers`, `authz`, `confiners`, tighter `@argpos`) so it never
  resurfaces. Ad-hoc dismissals are forbidden; packs are the memory.

Deep-dive tools for a single finding: `cpg slice --call <sink>` (backward
slice), `cpg flow '<src-glob>' '<sink-glob>'` (quick ad-hoc query),
`cpg serve` (interactive JSON queries: methods, calls, summaries, taint).

## 6. Widen

- **Persistence stitching** (`CPG_PERSIST=1`): connects `store(K = tainted)`
  / `x.K = tainted` writes to later reads of `K` in other methods — the
  config-written-here-executed-there class. Findings are prefixed
  `persisted:`; expect over-approximation and triage them as a separate
  queue.
- **Cross-service** (`cpg merge -o all.cpg --protos <dir>... --thrifts
  <dir>... a.cpg b.cpg ...`): stitches gRPC/thrift client calls to server
  handlers so taint crosses service boundaries; then scan the merged CPG.

## 7. Repeat

Every triage round updates the pack; every pack update makes the next scan
cheaper. When a pack stabilises for a codebase family, promote it into
`iris/packs/` so it ships in the binary.

## Machine use (MCP)

`cpg mcp --root <repo>` exposes this whole loop to AI agents over the Model
Context Protocol (stdio): building, scanning, coverage, slicing, flow
queries, and graph inspection as tools; the packs and this document as
resources. The agent loop is the same loop as above — the tools are shaped
so step N's output is step N+1's input.
