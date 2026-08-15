function source(value: string): string {
  return value;
}

function transform(value: string): string {
  return value;
}

function sink(value: string): void {
  console.log(value);
}

function main(user: string): void {
  const raw = source(user);
  const clean = transform(raw);
  sink(clean);
}
