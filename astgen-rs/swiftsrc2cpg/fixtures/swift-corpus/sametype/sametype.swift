func identical<T, U>(_ x: T, _ y: U) -> Bool where T == U {
  return true
}

func mixed<S: Sequence, T>(_ s: S, _ t: T) where S.Element == T, T: Equatable {
  _ = s
}

protocol Wrapper {
  associatedtype Wrapped
  func unwrap() -> Wrapped
}

extension Wrapper where Wrapped == Int {
  func double() -> Int {
    return unwrap() * 2
  }
}
