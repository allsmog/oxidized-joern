@objc
class Controller {
  @discardableResult
  func run() -> Int {
    return 0
  }
}

@objc(MyController)
class NamedController {
  @objc(doThing:with:)
  func doThing(_ a: Int, with b: Int) {}
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
