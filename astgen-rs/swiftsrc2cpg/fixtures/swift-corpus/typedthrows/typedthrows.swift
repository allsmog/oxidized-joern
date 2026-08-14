enum NetworkError: Error {
  case offline
}

func fetch() throws(NetworkError) -> Int {
  return 0
}

func load() async throws(NetworkError) -> [Int] {
  return []
}

func plain() throws -> Int {
  return 1
}

func rethrowing(_ f: () throws -> Int) rethrows -> Int {
  return try f()
}
