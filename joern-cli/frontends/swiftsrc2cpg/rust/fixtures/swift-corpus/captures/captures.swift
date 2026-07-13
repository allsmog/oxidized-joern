class Node {
  var next: Node?
  func attach() {
    let handler = { [weak self] in
      self?.attach()
    }
    handler()
  }
}
