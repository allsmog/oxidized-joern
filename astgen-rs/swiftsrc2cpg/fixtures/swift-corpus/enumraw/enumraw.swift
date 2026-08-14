enum Flags: Int {
  case none = 0
  case read = 1 << 0
  case write = 1 << 1
  case all = (1 << 0) | (1 << 1)
}

func bits(_ x: Int) -> Int {
  let masked = x & 0xFF
  let shifted = masked >> 2
  let combined = shifted ^ 0b1010
  return combined | 1
}
