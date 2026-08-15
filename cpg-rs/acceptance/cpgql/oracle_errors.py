#!/usr/bin/env python3
"""Classify the committed invalid-query corpus against pinned Joern."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import tempfile


SCRIPT = """import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.semanticcpg.language.*

def force(value: Any): Unit = value match {{
  case iterator: Iterator[?] => iterator.toList
  case iterable: Iterable[?] => iterable.toList
  case option: Option[?] => option.toList
  case _ => ()
}}

@main def exec(cpgPath: String) = {{
  val cpg = CpgLoader.load(cpgPath)
  try {{
    try {{
      val result = {query}
      force(result)
      println("CPGQL_ERROR\\t{id}\\taccepted")
    }} catch {{
      case _: Throwable => println("CPGQL_ERROR\\t{id}\\trejected")
    }}
  }} finally cpg.close()
}}
"""


def classify(joern: Path, graph: Path, case: dict[str, str], scratch: Path) -> str:
    if not case["query"].strip():
        return "not-applicable"
    script = scratch / f"{case['id']}.sc"
    script.write_text(
        SCRIPT.format(query=case["query"], id=case["id"]), encoding="utf-8"
    )
    try:
        completed = subprocess.run(
            [
                str(joern),
                "--nocolors",
                "--script",
                str(script),
                "--param",
                f"cpgPath={graph}",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"{case['id']}: Joern classification timed out") from error
    marker = f"CPGQL_ERROR\t{case['id']}\t"
    for line in (completed.stdout + completed.stderr).splitlines():
        if line.startswith(marker):
            return line.removeprefix(marker)
    # Syntax and static type failures prevent the script from reaching its
    # marker. They are oracle rejections just like runtime traversal errors.
    return "rejected"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--joern", type=Path, required=True)
    parser.add_argument("--graph", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--measure", action="store_true")
    args = parser.parse_args()

    cases = json.loads(args.catalog.read_text(encoding="utf-8"))
    ids = [case["id"] for case in cases]
    if len(ids) != len(set(ids)):
        raise RuntimeError("error catalog case ids must be unique")
    with tempfile.TemporaryDirectory(prefix="cpgql-oracle-errors-") as temp_name:
        scratch = Path(temp_name)
        for case in cases:
            observed = classify(args.joern, args.graph, case, scratch)
            if args.measure:
                print(f"{case['id']}\t{observed}")
                continue
            expected = case.get("oracle")
            if observed != expected:
                raise RuntimeError(
                    f"{case['id']}: Joern {observed}, expected {expected}"
                )
            print(f"CPGQL_ERROR\t{case['id']}\t{observed}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
