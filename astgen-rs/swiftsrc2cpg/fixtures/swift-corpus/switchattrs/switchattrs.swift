enum Direction {
  case north
  case south
}

func describe(_ d: Direction) -> Int {
  switch d {
  case .north:
    fallthrough
  case .south:
    return 1
  @unknown default:
    return 0
  }
}
