abstract class Repository<T> {
  private items: T[] = [];
  constructor(public readonly name: string) {}
  async load(id?: number): Promise<T> {
    return await Promise.resolve(this.items[id ?? 0]);
  }
  *stream(other: Repository<T>): Generator<T> {
    yield* other.stream(other);
  }
}
const transform = <U,>({ value, ...rest }: { value: U }, ...extra: U[]): U[] => [value, ...extra];
function probe(o?: { run?: () => number }): number | null {
  return o?.run?.() ?? null;
}
export { Repository, transform, probe };
