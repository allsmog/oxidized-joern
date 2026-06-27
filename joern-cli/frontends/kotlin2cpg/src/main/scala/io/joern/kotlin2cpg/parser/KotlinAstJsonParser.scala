package io.joern.kotlin2cpg.parser

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path}

final case class KotlinSourcePoint(line: Int, column: Int)

final case class KotlinAstNode(
  kind: String,
  fieldName: Option[String],
  named: Boolean,
  missing: Boolean,
  extra: Boolean,
  hasError: Boolean,
  startByte: Int,
  endByte: Int,
  start: KotlinSourcePoint,
  end: KotlinSourcePoint,
  code: String,
  children: List[KotlinAstNode]
) {
  def descendants: List[KotlinAstNode] = children ++ children.flatMap(_.descendants)
}

final case class KotlinAstDocument(fullName: String, relativeName: String, ast: KotlinAstNode)

object KotlinAstJsonParser {

  def parseFile(path: Path): KotlinAstDocument =
    parse(Files.readString(path, StandardCharsets.UTF_8))

  def parse(content: String): KotlinAstDocument = {
    val value = ujson.read(content)
    KotlinAstDocument(
      fullName = value("fullName").str,
      relativeName = value("relativeName").str,
      ast = parseNode(value("ast"))
    )
  }

  private def parseNode(value: ujson.Value): KotlinAstNode =
    KotlinAstNode(
      kind = value("kind").str,
      fieldName = optionalString(value, "fieldName"),
      named = value("named").bool,
      missing = value("missing").bool,
      extra = value("extra").bool,
      hasError = value("hasError").bool,
      startByte = value("startByte").num.toInt,
      endByte = value("endByte").num.toInt,
      start = parsePoint(value("start")),
      end = parsePoint(value("end")),
      code = value("code").str,
      children = value("children").arr.toList.map(parseNode)
    )

  private def parsePoint(value: ujson.Value): KotlinSourcePoint =
    KotlinSourcePoint(line = value("line").num.toInt, column = value("column").num.toInt)

  private def optionalString(value: ujson.Value, key: String): Option[String] = {
    value.obj.get(key).flatMap {
      case ujson.Null => None
      case item       => Some(item.str)
    }
  }
}
