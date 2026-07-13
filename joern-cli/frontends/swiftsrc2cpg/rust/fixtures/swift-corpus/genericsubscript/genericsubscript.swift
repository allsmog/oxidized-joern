struct Container {
  private var storage: [Int] = []

  subscript<Index>(_ index: Index) -> Int where Index: BinaryInteger {
    get {
      return storage[Int(index)]
    }
    set {
      storage[Int(index)] = newValue
    }
  }

  subscript<T, U>(_ first: T, _ second: U) -> Int {
    return 0
  }
}
