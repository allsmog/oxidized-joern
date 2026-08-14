enum Token {
  case number(Int)
  case word(String)
}

func describe(_ t: Token) -> Int {
  if case .number(let n) = t {
    return n
  }
  guard case .word(let w) = t else {
    return 0
  }
  return w.count
}
