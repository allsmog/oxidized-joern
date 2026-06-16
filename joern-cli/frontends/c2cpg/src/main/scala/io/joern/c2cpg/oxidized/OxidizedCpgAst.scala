package io.joern.c2cpg.oxidized

import ujson.Value

case class OxDocument(path: String, declarations: Seq[OxDeclaration])

sealed trait OxDeclaration {
  def name: String
  def code: String
  def line: Int
}

case class OxMacroDecl(name: String, code: String, line: Int, parameters: Seq[String], body: String)
    extends OxDeclaration

case class OxStructDecl(name: String, code: String, line: Int, fields: Seq[OxFieldDecl]) extends OxDeclaration

case class OxFieldDecl(name: String, typeName: String, code: String)

case class OxEnumDecl(name: String, code: String, line: Int, variants: Seq[OxEnumVariant]) extends OxDeclaration

case class OxEnumVariant(name: String, value: Option[String], code: String)

case class OxFunctionDecl(
  name: String,
  returnType: String,
  signature: String,
  code: String,
  line: Int,
  parameters: Seq[OxParameterDecl],
  body: Seq[OxStatement]
) extends OxDeclaration

case class OxParameterDecl(name: String, typeName: String, code: String, line: Int)

sealed trait OxStatement {
  def code: String
  def line: Int
}

case class OxLocalDecl(name: String, typeName: String, code: String, line: Int, initializer: Option[OxExpression])
    extends OxStatement

case class OxAssignment(code: String, line: Int, left: OxExpression, right: OxExpression) extends OxStatement

case class OxReturn(code: String, line: Int, expression: Option[OxExpression]) extends OxStatement

case class OxIf(
  code: String,
  line: Int,
  condition: OxExpression,
  thenBody: Seq[OxStatement],
  elseBody: Seq[OxStatement]
) extends OxStatement

case class OxWhile(code: String, line: Int, condition: OxExpression, body: Seq[OxStatement]) extends OxStatement

case class OxExpressionStatement(code: String, line: Int, expression: OxExpression) extends OxStatement

sealed trait OxExpression {
  def code: String
  def line: Int
}

case class OxIdentifier(name: String, code: String, line: Int) extends OxExpression

case class OxLiteral(value: String, code: String, line: Int) extends OxExpression

case class OxBinary(operator: String, code: String, line: Int, left: OxExpression, right: OxExpression)
    extends OxExpression

case class OxCall(name: String, code: String, line: Int, arguments: Seq[OxExpression]) extends OxExpression

case class OxFieldAccess(field: String, code: String, line: Int, base: OxExpression) extends OxExpression

object OxDocument {

  def fromJson(json: String): OxDocument = {
    val value = ujson.read(json)
    OxDocument(path = str(value, "path"), declarations = value("declarations").arr.map(declaration).toSeq)
  }

  private def declaration(value: Value): OxDeclaration = {
    str(value, "kind") match {
      case "macro" =>
        OxMacroDecl(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          parameters = value("parameters").arr.map(_.str).toSeq,
          body = str(value, "body")
        )
      case "struct" =>
        OxStructDecl(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          fields = value("fields").arr.map(field).toSeq
        )
      case "enum" =>
        OxEnumDecl(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          variants = value("variants").arr.map(enumVariant).toSeq
        )
      case "function" =>
        OxFunctionDecl(
          name = str(value, "name"),
          returnType = str(value, "returnType"),
          signature = str(value, "signature"),
          code = str(value, "code"),
          line = int(value, "line"),
          parameters = value("parameters").arr.map(parameter).toSeq,
          body = value("body").arr.map(statement).toSeq
        )
      case other =>
        throw new IllegalArgumentException(s"unsupported oxidized cxxastgen declaration kind '$other'")
    }
  }

  private def field(value: Value): OxFieldDecl = {
    OxFieldDecl(name = str(value, "name"), typeName = str(value, "typeName"), code = str(value, "code"))
  }

  private def enumVariant(value: Value): OxEnumVariant = {
    OxEnumVariant(
      name = str(value, "name"),
      value = value.obj.get("value").filter(!_.isNull).map(_.str),
      code = str(value, "code")
    )
  }

  private def parameter(value: Value): OxParameterDecl = {
    OxParameterDecl(
      name = str(value, "name"),
      typeName = str(value, "typeName"),
      code = str(value, "code"),
      line = int(value, "line")
    )
  }

  private def statement(value: Value): OxStatement = {
    str(value, "kind") match {
      case "localDecl" =>
        OxLocalDecl(
          name = str(value, "name"),
          typeName = str(value, "typeName"),
          code = str(value, "code"),
          line = int(value, "line"),
          initializer = value.obj.get("initializer").filter(!_.isNull).map(expression)
        )
      case "assignment" =>
        OxAssignment(
          code = str(value, "code"),
          line = int(value, "line"),
          left = expression(value("left")),
          right = expression(value("right"))
        )
      case "return" =>
        OxReturn(
          code = str(value, "code"),
          line = int(value, "line"),
          expression = value.obj.get("expression").filter(!_.isNull).map(expression)
        )
      case "if" =>
        OxIf(
          code = str(value, "code"),
          line = int(value, "line"),
          condition = expression(value("condition")),
          thenBody = value("thenBody").arr.map(statement).toSeq,
          elseBody = value("elseBody").arr.map(statement).toSeq
        )
      case "while" =>
        OxWhile(
          code = str(value, "code"),
          line = int(value, "line"),
          condition = expression(value("condition")),
          body = value("body").arr.map(statement).toSeq
        )
      case "expression" =>
        OxExpressionStatement(
          code = str(value, "code"),
          line = int(value, "line"),
          expression = expression(value("expression"))
        )
      case other =>
        throw new IllegalArgumentException(s"unsupported oxidized cxxastgen statement kind '$other'")
    }
  }

  private def expression(value: Value): OxExpression = {
    str(value, "kind") match {
      case "identifier" =>
        OxIdentifier(name = str(value, "name"), code = str(value, "code"), line = int(value, "line"))
      case "literal" =>
        OxLiteral(value = str(value, "value"), code = str(value, "code"), line = int(value, "line"))
      case "binary" =>
        OxBinary(
          operator = str(value, "operator"),
          code = str(value, "code"),
          line = int(value, "line"),
          left = expression(value("left")),
          right = expression(value("right"))
        )
      case "call" =>
        OxCall(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          arguments = value("arguments").arr.map(expression).toSeq
        )
      case "fieldAccess" =>
        OxFieldAccess(
          field = str(value, "field"),
          code = str(value, "code"),
          line = int(value, "line"),
          base = expression(value("base"))
        )
      case other =>
        throw new IllegalArgumentException(s"unsupported oxidized cxxastgen expression kind '$other'")
    }
  }

  private def str(value: Value, key: String): String = value(key).str

  private def int(value: Value, key: String): Int = value(key).num.toInt

}
