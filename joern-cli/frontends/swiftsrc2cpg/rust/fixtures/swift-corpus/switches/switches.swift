func classify(_ value: Int) -> String {
  switch value {
  case 0:
    return "zero"
  case 1, 2, 3:
    return "small"
  case let n where n < 0:
    return "negative"
  default:
    return "large"
  }
}
