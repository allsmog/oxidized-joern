let handler: @Sendable (Int) -> Int = { $0 }

func register(_ callback: @Sendable @escaping () -> Void) {
  callback()
}

func makeSendable() -> @Sendable () -> Void {
  return {}
}
