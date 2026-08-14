enum Token {
  case number(Int)
  case word(String)
}

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

func describe(_ point: (Int, Int)) -> String {
  switch point {
  case (0, 0):
    return "origin"
  case (let x, 0):
    return "axis"
  case (let x, let y):
    return "at"
  }
}

func token(_ t: Token) -> Int {
  switch t {
  case .number(let n):
    return n
  case .word(let w):
    return w.count
  }
}
