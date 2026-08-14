func makeTuple<each Element>(_ values: repeat each Element) -> (repeat each Element) {
  return (repeat each values)
}

func zip<each First, each Second>(_ first: repeat each First, _ second: repeat each Second) {
}

struct Variadic<each T> {
  let values: (repeat each T)
}
