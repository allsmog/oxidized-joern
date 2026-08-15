#!/usr/bin/env python3
"""Run the language differential queries through the native CLI."""

from __future__ import annotations

import argparse
import base64
import json
from pathlib import Path
import subprocess


def normalize(case_id: str, item: object) -> str:
    rendered = str(item).lower() if isinstance(item, bool) else str(item)
    if case_id in {"parameter-types", "return-types"}:
        rendered = rendered.replace("::", ".").rsplit(".", 1)[-1]
    if case_id == "returns":
        rendered = rendered.rstrip().removesuffix(";")
    return rendered


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpg", required=True)
    parser.add_argument("--graph", required=True)
    parser.add_argument("--language", required=True)
    parser.add_argument("--queries", required=True)
    args = parser.parse_args()

    queries = json.loads(Path(args.queries).read_text(encoding="utf-8"))
    for case in queries:
        completed = subprocess.run(
            [
                args.cpg,
                "query",
                "--load",
                args.graph,
                "--lang",
                args.language,
                "--query",
                case["query"],
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        value = json.loads(completed.stdout)
        if not isinstance(value, list):
            raise SystemExit(f"{case['id']}: differential query did not return a list")
        normalized = "\x1f".join(sorted(normalize(case["id"], item) for item in value))
        encoded = base64.b64encode(normalized.encode("utf-8")).decode("ascii")
        print(f"LANGUAGE\t{case['id']}\t{encoded}")


if __name__ == "__main__":
    main()
