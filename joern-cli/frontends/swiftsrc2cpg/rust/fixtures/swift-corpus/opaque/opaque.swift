protocol Shape {
  func area() -> Double
}

func makeShape() -> some Shape {
  return Circle()
}

func describe(_ shape: any Shape) -> Double {
  return shape.area()
}

struct Circle: Shape {
  func area() -> Double {
    return 3.14
  }
}
