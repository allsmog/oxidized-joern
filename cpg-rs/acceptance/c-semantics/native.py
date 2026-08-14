#!/usr/bin/env python3
"""Normalize the native compiler-input fixture to the pinned Joern probe."""

import argparse
import base64
import json
import subprocess


CASES = [
    ("selected-branch", 'cpg.method.nameExact("selected", "dead").name'),
    ("macro-call", 'cpg.call.nameExact("SCALE").code'),
    ("macro-origin", 'cpg.method.nameExact("SCALE").fullName'),
    (
        "expanded-multiply",
        'cpg.call.nameExact("<operator>.multiplication").code',
    ),
]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpg", required=True)
    parser.add_argument("--fixture", required=True)
    args = parser.parse_args()
    common = [
        args.cpg,
        "query",
        args.fixture,
        "--lang",
        "c",
        "--compile-commands",
        "compile_commands.json",
    ]
    for case_id, query in CASES:
        completed = subprocess.run(
            [*common, "--query", query],
            check=True,
            capture_output=True,
            text=True,
        )
        values = json.loads(completed.stdout)
        if not isinstance(values, list):
            raise SystemExit(f"{case_id}: native query did not return a list")
        normalized = "\x1f".join(sorted(str(value) for value in values))
        encoded = base64.b64encode(normalized.encode("utf-8")).decode("ascii")
        print(f"CSEM\t{case_id}\t{encoded}")


if __name__ == "__main__":
    main()
