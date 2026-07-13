func choose(_ flag: Bool, _ a: Int, _ b: Int) -> Int {
  return flag ? a : b
}

func chained(_ s: String?) -> Int {
  return s?.uppercased().count ?? 0
}

func ranges() -> Int {
  let a = (0..<10)
  let b = (0...10)
  return a.count + b.count
}
