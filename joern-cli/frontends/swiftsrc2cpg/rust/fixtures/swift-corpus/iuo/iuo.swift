class Node {
  var next: Node!
  var value: Int = 0

  func chain() -> Int {
    return next!.next!.value
  }
}

class View {
  var label: String!
  weak var delegate: AnyObject!
}
