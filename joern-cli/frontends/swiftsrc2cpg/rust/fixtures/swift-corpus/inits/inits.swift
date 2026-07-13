class Base {
  required init() {}
  convenience init(value: Int) {
    self.init()
  }
}

final class Derived: Base {
  override init() {
    super.init()
  }
}
