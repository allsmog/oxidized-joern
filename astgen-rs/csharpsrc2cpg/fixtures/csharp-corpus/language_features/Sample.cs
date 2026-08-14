// Multi-construct C# fixture for the dotnetastgen differential harness.
//
// It mirrors the breadth of the inline coverage fixture (namespaces, generic
// classes with constraints, generic methods, interfaces, delegates, records, enums,
// attributes with positional and named arguments, switch statements with
// pattern `case` arms and `when` guards, async/await, nullable types, LINQ-style
// lambdas, and properties) so a reference `dotnetastgen` and this crate's CLI
// can be compared node-for-node.
extern alias Legacy;
#define FEATURE_FLAG
#pragma warning disable CS0168
#nullable enable
#region LanguageFeatures
using System;
using System.Collections.Generic;
using System.Linq;
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
                total += 1;
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
