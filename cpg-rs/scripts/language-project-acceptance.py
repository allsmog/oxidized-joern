#!/usr/bin/env python3
"""Pinned non-C real-project build, persistence, query, scan, and determinism gate."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import time
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "acceptance" / "language-projects" / "manifest.json"
QUERY = "cpg.method.name.dedup"
PROMOTED_LANGUAGES = {
    "cpp",
    "go",
    "java",
    "javascript",
    "python",
    "ruby",
    "rust",
    "typescript",
}
PROJECTS_PER_LANGUAGE = 2
MINIMUM_GRAPH_NODES = 10


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str], *, env: dict[str, str] | None = None, timeout: int = 120) -> tuple[str, float]:
    started = time.monotonic()
    result = subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        timeout=timeout,
    )
    elapsed = time.monotonic() - started
    if result.returncode:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n{result.stdout}{result.stderr}"
        )
    return result.stdout + result.stderr, elapsed


def stdout(command: list[str], *, timeout: int = 120) -> str:
    result = subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )
    if result.returncode:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n{result.stdout}{result.stderr}"
        )
    return result.stdout


def fetch(project: dict, cache: Path) -> Path:
    archive = cache / f"{project['id']}.tar.gz"
    if not archive.exists():
        cache.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(project["url"], timeout=60) as response:
            archive.write_bytes(response.read())
    actual = sha256(archive)
    if actual != project["archiveSha256"]:
        raise RuntimeError(f"{project['id']}: archive hash {actual}, expected {project['archiveSha256']}")
    return archive


def extract(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True)
    with tarfile.open(archive, "r:gz") as bundle:
        root = destination.resolve()
        for member in bundle.getmembers():
            target = (destination / member.name).resolve()
            if root not in target.parents and target != root:
                raise RuntimeError(f"unsafe archive member: {member.name}")
        bundle.extractall(destination, filter="data")


def selected_sources(project: dict, source_root: Path) -> list[Path]:
    extensions = set(project["extensions"])
    excludes = project["excludes"]
    selected = []
    for path in source_root.rglob("*"):
        normalized = "/" + path.relative_to(source_root).as_posix()
        if path.is_file() and path.suffix.lstrip(".") in extensions and not any(
            excluded in normalized for excluded in excludes
        ):
            selected.append(path)
    return sorted(selected)


def validate_project(project: dict, binary: Path, cache: Path, *, measure: bool) -> dict:
    archive = fetch(project, cache)
    expected = project["expected"]
    with tempfile.TemporaryDirectory(prefix=f"cpg-{project['id']}-") as temp_name:
        temp = Path(temp_name)
        extract(archive, temp / "source")
        project_root = temp / "source" / project["archiveRoot"]
        source_root = project_root / project["sourceRoot"]
        license_path = project_root / project["licenseFile"]
        if not license_path.is_file():
            raise RuntimeError(f"{project['id']}: missing recorded license {project['licenseFile']}")
        sources = selected_sources(project, source_root)
        if len(sources) != expected["sourceFiles"]:
            raise RuntimeError(
                f"{project['id']}: {len(sources)} source files, expected {expected['sourceFiles']}"
            )
        if expected["nodes"] < MINIMUM_GRAPH_NODES:
            raise RuntimeError(
                f"{project['id']}: expected graph has only {expected['nodes']} nodes; "
                f"minimum is {MINIMUM_GRAPH_NODES}"
            )

        outputs = []
        nodes = 0
        findings = 0
        for iteration in (1, 2):
            graph = temp / f"graph-{iteration}.cpg"
            edges = temp / f"edges-{iteration}.txt"
            env = os.environ.copy()
            env["CPG_DUMP_EDGES"] = str(edges)
            command = [
                str(binary),
                "build",
                str(source_root),
                "-o",
                str(graph),
                "--lang",
                project["language"],
            ]
            for excluded in project["excludes"]:
                command.extend(["--exclude", excluded])
            output, elapsed = run(command, env=env, timeout=project["budgets"]["buildSeconds"] * 3)
            if elapsed > project["budgets"]["buildSeconds"]:
                raise RuntimeError(f"{project['id']}: build took {elapsed:.2f}s")
            nodes = int(output.split("saved ", 1)[1].split(" nodes", 1)[0])
            if not measure and nodes != expected["nodes"]:
                raise RuntimeError(f"{project['id']}: {nodes} nodes, expected {expected['nodes']}")

            export_dir = temp / f"export-{iteration}"
            run(
                [
                    str(binary),
                    "export",
                    "--load",
                    str(graph),
                    "--lang",
                    project["language"],
                    "--repr",
                    "all",
                    "--format",
                    "json",
                    "-o",
                    str(export_dir),
                ]
            )
            query = temp / f"query-{iteration}.json"
            query.write_text(
                stdout(
                    [
                        str(binary),
                        "query",
                        "--load",
                        str(graph),
                        "--lang",
                        project["language"],
                        "--query",
                        QUERY,
                    ]
                ),
                encoding="utf-8",
            )
            sarif = temp / f"scan-{iteration}.sarif"
            run(
                [
                    str(binary),
                    "scan",
                    "--load",
                    str(graph),
                    "--lang",
                    project["language"],
                    "-o",
                    str(sarif),
                ]
            )
            findings = len(json.loads(sarif.read_text(encoding="utf-8"))["runs"][0]["results"])
            if not measure and findings != expected["findings"]:
                raise RuntimeError(
                    f"{project['id']}: {findings} findings, expected {expected['findings']}"
                )
            outputs.append(
                {
                    "graph": sha256(graph),
                    "edges": sha256(edges),
                    "export": sha256(export_dir / "export.json"),
                    "query": sha256(query),
                    "sarif": sha256(sarif),
                }
            )

        if outputs[0] != outputs[1]:
            raise RuntimeError(f"{project['id']}: repeated outputs differ: {outputs}")
        keys = {
            "graph": "graphSha256",
            "edges": "edgesSha256",
            "export": "exportSha256",
            "query": "querySha256",
            "sarif": "sarifSha256",
        }
        if not measure:
            for key, manifest_key in keys.items():
                if outputs[0][key] != expected[manifest_key]:
                    raise RuntimeError(
                        f"{project['id']}: {key} hash {outputs[0][key]}, expected {expected[manifest_key]}"
                    )
        print(
            f"PASS {project['id']} ({project['language']}): "
            f"{len(sources)} files, {nodes} nodes, {findings} findings"
        )
        return {
            "id": project["id"],
            "sourceFiles": len(sources),
            "nodes": nodes,
            "graphSha256": outputs[0]["graph"],
            "edgesSha256": outputs[0]["edges"],
            "exportSha256": outputs[0]["export"],
            "querySha256": outputs[0]["query"],
            "sarifSha256": outputs[0]["sarif"],
            "findings": findings,
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "release" / "cpg")
    parser.add_argument("--cache", type=Path, default=Path(tempfile.gettempdir()) / "cpg-language-project-cache")
    parser.add_argument("--project", action="append")
    parser.add_argument("--measure", action="store_true")
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    projects_by_language = Counter(project["language"] for project in manifest["projects"])
    expected_counts = dict.fromkeys(PROMOTED_LANGUAGES, PROJECTS_PER_LANGUAGE)
    if projects_by_language != expected_counts:
        raise RuntimeError(
            "manifest must contain exactly two projects for each promoted language: "
            f"found {dict(sorted(projects_by_language.items()))}"
        )
    project_ids = [project["id"] for project in manifest["projects"]]
    if len(project_ids) != len(set(project_ids)):
        raise RuntimeError("manifest contains duplicate project ids")
    chosen = set(args.project or [])
    projects = [p for p in manifest["projects"] if not chosen or p["id"] in chosen]
    if chosen - {p["id"] for p in projects}:
        raise RuntimeError(f"unknown project(s): {sorted(chosen - {p['id'] for p in projects})}")
    actuals = [
        validate_project(project, args.binary.resolve(), args.cache, measure=args.measure)
        for project in projects
    ]
    if args.measure:
        print(json.dumps({"actuals": actuals}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
