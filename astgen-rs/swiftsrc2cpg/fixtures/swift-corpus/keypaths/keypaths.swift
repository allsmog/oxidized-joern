struct Person {
  var name: String
  var age: Int
}

let nameKeyPath = \Person.name
let ageKeyPath = \Person.age

func names(_ people: [Person]) -> [String] {
  return people.map(\.name)
}
