enum NetworkError: Error {
  case timeout
}

func fetch(_ ok: Bool) throws -> Int {
  if ok {
    return 200
  }
  throw NetworkError.timeout
}

func run() {
  do {
    let code = try fetch(true)
    print(code)
  } catch NetworkError.timeout {
    print("timeout")
  } catch {
    print(error)
  }
}
