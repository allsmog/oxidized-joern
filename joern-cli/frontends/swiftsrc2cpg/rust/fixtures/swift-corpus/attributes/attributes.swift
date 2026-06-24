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

struct Config {
  @Clamped(0)
  var level: Int = 0

  @Clamped(min: 0, max: 10)
  var ranged: Int = 0
}
