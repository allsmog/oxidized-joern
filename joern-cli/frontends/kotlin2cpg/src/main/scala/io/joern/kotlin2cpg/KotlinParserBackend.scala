package io.joern.kotlin2cpg

enum KotlinParserBackend(val name: String) {
  case KotlinCompiler extends KotlinParserBackend("kotlin-compiler")
  case Oxidized       extends KotlinParserBackend("oxidized")
}

object KotlinParserBackend {
  def fromString(value: String): Option[KotlinParserBackend] =
    KotlinParserBackend.values.find(_.name.equalsIgnoreCase(value))
}
