function source(value) {
  return value;
}

function transform(value) {
  return value;
}

function sink(value) {
  console.log(value);
}

function main(user) {
  const raw = source(user);
  const clean = transform(raw);
  sink(clean);
}
