#!/usr/bin/env python3
"""Run the committed differential CPGQL cases through the native CLI."""

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

    cases = json.loads(Path(args.catalog).read_text(encoding="utf-8"))
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
            raise SystemExit(f"{case['id']}: differential query did not return a list")
        if case["id"] == "flow-paths":
            value = [" -> ".join(str(node.get("code")) for node in path) for path in value]
        def render(item: object) -> str:
            if isinstance(item, bool):
                return str(item).lower()
            return str(item)

        normalized = "\x1f".join(sorted(render(item) for item in value))
        encoded = base64.b64encode(normalized.encode("utf-8")).decode("ascii")
        print(f"CPGQL\t{case['id']}\t{encoded}")


if __name__ == "__main__":
    main()
