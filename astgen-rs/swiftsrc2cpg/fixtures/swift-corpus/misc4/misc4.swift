protocol Shape {
  func area() -> Double
}

func makeShape() -> some Shape {
  return Circle()
}

func describe(_ s: any Shape) -> Double {
  return s.area()
}

struct Circle: Shape {
  func area() -> Double { return 3.14 }
}

func process<T>(_ items: [T]) throws -> T where T: Comparable {
  return items[0]
}

func loadAll() async throws -> [Int] {
  return []
}

func rethrowing(_ f: () throws -> Int) rethrows -> Int {
  return try f()
}
