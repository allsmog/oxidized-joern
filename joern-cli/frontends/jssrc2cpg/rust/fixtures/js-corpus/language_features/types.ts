type Keys = keyof { a: number; b: string };
type Indexed = { a: number; b: string }["a"];
type Conditional<T> = T extends string ? number : boolean;
type Mapped<T> = { [K in keyof T]: T[K] };
interface Container<T extends object = {}> {
  value: T;
  read(): T;
}
enum Color {
  Red,
  Green = "green",
}
const label = `count=${1 + 2}`;
function tag(s: TemplateStringsArray, ...v: number[]): string {
  return s.join("") + v.length;
}
const tagged = tag`sum=${1}${2}`;
export { Color, label, tagged };
export type { Keys, Indexed, Conditional, Mapped, Container };
