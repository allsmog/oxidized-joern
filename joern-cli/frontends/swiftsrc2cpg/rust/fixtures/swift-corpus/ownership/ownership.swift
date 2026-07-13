func consume(_ x: consuming String) -> String {
  return x
}

func borrow(_ x: borrowing String) -> Int {
  return x.count
}
