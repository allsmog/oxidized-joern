struct Matrix {
  var grid: [Double]
  let columns: Int

  subscript(row: Int, column: Int) -> Double {
    get {
      return grid[row * columns + column]
    }
    set {
      grid[row * columns + column] = newValue
    }
  }
}

struct Wrapper {
  subscript(index: Int) -> Int {
    return index * 2
  }
}
