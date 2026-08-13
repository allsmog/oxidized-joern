#!/usr/bin/env python3
"""Create and smoke-test a deterministic Rust-native cpg release archive."""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import os
import shutil
import stat
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--version", required=True)
    return parser.parse_args()


def source_date_epoch(repo: Path) -> int:
    configured = os.environ.get("SOURCE_DATE_EPOCH")
    if configured is not None:
        return int(configured)
    return int(
        subprocess.check_output(
            ["git", "show", "-s", "--format=%ct", "HEAD"],
            cwd=repo,
            text=True,
        ).strip()
    )


def verify_version(binary: Path, version: str) -> None:
    result = subprocess.run(
        [str(binary), "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    actual = result.stdout.strip()
    expected = f"cpg {version}"
    if actual != expected:
        raise SystemExit(f"expected {expected!r}, got {actual!r}")


def normalized_tar(archive: Path, root: Path, package: str, epoch: int) -> None:
    with (
        archive.open("wb") as raw,
        gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as compressed,
        tarfile.open(fileobj=compressed, mode="w") as output,
    ):
        for path in sorted((root / package).rglob("*")):
            relative = Path(package) / path.relative_to(root / package)
            info = output.gettarinfo(str(path), arcname=relative.as_posix())
            info.uid = 0
            info.gid = 0
            info.uname = "root"
            info.gname = "root"
            info.mtime = epoch
            with path.open("rb") as source:
                output.addfile(info, source)


def normalized_zip(archive: Path, root: Path, package: str, epoch: int) -> None:
    timestamp = dt.datetime.fromtimestamp(max(epoch, 315532800), tz=dt.timezone.utc)
    date_time = (
        timestamp.year,
        timestamp.month,
        timestamp.day,
        timestamp.hour,
        timestamp.minute,
        timestamp.second,
    )
    with zipfile.ZipFile(
        archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as output:
        for path in sorted((root / package).rglob("*")):
            relative = Path(package) / path.relative_to(root / package)
            info = zipfile.ZipInfo(relative.as_posix(), date_time=date_time)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = (path.stat().st_mode & 0xFFFF) << 16
            output.writestr(info, path.read_bytes(), compresslevel=9)


def extract_and_smoke_test(
    archive: Path, package: str, executable: str, version: str
) -> None:
    with tempfile.TemporaryDirectory(prefix="cpg-release-smoke-") as temp:
        destination = Path(temp)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as source:
                source.extractall(destination)
        else:
            with tarfile.open(archive, "r:gz") as source:
                source.extractall(destination, filter="data")
        binary = destination / package / executable
        if os.name != "nt":
            binary.chmod(
                binary.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
            )
        verify_version(binary, version)


def main() -> None:
    args = arguments()
    repo = Path(__file__).resolve().parents[2]
    binary = args.binary.resolve()
    if not binary.is_file():
        raise SystemExit(f"release binary does not exist: {binary}")
    if not args.version or args.version.startswith("v"):
        raise SystemExit("--version must be the Cargo version without a leading v")

    windows = args.platform.startswith("windows-")
    executable = "cpg.exe" if windows else "cpg"
    package = f"oxidized-joern-cpg-{args.platform}"
    extension = ".zip" if windows else ".tar.gz"
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output / f"{package}{extension}"
    epoch = source_date_epoch(repo)

    verify_version(binary, args.version)
    with tempfile.TemporaryDirectory(prefix="cpg-release-stage-") as temp:
        stage = Path(temp)
        package_dir = stage / package
        package_dir.mkdir()
        shutil.copy2(binary, package_dir / executable)
        shutil.copy2(repo / "LICENSE", package_dir / "LICENSE")
        shutil.copy2(repo / "cpg-rs" / "README.md", package_dir / "README.md")
        (package_dir / executable).chmod(0o755)
        (package_dir / "LICENSE").chmod(0o644)
        (package_dir / "README.md").chmod(0o644)
        if windows:
            normalized_zip(archive, stage, package, epoch)
        else:
            normalized_tar(archive, stage, package, epoch)

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(f"{archive.name}.sha256").write_text(
        f"{digest}  {archive.name}\n",
        encoding="utf-8",
    )
    extract_and_smoke_test(archive, package, executable, args.version)
    print(archive)


if __name__ == "__main__":
    main()
