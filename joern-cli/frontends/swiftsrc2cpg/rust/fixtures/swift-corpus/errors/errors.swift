enum NetworkError: Error {
  case timeout
}

func fetch(_ ok: Bool) throws -> Int {
  if ok {
    return 200
  }
  throw NetworkError.timeout
}
