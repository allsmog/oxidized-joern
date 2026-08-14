infix operator |>: AdditionPrecedence
prefix operator !!!
postfix operator ^^^

func |> (lhs: Int, rhs: Int) -> Int {
  return lhs + rhs
}

prefix func !!! (value: Bool) -> Bool {
  return !value
}

postfix func ^^^ (value: Int) -> Int {
  return value * value
}
