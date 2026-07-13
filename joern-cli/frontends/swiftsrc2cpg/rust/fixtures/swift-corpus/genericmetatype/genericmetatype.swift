let intType = Optional<Int>.self
let arrayType = Array<String>.self
let dictType = Dictionary<String, Int>.self

func describe() {
  print(Optional<Int>.self)
  let none = Optional<Int>.none
  _ = none
}
