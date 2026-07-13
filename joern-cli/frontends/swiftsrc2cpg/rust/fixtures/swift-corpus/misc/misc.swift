import Foundation

infix operator <+>: AdditionPrecedence

func <+> (lhs: Int, rhs: Int) -> Int {
  return lhs + rhs
}

class Resource {
  deinit {
    cleanup()
  }
  func cleanup() {}
}

#if DEBUG
let mode = "debug"
#else
let mode = "release"
#endif
