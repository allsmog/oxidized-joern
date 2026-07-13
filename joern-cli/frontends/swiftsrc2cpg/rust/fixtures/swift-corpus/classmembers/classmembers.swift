class Counter {
  class func reset() {}
  static func shared() -> Counter { return Counter() }

  class var defaultValue: Int {
    return 0
  }

  final class func sealed() {}
}
