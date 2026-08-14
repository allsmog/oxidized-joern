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
/// generic classes with constraints, generic methods, interfaces, delegates, records,
/// switch statements with pattern `case` arms and `when` guards, attributes
/// with positional and named arguments, collection/expression elements,
/// async/await, nullable types, LINQ-style lambdas, and properties.
const FIXTURE: &str = r#"#!/usr/bin/env dotnet-script
extern alias Legacy;
#define FEATURE_FLAG
#pragma warning disable CS0168
#nullable enable
#region LanguageFeatures
using System;
using System.Collections.Generic;
using System.Linq;
using static System.Math;
using TextAlias = System.String;
using System.Threading.Tasks;

[assembly: CLSCompliant(true)]

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

    public delegate TResult Projector<T, TResult>(T item) where T : class;

    public enum Status
    {
        Active,
        Inactive,
    }

    public interface IWorker
    {
        void Work();
        int Count { get; }
        int this[int index] { get; }
    }

    public class WorkerBase
    {
        public WorkerBase(int seed) { }
    }

    public class Worker(int seed) : WorkerBase(seed), IWorker
    {
        public Worker() : this(1) { }
        public Worker(string text) : base(text.Length) { }
        void IWorker.Work() { }
        int IWorker.Count => seed;
        int IWorker.this[int index] => index + seed;

        public void Guard(Action action)
        {
            try
            {
                action();
            }
            catch (InvalidOperationException ex) when (ex.Message != null)
            {
                Console.WriteLine(ex.Message);
            }
        }
    }

    [Tag("repo", Order = 1)]
    public class Repository<T> : IRepository<T> where T : class
    {
        private readonly List<T> _items = new() { };
        public delegate*<int, void> Callback;

        public IReadOnlyList<T> Items => _items;
        public event EventHandler? Changed;
        public event EventHandler? CustomChanged
        {
            add {}
            remove {}
        }

        public T this[int index]
        {
            get => _items[index];
            set { _items[index] = value; }
        }

        ~Repository()
        {
        }

        public static Repository<T> operator +(Repository<T> left, Repository<T> right) => left;
        public static explicit operator int(Repository<T> value) => value._items.Count;

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
            var merged = [0, .. codes, 4];
            Predicate<int> positive = delegate (int code) { return code > 0; };
            ref int firstCode = ref codes[0];
            firstCode = ref codes[1];
            var doubled = codes.Where(c => c > 1).Select(c => c * 2);
            var queried =
                from code in codes
                where code > 1
                select code;
            return projected.First();
        }

        public object TypeOperators(object value)
        {
            string? casted = value as string;
            bool ok = value is string;
            var typ = typeof(string);
            var size = sizeof(int);
            string text = default;
            var fallback = default(string);
            TypedReference typed = __makeref(value);
            var refType = __reftype(typed);
            var refValue = __refvalue(typed, object);
            Func<int> factory = () => throw new Exception();
            return ok ? casted ?? fallback : typ.Name;
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

        public int CountLocked()
        {
            lock (_items)
            {
                return _items.Count;
            }
        }

        public int CountChecked(int value)
        {
            if (value < 0)
                ;

            checked
            {
                value += 1;
            }

            unchecked
            {
                value -= 1;
            }

            return checked(value + 1);
        }

        public int CountUnsafe(int value)
        {
            unsafe
            {
                value += 1;
            }

            return value;
        }

        public int CountFixed(int[] values)
        {
            int total = 0;
            unsafe
            {
                fixed (int* p = values)
                {
                    total += 1;
                }
            }

            return total;
        }

        public int CountStackAlloc()
        {
            int total = 0;
            unsafe
            {
                int* values = stackalloc int[3];
                total += *values;
            }

            return total;
        }

        public int CountScoped(scoped Span<int> values)
        {
            scoped ref int first = ref values[0];
            return first;
        }

        public int CountRange(int[] values)
        {
            var last = values[^1];
            var middle = values[1..^1];
            return last + middle.Length;
        }

        public int CountTuple()
        {
            var pair = (a: 1, b: 2);
            var (left, right) = pair;
            return pair.a + pair.b + left + right;
        }

        public (int a, int b) EchoTuple((int a, int b) pair)
        {
            (string name, int count) local = ("x", 1);
            return pair;
        }

        public int CountSwitchExpression(int value)
        {
            return value switch
            {
                (> 10) => 3,
                > 0 and < 10 => 1,
                0 or 10 => 2,
                _ => 0,
            };
        }

        public int CountListPattern(int[] values)
        {
            return values switch
            {
                [1, 2] => 1,
                [1, ..] => 2,
                [] => 0,
                _ => -1,
            };
        }

        public int CountRecursivePattern(string text)
        {
            var pair = (1, 2);
            var property = text is { Length: > 3 };
            return pair switch
            {
                (1, > 0) => property ? 1 : 0,
                (_, int) => 3,
                (_, _) => 2,
                _ => -1,
            };
        }

        public Person WithAge(Person person)
        {
            return person with { Age = 2 };
        }

        public IEnumerable<int> CountYield(int value)
        {
            yield return value;
            yield break;
        }
    }
}
#if FEATURE_FLAG
public class PreprocessorEnabled
{
    public void M()
    {
        int value = 1;
    }
}
#else
#error disabled
#endif
#endregion
#undef FEATURE_FLAG
#line 200 "Generated.cs"
#warning generated
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
    let documents = json_files
        .iter()
        .map(|path| fs::read_to_string(path).expect("reading JSON output"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !documents.contains("\"Kind\":\"ast.Unknown\""),
        "fixture emitted ast.Unknown fallback nodes\nstdout:\n{stdout}\nstderr:\n{stderr}\njson:\n{documents}"
    );
    assert!(
        documents.contains("\"Kind\":\"ast.IndexExpression\"")
            && documents.contains("\"Alias\"")
            && documents.contains("\"Kind\":\"ast.RangeExpression\"")
            && documents.contains("\"Kind\":\"ast.TupleExpression\"")
            && documents.contains("\"Kind\":\"ast.TupleType\"")
            && documents.contains("\"Kind\":\"ast.SwitchExpression\"")
            && documents.contains("\"Kind\":\"ast.AndPattern\"")
            && documents.contains("\"Kind\":\"ast.OrPattern\"")
            && documents.contains("\"Kind\":\"ast.ParenthesizedPattern\"")
            && documents.contains("\"Kind\":\"ast.ListPattern\"")
            && documents.contains("\"Kind\":\"ast.RecursivePattern\"")
            && documents.contains("\"Kind\":\"ast.TuplePattern\"")
            && documents.contains("\"Kind\":\"ast.TypePattern\"")
            && documents.contains("\"Kind\":\"ast.WithExpression\"")
            && documents.contains("\"Kind\":\"ast.StackAllocExpression\"")
            && documents.contains("\"Kind\":\"ast.IndirectionExpression\"")
            && documents.contains("\"Kind\":\"ast.QueryExpression\"")
            && documents.contains("\"Kind\":\"ast.AsExpression\"")
            && documents.contains("\"Kind\":\"ast.IsExpression\"")
            && documents.contains("\"Kind\":\"ast.TypeOfExpression\"")
            && documents.contains("\"Kind\":\"ast.SizeOfExpression\"")
            && documents.contains("\"Kind\":\"ast.DefaultExpression\"")
            && documents.contains("\"Kind\":\"ast.ThrowExpression\"")
            && documents.contains("\"Kind\":\"ast.ShebangDirective\"")
            && documents.contains("\"Kind\":\"ast.PreprocessorDirective\"")
            && documents.contains("\"Kind\":\"ast.PreprocessorIfDirective\"")
            && documents.contains("\"Kind\":\"ast.PreprocessorElseDirective\"")
            && documents.contains("\"Kind\":\"ast.ExternAliasDirective\"")
            && documents.contains("\"Kind\":\"ast.GlobalAttribute\"")
            && documents.contains("\"Kind\":\"ast.PrimaryConstructorBaseType\"")
            && documents.contains("\"Kind\":\"ast.ThisConstructorInitializer\"")
            && documents.contains("\"Kind\":\"ast.BaseConstructorInitializer\"")
            && documents.contains("\"Kind\":\"ast.ExplicitInterfaceSpecifier\"")
            && documents.contains("\"Kind\":\"ast.CatchFilterClause\"")
            && documents.contains("\"Kind\":\"ast.EventFieldDeclaration\"")
            && documents.contains("\"Kind\":\"ast.EventDeclaration\"")
            && documents.contains("\"Kind\":\"ast.IndexerDeclaration\"")
            && documents.contains("\"Kind\":\"ast.DestructorDeclaration\"")
            && documents.contains("\"Kind\":\"ast.OperatorDeclaration\"")
            && documents.contains("\"Kind\":\"ast.ConversionOperatorDeclaration\"")
            && documents.contains("\"Kind\":\"ast.AnonymousMethodExpression\"")
            && documents.contains("\"Kind\":\"ast.SpreadElement\"")
            && documents.contains("\"Kind\":\"ast.RefType\"")
            && documents.contains("\"Kind\":\"ast.ScopedType\"")
            && documents.contains("\"Kind\":\"ast.RefExpression\"")
            && documents.contains("\"Kind\":\"ast.MakeRefExpression\"")
            && documents.contains("\"Kind\":\"ast.RefTypeExpression\"")
            && documents.contains("\"Kind\":\"ast.RefValueExpression\""),
        "fixture did not exercise C# index/range/tuple expression/tuple type/switch/and/or/parenthesized/list/recursive/tuple-designation/type-pattern/with/stackalloc/indirection/query/as/is/typeof/sizeof/default/throw/shebang/preprocessor/extern-alias/global-attribute/event/indexer/destructor/operator/conversion/anonymous-method/spread/ref expressions/ref intrinsics\njson:\n{documents}"
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
