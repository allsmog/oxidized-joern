func demo(_ x: Int?) -> Int {
  guard let value = x else {
    return 0
  }
  let y: Int? = nil
  let z = y ?? value
  let forced = x!
  return z + forced
}

func chain(_ s: String?) -> Int? {
  return s?.count
}
