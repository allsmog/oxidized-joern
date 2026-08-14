typealias Handler = (Int) -> Void

actor Counter {
  var value = 0
  func increment() {
    value += 1
  }
}

func sum(_ numbers: Int..., scale: Int = 1) -> Int {
  var total = 0
  for n in numbers {
    total += n * scale
  }
  return total
}

extension Collection where Element: Equatable {
  func hasDuplicates() -> Bool {
    return false
  }
}
