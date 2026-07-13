func first(_ values: [Int]!) -> Int {
  return values![0]
}

func describe(_ name: String!, _ tags: [String]?) -> Int {
  return name.count + (tags?.count ?? 0)
}

func dictionary(_ map: [String: Int]!) -> Int {
  return map.count
}
