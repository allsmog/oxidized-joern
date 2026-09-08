"""Regression tests for the parity gate, without compiling Rust or running a JVM."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ORACLE = """AST|METHOD NAME=first FULL_NAME=first
AST|  METHOD_RETURN CODE=RET
AST|
AST|METHOD NAME=second FULL_NAME=second
AST|  METHOD_RETURN CODE=RET
AST|
NODES|TYPE NAME=int
EDGES|CFG first#0 -> first#1
FLOWS|REACHING_DEF[] first#0 -> first#1
"""


def rust_dump(oracle):
    return "\n".join(line.removeprefix("AST|") for line in oracle.splitlines()) + "\n"


class ParityGateTests(unittest.TestCase):
    def run_gate(
        self,
        actual=None,
        cargo_exit=0,
        oracle=ORACLE,
        mode="--committed-only",
        live_output=None,
        live_exit=0,
    ):
        with tempfile.TemporaryDirectory(prefix="parity-gate-test-") as directory:
            root = Path(directory)
            gate = root / "joern-parity"
            gate.mkdir()
            shutil.copyfile(Path(__file__).with_name("check.sh"), gate / "check.sh")
            (gate / "oracle_all.txt").write_text(oracle)
            (gate / "corpus").mkdir()
            (gate / "corpus" / "test.c").write_text("int first(void) { return 1; }\n")
            output = root / "actual.txt"
            output.write_text(rust_dump(ORACLE) if actual is None else actual)
            binaries = root / "bin"
            binaries.mkdir()
            cargo = binaries / "cargo"
            cargo.write_text(
                '#!/bin/sh\ncat "$PARITY_TEST_ACTUAL"\nexit "$PARITY_TEST_EXIT"\n'
            )
            cargo.chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                PATH=str(binaries) + os.pathsep + environment["PATH"],
                PARITY_TEST_ACTUAL=str(output),
                PARITY_TEST_EXIT=str(cargo_exit),
                JOERN=str(root / "oracle"),
            )
            if live_output is not None:
                oracle_directory = root / "oracle"
                oracle_directory.mkdir()
                (oracle_directory / "workspace").mkdir()
                sentinel = oracle_directory / "workspace" / "keep"
                sentinel.write_text("existing workspace")
                live_reference = root / "live.txt"
                live_reference.write_text(live_output)
                joern = oracle_directory / "joern"
                joern.write_text(
                    '#!/bin/sh\ncat "$PARITY_TEST_ORACLE"\nexit "$PARITY_TEST_ORACLE_EXIT"\n'
                )
                joern.chmod(0o755)
                environment.update(
                    PARITY_TEST_ORACLE=str(live_reference),
                    PARITY_TEST_ORACLE_EXIT=str(live_exit),
                )
            result = subprocess.run(
                ["bash", str(gate / "check.sh"), mode],
                capture_output=True,
                text=True,
                env=environment,
                timeout=15,
            )
            self.assertEqual((gate / "oracle_all.txt").read_text(), oracle)
            if live_output is not None:
                self.assertEqual(sentinel.read_text(), "existing workspace")
            return result

    def test_identical_projection_passes(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_missing_expected_method_fails(self):
        actual = rust_dump(ORACLE).replace(
            "METHOD NAME=second FULL_NAME=second\n  METHOD_RETURN CODE=RET\n\n", ""
        )
        result = self.run_gate(actual=actual)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unexpected_method_fails(self):
        result = self.run_gate(
            actual=rust_dump(ORACLE) + "METHOD NAME=extra FULL_NAME=extra\n\n"
        )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_changed_flow_fails(self):
        result = self.run_gate(
            actual=rust_dump(ORACLE).replace("REACHING_DEF[]", "REACHING_DEF[x]")
        )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_failed_rust_command_cannot_pass(self):
        result = self.run_gate(cargo_exit=1)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_incomplete_reference_cannot_pass(self):
        oracle = "\n".join(
            line for line in ORACLE.splitlines() if not line.startswith("FLOWS|")
        ) + "\n"
        result = self.run_gate(oracle=oracle, actual=rust_dump(oracle))
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_256_failures_cannot_wrap_exit_status_to_success(self):
        oracle = ORACLE + "".join(
            f"AST|METHOD NAME=missing{i} FULL_NAME=missing{i}\nAST|\n"
            for i in range(256)
        )
        result = self.run_gate(oracle=oracle)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_live_requires_an_installed_oracle(self):
        result = self.run_gate(mode="--live")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_live_failure_cannot_fall_back_to_passing_committed_reference(self):
        result = self.run_gate(mode="--live", live_output=ORACLE, live_exit=1)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_live_incomplete_output_cannot_fall_back(self):
        result = self.run_gate(
            mode="--live", live_output="AST|METHOD NAME=first FULL_NAME=first\n"
        )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_live_compares_actual_live_output_without_rewriting_reference(self):
        live = ORACLE.replace("second", "third")
        result = self.run_gate(mode="--live", live_output=live, actual=rust_dump(live))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
