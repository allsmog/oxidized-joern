func demo() -> Int {
  let add = { (a: Int, b: Int) -> Int in
    return a + b
  }
  return add(1, 2)
}

func shorthandDemo() -> Int {
  let combine = { acc, x in
    return acc + x
  }
  return combine(1, 2)
}

class Worker {
  func setup() {
    let task = { [weak self] in
      print("done")
    }
    task()
  }
}

func discardDemo() {
  let value = 1
  _ = value
}
