@dynamicMemberLookup
struct Wrapper {
  subscript(dynamicMember key: String) -> Int {
    return 0
  }
  subscript(row: Int, col: Int) -> Int {
    return row + col
  }
}

func captures() {
  var total = 0
  let add: (Int) -> Void = { [total] x in
    print(total + x)
  }
  add(1)
}

struct Model {
  var name: String = ""
}

let nameKey = \Model.name
let counts = [1, 2, 3].map(\.description)
