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

func allSorted<C: Collection>(_ collection: C) -> Bool where C.Element: Comparable {
  return collection.count >= 0
}

struct Box<T> where T: Equatable {
  let value: T
}
