func freestanding() {
  let url = #URL("https://example.com")
  print(url)
}

func concurrentLoad() async {
  async let a = fetch(1)
  async let b = fetch(2)
  let total = await a + b
  print(total)
}

func fetch(_ id: Int) async -> Int {
  return id
}

func labeled() {
  outer: for i in 0..<3 {
    for j in 0..<3 {
      if i == j {
        continue outer
      }
      break outer
    }
  }
}
