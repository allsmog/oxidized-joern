protocol Container<Element> {
  associatedtype Element
  func first() -> Element?
}

protocol Mapping<Key, Value>: Collection {
  associatedtype Key
  associatedtype Value
}
