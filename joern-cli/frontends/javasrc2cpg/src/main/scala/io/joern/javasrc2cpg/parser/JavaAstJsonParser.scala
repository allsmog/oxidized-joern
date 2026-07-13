package io.joern.javasrc2cpg.parser

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path}

object JavaAstJsonParser {

  final case class SourcePoint(line: Int, column: Int)

  final case class JavaAstNode(
    kind: String,
    fieldName: Option[String],
    named: Boolean,
    missing: Boolean,
    extra: Boolean,
    hasError: Boolean,
    startByte: Long,
    endByte: Long,
    start: SourcePoint,
    end: SourcePoint,
    code: String,
    children: List[JavaAstNode]
  ) {
    def descendants: List[JavaAstNode] = children ++ children.flatMap(_.descendants)
  }

  final case class JavaAstDocument(fullName: String, relativeName: String, ast: JavaAstNode)

  def parseFile(path: Path): JavaAstDocument =
    parseString(Files.readString(path, StandardCharsets.UTF_8))

  def parseString(content: String): JavaAstDocument =
    parse(ujson.read(content))

  def parse(value: ujson.Value): JavaAstDocument = {
    JavaAstDocument(
      fullName = stringField(value, "fullName"),
      relativeName = stringField(value, "relativeName"),
      ast = parseNode(value("ast"))
    )
  }

  private def parseNode(value: ujson.Value): JavaAstNode = {
    JavaAstNode(
      kind = stringField(value, "kind"),
      fieldName = optionalStringField(value, "fieldName"),
      named = boolField(value, "named"),
      missing = boolField(value, "missing"),
      extra = boolField(value, "extra"),
      hasError = boolField(value, "hasError"),
      startByte = longField(value, "startByte"),
      endByte = longField(value, "endByte"),
      start = parsePoint(value("start")),
      end = parsePoint(value("end")),
      code = stringField(value, "code"),
      children = value("children").arr.toList.map(parseNode)
    )
  }

  private def parsePoint(value: ujson.Value): SourcePoint =
    SourcePoint(line = intField(value, "line"), column = intField(value, "column"))

  private def stringField(value: ujson.Value, name: String): String = value(name).str

  private def optionalStringField(value: ujson.Value, name: String): Option[String] = {
    value.obj.get(name).collect { case ujson.Str(value) => value }
  }

  private def boolField(value: ujson.Value, name: String): Boolean = value(name).bool

  private def intField(value: ujson.Value, name: String): Int = value(name).num.toInt

  private def longField(value: ujson.Value, name: String): Long = value(name).num.toLong
}
