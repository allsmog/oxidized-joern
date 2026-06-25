struct Store {
  var value: Int {
    get throws {
      return 0
    }
  }
}

actor Cache {
  var entry: Int {
    get async {
      return 1
    }
  }

  var combined: Int {
    get async throws {
      return 2
    }
  }
}
