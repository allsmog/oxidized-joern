# Quarry Enhancements

Quarry is a fork of [Joern](https://github.com/joernio/joern). It tracks upstream Joern
and concentrates on maturing the **Go** (`gosrc2cpg`) and **Python** (`pysrc2cpg`)
frontends so that data-flow and taint queries surface more on real-world Go and Python
codebases. Everything else — the query language, the other language frontends, the CPG
schema, and tooling — is unchanged from upstream.

These improvements come with end-to-end test coverage (comprehensions, the walrus
operator, kwargs, match patterns, Go concurrency statements, type assertions, and more).

---

## Go frontend (`gosrc2cpg`)

- **Concurrency and channel statements:** added support for `defer`, `go`, `select`,
  and channel send (`<-`) statements, plus `CommClause` handling, via new parser node
  types and handler methods.
- **Tuple returns:** multi-value returns now correctly represent `(type1, type2)`
  instead of collapsing to a single type.
- **`fallthrough`:** now produces a proper control-structure node instead of being
  dropped.
- **Literal types per Go spec:** float literals resolve to `float64`, imaginary
  literals to `complex128`, and character literals to `int32`.
- **Pointer-to-pointer (`**T`):** resolved recursively so multi-level pointer types are
  represented correctly.
- **Type assertions:** `TypeAssertExpr` now produces an `Operators.cast` call with the
  correct result type.
- **Interfaces:** interface method sets are extracted, with an interface method-lookup
  fallback for resolution.

## Python frontend (`pysrc2cpg`)

- **`**kwargs` taint tracking:** kwargs unpacking now preserves the dict argument (using
  a `<keyword_dict>` argument name) so taint flows through keyword arguments.
- **Type recovery from annotations:** the type-recovery symbol table is seeded from
  parameter, local, and return-type annotations.
- **Richer type-hint extraction:** handles `Optional`, `Union`, `List`, `Dict`, and
  `Tuple` generics, plus Python 3.10+ pipe-union syntax (`int | str`).
- **Stdlib type stubs:** type stubs for ~45 stdlib builtins and common stdlib functions.
- **Match-pattern lowering:** `match`/`case` patterns are lowered into proper AST
  assignment nodes that encode destructuring semantics, enabling data-flow tracking
  through pattern-bound variables — `MatchSequence`, `MatchAs` (catch-all and alias),
  `MatchMapping`, `MatchClass`, `MatchOr`, and `MatchStar`. Nested and complex subjects
  use temp variables (as tuple unpacking does), and `JumpTarget` nodes are preserved for
  CFG compatibility.
- **Dependency resolution:** added support for `pyproject.toml` (PEP 621 and Poetry) and
  `setup.cfg`, plus flexible version specifiers (`>=`, `<=`, `~=`, …) in
  `requirements.txt`.
- **Import resolution:** `__init__.py` package imports and star-import expansion (via
  module-cache lookup).
- **Test coverage:** comprehensive tests for list/set/dict/generator comprehensions and
  the walrus operator.

---

## Upstream contributions

Some of this work has been contributed back to upstream Joern (e.g. PR #5910,
`[pysrc2cpg] Fix **kwargs handling, walrus operator and comprehension test coverage`).
