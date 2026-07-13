# CPG Conformance Seed

This directory defines the start of the schema-as-contract gate for oxidized frontends.

Parity tests answer "does this frontend match the current reference emitter?" Conformance tests answer "does this frontend produce the required CPG shape for a language construct?" A frontend is correct when its emitted graph validates against `schema/cpg-schema.json` and passes the relevant conformance specs.

The seed suite is wired to rust2cpg first:

```bash
sbt "rust2cpg/testOnly io.joern.rust2cpg.conformance.LanguageNeutralConformanceTests"
```

The full rust2cpg gate also runs it:

```bash
sbt "rust2cpg/test"
```

## Seed Constructs

The initial executable specs cover ten language-neutral constructs:

| Construct | Required CPG shape |
| --- | --- |
| Function definition | A `METHOD` has `FULL_NAME`, `SIGNATURE`, `METHOD_RETURN`, and a body `BLOCK`. |
| Call | A source call is a `CALL` with `NAME`, `METHOD_FULL_NAME`, `DISPATCH_TYPE`, and AST placement in the enclosing method body. |
| If/else | A conditional is a `CONTROL_STRUCTURE` of type `IF`, with a condition expression, true body, and else body. |
| Loop | A loop is a `CONTROL_STRUCTURE` with an explicit condition and body block. |
| Local + assignment | A binding creates a `LOCAL` and an assignment `CALL` whose arguments are the left-side identifier and right-side expression. |
| Field access | A field read is a `CALL` to `<operator>.fieldAccess` with base and field arguments. |
| Closure | A closure creates a lambda `METHOD` plus a `METHOD_REF` pointing to it. |
| Type declaration | A nominal type creates a `TYPE_DECL` with `FULL_NAME` and `MEMBER` children for fields. |
| Return | A return statement is a `RETURN` node under the method body with the returned expression as an AST child. |
| Literals | Literal expressions are `LITERAL` nodes with code and type information. |

## Fidelity And Lazy Analysis

The seed uses `Rust2CpgSuite(noSysRoot = true)`, so it exercises the fast syntactic tier without requiring a full build or sysroot. Specs should be explicit about which tier they require:

- Tier 1, syntactic: AST-level CPG shape, local construct structure, literals, calls as emitted.
- Tier 2, resolved: name/type resolution that requires a language service or compiler-like frontend.
- Tier 3, whole-program: build-integrated facts and cross-crate/project analysis.

Future frontends should add the same conformance constructs before replacing their legacy lowering path. Expensive semantic passes should stay separate from these shape checks unless a spec explicitly declares a higher fidelity tier.
