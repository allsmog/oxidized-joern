#!/usr/bin/env python3
"""Run the complete committed CPGQL catalog through the native CLI."""

import argparse
import base64
import json
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpg", required=True)
    parser.add_argument("--graph", required=True)
    parser.add_argument("--catalog", required=True)
    args = parser.parse_args()

    document = json.loads(Path(args.catalog).read_text(encoding="utf-8"))
    cases = [case for tier in document["tiers"] for case in tier["cases"]]
    for case in cases:
        completed = subprocess.run(
            [
                args.cpg,
                "query",
                "--load",
                args.graph,
                "--lang",
                "c",
                "--query",
                case["query"],
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        value = json.loads(completed.stdout)
        if not isinstance(value, list):
            value = [value]

        def render(item: object) -> str:
            if isinstance(item, bool):
                return str(item).lower()
            if isinstance(item, dict) and "id" in item and "label" in item:
                return f"node:{item['label']}"
            if isinstance(item, list):
                return " -> ".join(render(element) for element in item)
            return str(item)

        normalized = "\x1f".join(sorted(render(item) for item in value))
        encoded = base64.b64encode(normalized.encode("utf-8")).decode("ascii")
        print(f"CPGQL\t{case['id']}\t{encoded}")


if __name__ == "__main__":
    main()
