func sums(_ values: [Int]) -> Int {
  return values.reduce(0, +)
}

func products(_ values: [Int]) -> Int {
  return values.reduce(1, *)
}

func sortedDescending(_ values: [Int]) -> [Int] {
  return values.sorted(by: >)
}
