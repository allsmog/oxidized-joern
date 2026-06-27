package io.joern.javasrc2cpg

enum JavaParserBackend(val name: String) {
  case JavaParser extends JavaParserBackend("javaparser")
  case Oxidized   extends JavaParserBackend("oxidized")
}

object JavaParserBackend {
  def fromString(value: String): Option[JavaParserBackend] =
    JavaParserBackend.values.find(_.name.equalsIgnoreCase(value))
}
