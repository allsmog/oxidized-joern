package io.joern.pysrc2cpg

enum PythonParserBackend(val name: String) {
  case JavaCc   extends PythonParserBackend("javacc")
  case Oxidized extends PythonParserBackend("oxidized")
}

object PythonParserBackend {

  def fromString(value: String): Option[PythonParserBackend] = {
    val normalized = value.trim.toLowerCase
    PythonParserBackend.values.find(_.name == normalized)
  }
}
