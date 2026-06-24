func demo() -> Int {
  let add = { (a: Int, b: Int) -> Int in
    return a + b
  }
  return add(1, 2)
}
