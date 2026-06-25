func consume(_ stream: AsyncStream<Int>) async {
  for await value in stream {
    print(value)
  }
}

func sumUp(_ stream: AsyncStream<Int>) async -> Int {
  var total = 0
  for await x in stream {
    total += x
  }
  return total
}
