func firstIndex(_ values: [Int], _ target: Int) -> Int? {
  for (index, item) in values.enumerated() where item == target {
    return index
  }
  return nil
}
