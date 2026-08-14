func swapValues(_ a: inout Int, _ b: inout Int) {
  let tmp = a
  a = b
  b = tmp
}

func increment(_ value: inout Int, by amount: Int) {
  value += amount
}
