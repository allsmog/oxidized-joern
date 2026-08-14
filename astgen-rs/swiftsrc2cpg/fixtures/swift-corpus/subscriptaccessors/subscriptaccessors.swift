struct Grid {
  private var storage: [Int] = []
  subscript(index: Int) -> Int {
    get {
      return storage[index]
    }
    set {
      storage[index] = newValue
    }
  }
}
