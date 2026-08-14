struct Legacy {
  @available(*, deprecated, message: "use modern() instead")
  func legacy() {}

  @available(*, deprecated, renamed: "current()")
  func old() {}

  @available(*, unavailable)
  func banned() {}

  @available(iOS 13.0, macOS 10.15, *)
  func modern() {}
}
