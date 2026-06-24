@objc
class Controller {
  @discardableResult
  func run() -> Int {
    return 0
  }
}

@frozen
public struct Point {
  let x: Int
}

@propertyWrapper
struct Clamped {
  var wrappedValue: Int
}
