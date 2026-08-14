struct API {
  @available(iOS, introduced: 13.0, deprecated: 15.0, message: "use v2")
  func legacy() {}

  @available(macOS, introduced: 10.15)
  func mac() {}

  @available(iOS, obsoleted: 16.0, renamed: "modern()")
  func removed() {}
}
