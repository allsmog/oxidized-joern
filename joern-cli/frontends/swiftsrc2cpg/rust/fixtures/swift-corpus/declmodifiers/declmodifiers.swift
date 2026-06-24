public struct Service {
  private let id: Int
  fileprivate var cache: [String: Int] = [:]
  static let shared = Service(id: 0)

  public init(id: Int) {
    self.id = id
  }

  typealias Handler = (Int) -> Void

  struct Inner {
    let value: Int
  }

  static func make() -> Service {
    return Service(id: 1)
  }
}
