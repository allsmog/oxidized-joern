let noop: () -> Void = {}
let ignore: (Int) -> Void = { _ in }
let ignoreTwo: (Int, Int) -> Void = { _, _ in }

func register(_ handler: () -> Void) {
  handler()
}

func setup() {
  register {
  }
}
