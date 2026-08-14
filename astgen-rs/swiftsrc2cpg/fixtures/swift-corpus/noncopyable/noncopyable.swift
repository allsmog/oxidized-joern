struct FileHandle: ~Copyable {
  let fd: Int

  consuming func close() {
    print(fd)
  }

  borrowing func peek() -> Int {
    return fd
  }
}

func consume(_ handle: consuming FileHandle) {
  handle.close()
}

enum Resource: ~Copyable {
  case open(Int)
  case closed
}
