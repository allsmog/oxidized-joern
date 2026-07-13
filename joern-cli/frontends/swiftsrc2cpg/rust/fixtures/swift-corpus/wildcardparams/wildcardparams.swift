let ignoreFirst: (Int, Int) -> Int = { _, y in
  return y
}

let ignoreAll: (Int) -> String = { _ in
  return "x"
}

let typed: (Int) -> Int = { (_: Int) in
  return 0
}
