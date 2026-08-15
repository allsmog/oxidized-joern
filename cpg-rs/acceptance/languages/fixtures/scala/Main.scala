object Main {
  def source(value: String): String = value
  def transform(value: String): String = value
  def sink(value: String): Unit = println(value)

  def main(user: String): Unit = {
    val raw = source(user)
    val clean = transform(raw)
    sink(clean)
  }
}
