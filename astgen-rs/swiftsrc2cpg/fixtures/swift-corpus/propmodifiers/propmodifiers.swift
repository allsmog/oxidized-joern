class Cache {
  static let shared = Cache()
  lazy var data: [Int] = []
  weak var delegate: AnyObject?
  unowned let owner: AnyObject

  init(_ o: AnyObject) {
    owner = o
  }
}
