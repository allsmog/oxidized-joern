package func shared() {}

public struct Counter {
  private(set) public var count = 0
  internal(set) var name = ""
}

nonisolated func free() {}
