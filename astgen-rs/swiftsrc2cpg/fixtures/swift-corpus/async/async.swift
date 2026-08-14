func fetchValue() async -> Int {
  return 42
}

func caller() async throws -> Int {
  let value = await fetchValue()
  return value
}

actor Counter {
  var count = 0

  func increment() {
    count += 1
  }
}
