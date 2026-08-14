protocol Container {
  associatedtype Item where Item: Equatable
  associatedtype Iterator: Sequence = [Item]
  func first() -> Item?
}
