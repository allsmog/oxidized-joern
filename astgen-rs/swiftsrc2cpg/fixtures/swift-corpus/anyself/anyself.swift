struct Box {
  func clone() -> Self {
    return self
  }
  var anything: Any = 0
  let values: [Any] = []
}

func erase(_ x: Int) -> Any {
  return x
}
