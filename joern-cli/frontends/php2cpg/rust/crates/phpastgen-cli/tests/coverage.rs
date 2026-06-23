use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// PHP fixture exercising the common constructs the lowering passes must map.
///
/// Every construct here must lower without leaving an unmapped tree-sitter node,
/// so the `phpastgen: N unmapped node(s): …` stderr summary acts as a coverage
/// gate. Adding a construct that the core cannot yet map will fail this test with
/// the offending kinds, prompting either Scala/Rust handling or a fixture change.
const COVERAGE_FIXTURE: &str = r#"<?php

namespace App\Demo;

use App\Other\Helper;

interface Greeter
{
    public function greet(string $name): string;
}

trait Loud
{
    public function shout(): string
    {
        return "LOUD";
    }
}

trait Quiet
{
    public function shout(): string
    {
        return "quiet";
    }

    public function whisper(): string
    {
        return "...";
    }
}

class Service implements Greeter
{
    use Loud, Quiet {
        Loud::shout insteadof Quiet;
        Quiet::shout as protected murmur;
    }

    const VERSION = "1.0";

    public function greet(string $name): string
    {
        $where = __CLASS__;
        $line = __LINE__;
        $message = <<<TEXT
Hello $name from {$where} at line $line
TEXT;
        return $message;
    }
}

function run(?Service $service): string
{
    if ($service === null) {
        return "none";
    } else {
        $names = ["a", "b", "c"];
        foreach ($names as $name) {
            $service->greet($name);
        }
        $kind = match (count($names)) {
            0 => "empty",
            default => "many",
        };
        $callback = function (string $value): string {
            return strtoupper($value);
        };
        try {
            $maybe = $service?->greet("x");
            return $callback($maybe ?? $kind);
        } catch (\Throwable $error) {
            return $error->getMessage();
        }
    }
}

__halt_compiler();
"#;

#[test]
fn coverage_fixture_lowers_with_zero_unmapped_nodes() {
    let dir = tempdir().expect("creating temp dir");
    let fixture = dir.path().join("coverage.php");
    fs::write(&fixture, COVERAGE_FIXTURE).expect("writing fixture");

    let assert = Command::cargo_bin("phpastgen")
        .expect("locating phpastgen binary")
        .arg(&fixture)
        .assert()
        .success();

    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(summary) = unmapped_summary_line(&stderr) {
        panic!(
            "coverage fixture produced unmapped tree-sitter nodes; the corpus is a coverage \
             gate, so either map these kinds or update the fixture.\n{summary}\nfull stderr:\n{stderr}"
        );
    }

    // Sanity check: the CLI must have emitted a JSON dump for the fixture.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("==> JSON dump:"),
        "expected a JSON dump in stdout, got:\n{stdout}"
    );
}

/// Returns the `phpastgen: … unmapped node(s): …` summary line if present.
fn unmapped_summary_line(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .find(|line| line.starts_with("phpastgen:") && line.contains("unmapped node(s)"))
}
