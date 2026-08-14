#!/usr/bin/env python3
"""Exercise a release binary (or container) through its supported core flow."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path


TIMEOUT_SECONDS = 120


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--binary", type=Path)
    target.add_argument("--container")
    parser.add_argument("--version", required=True)
    return parser.parse_args()


class ReleaseTarget:
    def __init__(
        self, *, binary: Path | None, container: str | None, workspace: Path
    ) -> None:
        self.binary = binary
        self.container = container
        self.workspace = workspace

    def command(self, args: list[str]) -> list[str]:
        if self.binary is not None:
            return [str(self.binary), *args]
        assert self.container is not None
        mount = f"type=bind,source={self.workspace},target=/workspace"
        return [
            "docker",
            "run",
            "--rm",
            "-i",
            "--mount",
            mount,
            "--workdir",
            "/workspace",
            self.container,
            *args,
        ]

    def run(
        self,
        args: list[str],
        *,
        input_text: str | None = None,
        check: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            self.command(args),
            cwd=self.workspace,
            input=input_text,
            capture_output=True,
            text=True,
            timeout=TIMEOUT_SECONDS,
        )
        if check and result.returncode != 0:
            command = " ".join(self.command(args))
            raise SystemExit(
                f"release command failed ({result.returncode}): {command}\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        return result


def write_fixture(workspace: Path) -> tuple[Path, Path]:
    source_dir = workspace / "fixture project"
    source_dir.mkdir()
    source = source_dir / "vulnerable.c"
    source.write_text(
        """#include <stdlib.h>

int main(void) {
    char *command = getenv("RELEASE_COMMAND");
    system(command);
    return 0;
}
""",
        encoding="utf-8",
    )
    rules = workspace / "release rules.json"
    rules.write_text(
        json.dumps(
            {
                "rules": [
                    {
                        "id": "RELEASE-TAINT",
                        "name": "release-taint-flow",
                        "description": "release acceptance source-to-sink flow",
                        "severity": "high",
                        "sources": ["getenv"],
                        "sinks": ["system"],
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    return source, rules


def assert_version(target: ReleaseTarget, version: str) -> None:
    result = target.run(["--version"])
    expected = f"cpg {version}"
    actual = result.stdout.strip()
    if actual != expected:
        raise SystemExit(f"expected {expected!r}, got {actual!r}")


def assert_saved_graph(target: ReleaseTarget, graph: Path) -> None:
    target.run(
        ["build", "fixture project", "-o", graph.name, "--lang", "c"]
    )
    if not graph.is_file() or graph.stat().st_size == 0:
        raise SystemExit("cpg build did not create a non-empty saved graph")

    result = target.run(
        ["serve", "--load", graph.name],
        input_text='{"cmd":"stats"}\n{"cmd":"quit"}\n',
    )
    responses = [json.loads(line) for line in result.stdout.splitlines() if line]
    if len(responses) != 1:
        raise SystemExit(f"expected one stats response, got {responses!r}")
    stats = responses[0]
    if not all(isinstance(stats.get(key), int) for key in ("nodes", "methods", "calls")):
        raise SystemExit(f"saved graph returned invalid stats: {stats!r}")
    if stats["nodes"] <= 0 or stats["methods"] <= 0 or stats["calls"] < 2:
        raise SystemExit(f"saved graph returned incomplete stats: {stats!r}")


def assert_scan(target: ReleaseTarget, graph: Path, rules: Path, sarif: Path) -> None:
    target.run(
        [
            "scan",
            "--load",
            graph.name,
            "--rules",
            rules.name,
            "--lang",
            "c",
            "-o",
            sarif.name,
        ]
    )
    try:
        document = json.loads(sarif.read_text(encoding="utf-8"))
        run = document["runs"][0]
        rules_by_id = {rule["id"]: rule for rule in run["tool"]["driver"]["rules"]}
        findings = [
            result
            for result in run["results"]
            if result.get("ruleId") == "RELEASE-TAINT"
        ]
    except (OSError, json.JSONDecodeError, KeyError, IndexError, TypeError) as error:
        raise SystemExit(f"invalid release SARIF: {error}") from error

    if "RELEASE-TAINT" not in rules_by_id or not findings:
        raise SystemExit("release scan did not emit the expected rule and finding")
    location = findings[0]["locations"][0]["physicalLocation"]
    uri = location["artifactLocation"]["uri"]
    line = location["region"]["startLine"]
    if not uri.endswith("vulnerable.c") or not isinstance(line, int) or line < 1:
        raise SystemExit(f"release finding has an invalid location: {location!r}")


def assert_malformed_graph_rejected(target: ReleaseTarget, malformed: Path) -> None:
    malformed.write_bytes(b"not-a-cpg")
    result = target.run(["serve", "--load", malformed.name], check=False)
    if result.returncode == 0:
        raise SystemExit("malformed saved graph was accepted")
    if result.returncode < 0:
        raise SystemExit(f"malformed saved graph killed cpg with signal {-result.returncode}")
    if "load failed" not in result.stderr.lower():
        raise SystemExit(
            "malformed saved graph did not produce a controlled load error:\n"
            f"{result.stderr}"
        )


def main() -> None:
    args = arguments()
    binary = args.binary.resolve() if args.binary is not None else None
    if binary is not None and not binary.is_file():
        raise SystemExit(f"release binary does not exist: {binary}")
    if not args.version or args.version.startswith("v"):
        raise SystemExit("--version must be the Cargo version without a leading v")

    with tempfile.TemporaryDirectory(prefix="cpg release acceptance ") as temp:
        workspace = Path(temp)
        if args.container is not None:
            os.chmod(workspace, 0o777)
        _, rules = write_fixture(workspace)
        graph = workspace / "release graph.cpg"
        sarif = workspace / "release findings.sarif"
        malformed = workspace / "malformed graph.cpg"
        target = ReleaseTarget(
            binary=binary, container=args.container, workspace=workspace
        )

        assert_version(target, args.version)
        assert_saved_graph(target, graph)
        assert_scan(target, graph, rules, sarif)
        assert_malformed_graph_rejected(target, malformed)

    kind = f"container {args.container}" if args.container else str(binary)
    print(f"release acceptance passed: {kind}")


if __name__ == "__main__":
    main()
