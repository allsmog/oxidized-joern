package io.joern.jimple2cpg

enum JimpleParserBackend(val name: String) {
  case Soot     extends JimpleParserBackend("soot")
  case Oxidized extends JimpleParserBackend("oxidized")
}

object JimpleParserBackend {
  def fromString(value: String): Option[JimpleParserBackend] =
    JimpleParserBackend.values.find(_.name.equalsIgnoreCase(value))
}
