// Multi-construct C# fixture for the dotnetastgen differential harness.
//
// It mirrors the breadth of the inline coverage fixture (namespaces, generic
// classes with constraints, generic methods, interfaces, records, enums,
// attributes with positional and named arguments, switch statements with
// pattern `case` arms and `when` guards, async/await, nullable types, LINQ-style
// lambdas, and properties) so a reference `dotnetastgen` and this crate's CLI
// can be compared node-for-node.
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
