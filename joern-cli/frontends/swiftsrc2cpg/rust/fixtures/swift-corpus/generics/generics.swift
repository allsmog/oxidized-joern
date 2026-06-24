struct Stack<Element> {
  private var items: [Element] = []

  mutating func push(_ item: Element) {
    items.append(item)
  }

  mutating func pop() -> Element? {
    return items.popLast()
  }
}

func identity<T>(_ value: T) -> T {
  return value
}

func firstOf<T: Equatable>(_ values: [T]) -> T? {
  return values.first
}
