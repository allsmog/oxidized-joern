#!/usr/bin/env python3
"""Generate a Joern probe directly from the complete CPGQL catalog."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


HEADER = """import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*
import java.nio.charset.StandardCharsets
import java.util.Base64

def asValues(value: Any): List[Any] = value match {
  case iterator: Iterator[?] => iterator.toList
  case iterable: Iterable[?] => iterable.toList
  case option: Option[?] => option.toList
  case value => List(value)
}

def canonical(value: Any): String = value match {
  case node: StoredNode => s"node:${node.label}"
  case value => value.toString
}

def emit(id: String, value: Any): Unit = {
  val normalized = asValues(value).map(canonical).sorted.mkString("\\u001f")
  val encoded = Base64.getEncoder.encodeToString(normalized.getBytes(StandardCharsets.UTF_8))
  println(s"CPGQL\\t$id\\t$encoded")
}

@main def exec(cpgPath: String) = {
  val cpg = CpgLoader.load(cpgPath)
  try {
"""

FOOTER = """  } finally cpg.close()
}
"""

# The native compiler accepts a small set of source-compatible aliases for
# steps whose Joern v4.0.555 spelling is either lower-level or more awkward.
# Keep the catalog query unchanged and map only the oracle expression here.
ORACLE_EQUIVALENTS = {
    "call-file": 'cpg.call.method.filename.dedup',
    "call-limit": 'cpg.call.take(10).name',
    "contained-identifiers": 'cpg.method("main").ast.isIdentifier.name',
    "parameter-out": (
        'cpg.method("main").astChildren.collectAll[MethodParameterOut].name'
    ),
    "parameter-link": (
        'cpg.method("main").parameter.flatMap(_._parameterLinkOut)'
        ".collectAll[MethodParameterOut].name"
    ),
    "repeat-until": (
        'cpg.call("strcpy").repeat(_.astParent)(_.until(_.isMethod))'
        ".isMethod.name"
    ),
    "repeat-times-emit": (
        'cpg.method("main").repeat(_.astChildren)(_.emitAllButFirst.maxDepth(2)).label'
    ),
    "all-returns-alias": 'cpg.ret.code',
    "filter-not": 'cpg.method.filterNot(_.call("strcpy").nonEmpty).name',
    "surrounding-call": (
        'cpg.identifier("input").where(_.method.name("main")).inCall.name'
    ),
    "repeat-max-depth": (
        'cpg.method("main").repeat(_.astChildren)'
        '(_.emit(_.isCall).maxDepth(2)).isCall.name'
    ),
    "descendant-annotation": 'cpg.method.ast.isAnnotation',
    "source-file-edge": (
        "cpg.method.flatMap(_._sourceFileOut).collectAll[File].name"
    ),
    "positive-source-file-edge": (
        "cpg.method.flatMap(_._sourceFileOut).collectAll[File].name"
    ),
    "positive-base-type-declaration": (
        'cpg.typeDecl.name("Derived").flatMap(_._inheritsFromOut)'
        ".collectAll[TypeDecl].fullName"
    ),
    "positive-derived-type-declaration": (
        'cpg.typeDecl.name("Base").flatMap(_._inheritsFromIn)'
        ".collectAll[TypeDecl].fullName"
    ),
    # The C fixture has no closure bindings; selection still proves the empty
    # traversal behavior while the positive CAPTURE edge is covered by the
    # Flatgraph interoperability corpus.
    "capture-edges": 'cpg.closureBinding.flatMap(_._captureOut)',
    "positive-capture-edge": (
        'cpg.closureBinding.flatMap(_._captureOut).map(_.label)'
    ),
}


def oracle_expression(case: dict[str, str]) -> str:
    query = ORACLE_EQUIVALENTS.get(case["id"], case["query"])
    # The native CLI itself is the JSON/pretty-print execution boundary, so
    # these Joern console terminals normalize to their underlying traversal.
    for terminal in (".toJsonPretty", ".toJson", ".browse", ".p", ".clone"):
        if query.endswith(terminal):
            query = query[: -len(terminal)]
            break
    if "reachableByFlows(" in query:
        return (
            f"({query}).map(path => path.elements.map(node => "
            's"node:${node.label}").mkString(" -> "))'
        )
    return query


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    document = json.loads(Path(args.catalog).read_text(encoding="utf-8"))
    cases = [case for tier in document["tiers"] for case in tier["cases"]]
    ids = [case["id"] for case in cases]
    if len(ids) != len(set(ids)):
        raise SystemExit("CPGQL catalog case ids must be unique")

    lines = [HEADER]
    for case in cases:
        identifier = json.dumps(case["id"])
        lines.append(f"    emit({identifier}, {oracle_expression(case)})\n")
    lines.append(FOOTER)
    Path(args.output).write_text("".join(lines), encoding="utf-8")


if __name__ == "__main__":
    main()
