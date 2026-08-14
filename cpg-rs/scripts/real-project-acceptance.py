#!/usr/bin/env python3
"""Pinned real-C-project determinism, persistence, scan, and budget gate."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "acceptance" / "real-projects" / "manifest.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str], *, env: dict[str, str] | None = None, timeout: int = 120) -> tuple[str, float, float]:
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
    rss = float(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)
    if sys.platform == "darwin":
        rss /= 1024 * 1024
    else:
        rss /= 1024
    return result.stdout + result.stderr, elapsed, rss


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


def sources(project: dict, source_root: Path) -> list[Path]:
    extensions = set(project["extensions"])
    excludes = project["excludes"]
    selected = []
    for path in source_root.rglob("*"):
        normalized = "/" + path.relative_to(source_root).as_posix()
        if path.is_file() and path.suffix.lstrip(".") in extensions and not any(x in normalized for x in excludes):
            selected.append(path)
    return sorted(selected)


def validate_project(
    project: dict, binary: Path, parity: Path, cache: Path, *, measure: bool = False
) -> dict:
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
        selected = sources(project, source_root)
        if len(selected) != expected["sourceFiles"]:
            raise RuntimeError(f"{project['id']}: {len(selected)} source files, expected {expected['sourceFiles']}")

        outputs = []
        peak_rss = 0.0
        for iteration in (1, 2):
            graph = temp / f"graph-{iteration}.cpg"
            edges = temp / f"edges-{iteration}.txt"
            env = os.environ.copy()
            env["CPG_DUMP_EDGES"] = str(edges)
            command = [str(binary), "build", str(source_root), "-o", str(graph), "--lang", "c"]
            for exclude in project["excludes"]:
                command.extend(["--exclude", exclude])
            output, elapsed, rss = run(command, env=env, timeout=project["budgets"]["buildSeconds"] * 3)
            peak_rss = max(peak_rss, rss)
            if elapsed > project["budgets"]["buildSeconds"]:
                raise RuntimeError(f"{project['id']}: build took {elapsed:.2f}s")
            marker = "saved "
            nodes = int(output.split(marker, 1)[1].split(" nodes", 1)[0])
            if not measure and nodes != expected["nodes"]:
                raise RuntimeError(f"{project['id']}: {nodes} nodes, expected {expected['nodes']}")

            export_dir = temp / f"export-{iteration}"
            run([str(binary), "export", "--load", str(graph), "--lang", "c", "--repr", "all", "--format", "json", "-o", str(export_dir)])
            sarif = temp / f"scan-{iteration}.sarif"
            run([str(binary), "scan", "--load", str(graph), "--lang", "c", "-o", str(sarif)])
            findings = len(json.loads(sarif.read_text())["runs"][0]["results"])
            if not measure and findings != expected["findings"]:
                raise RuntimeError(f"{project['id']}: {findings} findings, expected {expected['findings']}")
            outputs.append({
                "graph": sha256(graph),
                "edges": sha256(edges),
                "export": sha256(export_dir / "export.json"),
                "sarif": sha256(sarif),
            })

        if outputs[0] != outputs[1]:
            raise RuntimeError(f"{project['id']}: repeated outputs differ: {outputs}")
        for key, manifest_key in [("graph", "graphSha256"), ("edges", "edgesSha256"), ("export", "exportSha256"), ("sarif", "sarifSha256")]:
            if not measure and outputs[0][key] != expected[manifest_key]:
                raise RuntimeError(f"{project['id']}: {key} hash {outputs[0][key]}, expected {expected[manifest_key]}")
        if not measure and peak_rss > project["budgets"]["peakRssMiB"]:
            raise RuntimeError(f"{project['id']}: peak RSS {peak_rss:.1f} MiB exceeds budget")

        _, update_elapsed, _ = run(
            [str(parity), "--update-equivalence", *map(str, selected)],
            timeout=project["budgets"]["buildSeconds"] * 6,
        )
        print(
            f"PASS {project['id']}: {len(selected)} files, {nodes} nodes, "
            f"build <= {project['budgets']['buildSeconds']}s, peak RSS {peak_rss:.1f} MiB, "
            f"incremental equivalence {update_elapsed:.2f}s"
        )
        return {
            "id": project["id"],
            "sourceFiles": len(selected),
            "nodes": nodes,
            "graphSha256": outputs[0]["graph"],
            "edgesSha256": outputs[0]["edges"],
            "exportSha256": outputs[0]["export"],
            "sarifSha256": outputs[0]["sarif"],
            "findings": findings,
            "peakRssMiB": round(peak_rss, 1),
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "release" / "cpg")
    parser.add_argument("--parity", type=Path, default=ROOT / "target" / "release" / "joern-parity")
    parser.add_argument("--cache", type=Path, default=Path(tempfile.gettempdir()) / "cpg-real-project-cache")
    parser.add_argument("--project", action="append")
    parser.add_argument(
        "--measure",
        action="store_true",
        help="print deterministic actuals without accepting changed baselines",
    )
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())
    chosen = set(args.project or [])
    projects = [p for p in manifest["projects"] if not chosen or p["id"] in chosen]
    if chosen - {p["id"] for p in projects}:
        raise RuntimeError(f"unknown project(s): {sorted(chosen - {p['id'] for p in projects})}")
    actuals = []
    for project in projects:
        actuals.append(
            validate_project(
                project,
                args.binary.resolve(),
                args.parity.resolve(),
                args.cache,
                measure=args.measure,
            )
        )
    if args.measure:
        print(json.dumps({"actuals": actuals}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
