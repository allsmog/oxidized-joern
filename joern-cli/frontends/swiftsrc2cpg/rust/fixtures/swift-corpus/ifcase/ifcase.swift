enum Token {
  case number
  case word
}

func describe(_ t: Token) -> Int {
  if case .number = t {
    return 1
  }
  guard case .word = t else {
    return 0
  }
  return 2
}
