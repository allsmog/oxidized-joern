func first(_ a: [Int]?) -> Int? {
  return a?[0]
}

func nested(_ m: [String: [Int]]?) -> Int? {
  return m?["key"]?[0]
}

func plain(_ a: [Int]) -> Int {
  return a[0]
}
