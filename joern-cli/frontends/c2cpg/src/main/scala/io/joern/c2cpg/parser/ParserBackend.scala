package io.joern.c2cpg.parser

enum ParserBackend {
  case Cdt, Oxidized
}

object ParserBackend {

  val Default: ParserBackend = ParserBackend.Cdt

  def fromString(value: String): Either[String, ParserBackend] = {
    value.trim.toLowerCase match {
      case "cdt"      => Right(ParserBackend.Cdt)
      case "oxidized" => Right(ParserBackend.Oxidized)
      case other      => Left(s"unsupported c2cpg parser backend '$other'; expected one of: cdt, oxidized")
    }
  }

}
