//! Coverage harness: every tree-sitter node kind exercised by the inline C#
//! fixture below must map to a dedicated AST node. The CLI prints a single
//! `dotnetastgen: N unmapped node(s): …` line on stderr when any kind falls
//! through to `Unknown`; this test fails loudly and lists the offending kinds
//! so regressions in mapping coverage are caught immediately.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Broad C# surface intended to drive as many distinct tree-sitter node kinds
/// through the emitter as a single compilation unit reasonably can: namespaces,
/// generic classes with constraints, generic methods, interfaces, records,
/// switch statements with pattern `case` arms and `when` guards, attributes
/// with positional and named arguments, collection/expression elements,
/// async/await, nullable types, LINQ-style lambdas, and properties.
const FIXTURE: &str = r#"
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

[AttributeUsage(AttributeTargets.Class)]
public sealed class TagAttribute : Attribute
{
    public TagAttribute(string name) => Name = name;
    public string Name { get; }
    public int Order { get; set; }
}

namespace Acme.Sample
{
    public interface IRepository<T> where T : class
    {
        Task<T?> FindAsync(int id);
    }

    public record Person(string FirstName, string? LastName)
    {
        public int Age { get; init; }
    }

    public enum Status
    {
        Active,
        Inactive,
    }

    [Tag("repo", Order = 1)]
    public class Repository<T> : IRepository<T> where T : class
    {
        private readonly List<T> _items = new() { };

        public IReadOnlyList<T> Items => _items;

        public async Task<T?> FindAsync(int id)
        {
            await Task.Delay(1);
            return _items.FirstOrDefault();
        }

        public TResult Map<TResult>(Func<T, TResult> selector)
            where TResult : notnull
        {
            var projected = _items.Select(item => selector(item)).ToList();
            int[] codes = new[] { 1, 2, 3 };
            var doubled = codes.Where(c => c > 1).Select(c => c * 2);
            return projected.First();
        }

        public string Describe(Status status)
        {
            switch (status)
            {
                case Status.Active:
                    return "active";
                case Status s when s == Status.Inactive:
                    return "inactive";
                default:
                    return "unknown";
            }
        }
    }
}
"#;

#[test]
fn fixture_produces_no_unmapped_nodes() {
    let tmp = tempfile::tempdir().expect("creating temp dir");
    let input = tmp.path().join("input");
    fs::create_dir_all(&input).expect("creating input dir");
    fs::write(input.join("Sample.cs"), FIXTURE).expect("writing fixture");
    let out = tmp.path().join("out");

    let output = run_cli(&input, &out);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "dotnetastgen exited with failure\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // The CLI emits exactly one JSON document per source file; make sure the
    // run actually produced output rather than silently skipping the fixture.
    let mut json_files = Vec::new();
    collect_json_files(&out, &mut json_files);
    assert!(
        !json_files.is_empty(),
        "fixture produced no JSON output\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    if let Some(summary) = unmapped_summary(&stderr) {
        panic!(
            "fixture exercised tree-sitter node kinds that fall through to `Unknown`:\n{summary}\n\
             Add dedicated mappings in dotnetastgen-core, or narrow the fixture if the kind is \
             intentionally unsupported.\nfull stderr:\n{stderr}"
        );
    }
}

/// Returns the `dotnetastgen: N unmapped node(s): …` summary line if the CLI
/// printed one, otherwise `None`.
fn unmapped_summary(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find(|line| line.starts_with("dotnetastgen: ") && line.contains("unmapped node(s)"))
        .map(str::to_string)
}

fn run_cli(input: &Path, out: &Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotnetastgen"));
    command.arg("--input").arg(input).arg("--out").arg(out);
    command.output().expect("running dotnetastgen")
}

fn collect_json_files(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    for entry in
        fs::read_dir(root).unwrap_or_else(|err| panic!("reading {}: {err}", root.display()))
    {
        let entry = entry.expect("reading directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            out.push(path);
        }
    }
    out.sort();
}
