protocol Container {
  associatedtype Item
  var count: Int { get }
  mutating func append(_ item: Item)
  subscript(i: Int) -> Item { get }
}

protocol Named {
  var name: String { get set }
  func greeting() -> String
}

extension Named {
  func greeting() -> String {
    return "Hello, \(name)"
  }
}
