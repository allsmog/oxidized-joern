enum NetworkError: Error {
  case offline
}

func handle() {
  do {
    try perform()
  } catch let error as NetworkError {
    print(error)
  } catch {
    print("other")
  }
}

func perform() throws {}
