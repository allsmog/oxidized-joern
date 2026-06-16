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

case class OxIncludeDecl(name: String, code: String, line: Int) extends OxDeclaration

case class OxStructDecl(
  name: String,
  code: String,
  line: Int,
  fields: Seq[OxFieldDecl],
  nestedDeclarations: Seq[OxDeclaration]
) extends OxDeclaration

case class OxFieldDecl(name: String, typeName: String, code: String)

case class OxEnumDecl(name: String, code: String, line: Int, variants: Seq[OxEnumVariant]) extends OxDeclaration

case class OxEnumVariant(name: String, value: Option[String], code: String, line: Int)

case class OxGlobalVariableDecl(
  name: String,
  typeName: String,
  code: String,
  line: Int,
  initializer: Option[OxExpression]
) extends OxDeclaration

case class OxTypedefDecl(name: String, typeName: String, code: String, line: Int) extends OxDeclaration

case class OxFunctionDecl(
  name: String,
  returnType: String,
  signature: String,
  isDefinition: Boolean,
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

case class OxAssignment(operator: String, code: String, line: Int, left: OxExpression, right: OxExpression)
    extends OxStatement

case class OxReturn(code: String, line: Int, expression: Option[OxExpression]) extends OxStatement

case class OxIf(
  code: String,
  line: Int,
  condition: OxExpression,
  thenBody: Seq[OxStatement],
  elseBody: Seq[OxStatement]
) extends OxStatement

case class OxWhile(code: String, line: Int, condition: OxExpression, body: Seq[OxStatement]) extends OxStatement

case class OxDoWhile(code: String, line: Int, condition: OxExpression, body: Seq[OxStatement]) extends OxStatement

case class OxFor(
  code: String,
  line: Int,
  initializer: Seq[OxStatement],
  condition: Option[OxExpression],
  update: Option[OxExpression],
  body: Seq[OxStatement]
) extends OxStatement

case class OxBreak(code: String, line: Int) extends OxStatement

case class OxContinue(code: String, line: Int) extends OxStatement

case class OxGoto(code: String, line: Int, label: String) extends OxStatement

case class OxLabel(code: String, line: Int, label: String, body: Seq[OxStatement]) extends OxStatement

case class OxSwitch(code: String, line: Int, condition: OxExpression, body: Seq[OxStatement]) extends OxStatement

case class OxCase(code: String, line: Int, value: Option[OxExpression], body: Seq[OxStatement]) extends OxStatement

case class OxExpressionStatement(code: String, line: Int, expression: OxExpression) extends OxStatement

sealed trait OxExpression {
  def code: String
  def line: Int
}

case class OxIdentifier(name: String, code: String, line: Int) extends OxExpression

case class OxLiteral(value: String, code: String, line: Int) extends OxExpression

case class OxBinary(operator: String, code: String, line: Int, left: OxExpression, right: OxExpression)
    extends OxExpression

case class OxUnary(operator: String, code: String, line: Int, prefix: Boolean, argument: OxExpression)
    extends OxExpression

case class OxConditional(
  code: String,
  line: Int,
  condition: OxExpression,
  consequence: Option[OxExpression],
  alternative: OxExpression
) extends OxExpression

case class OxCast(typeName: String, code: String, line: Int, value: OxExpression) extends OxExpression

case class OxSizeOf(code: String, line: Int, value: Option[OxExpression], typeName: Option[String]) extends OxExpression

case class OxCall(name: String, code: String, line: Int, callee: OxExpression, arguments: Seq[OxExpression])
    extends OxExpression

case class OxFieldAccess(field: String, code: String, line: Int, base: OxExpression) extends OxExpression

case class OxIndexAccess(code: String, line: Int, base: OxExpression, index: OxExpression) extends OxExpression

case class OxInitializerList(code: String, line: Int, elements: Seq[OxExpression]) extends OxExpression

case class OxDesignatedInitializer(code: String, line: Int, designator: OxExpression, value: OxExpression)
    extends OxExpression

case class OxDesignator(name: String, code: String, line: Int) extends OxExpression

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
      case "include" =>
        OxIncludeDecl(name = str(value, "name"), code = str(value, "code"), line = int(value, "line"))
      case "struct" =>
        OxStructDecl(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          fields = value("fields").arr.map(field).toSeq,
          nestedDeclarations =
            value.obj.get("nestedDeclarations").map(_.arr.map(declaration).toSeq).getOrElse(Seq.empty)
        )
      case "enum" =>
        OxEnumDecl(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          variants = value("variants").arr.map(enumVariant).toSeq
        )
      case "globalVariable" =>
        OxGlobalVariableDecl(
          name = str(value, "name"),
          typeName = str(value, "typeName"),
          code = str(value, "code"),
          line = int(value, "line"),
          initializer = value.obj.get("initializer").filter(!_.isNull).map(expression)
        )
      case "typedef" =>
        OxTypedefDecl(
          name = str(value, "name"),
          typeName = str(value, "typeName"),
          code = str(value, "code"),
          line = int(value, "line")
        )
      case "function" =>
        OxFunctionDecl(
          name = str(value, "name"),
          returnType = str(value, "returnType"),
          signature = str(value, "signature"),
          isDefinition = value("isDefinition").bool,
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
      code = str(value, "code"),
      line = int(value, "line")
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
          operator = str(value, "operator"),
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
      case "doWhile" =>
        OxDoWhile(
          code = str(value, "code"),
          line = int(value, "line"),
          condition = expression(value("condition")),
          body = value("body").arr.map(statement).toSeq
        )
      case "for" =>
        OxFor(
          code = str(value, "code"),
          line = int(value, "line"),
          initializer = value("initializer").arr.map(statement).toSeq,
          condition = value.obj.get("condition").filter(!_.isNull).map(expression),
          update = value.obj.get("update").filter(!_.isNull).map(expression),
          body = value("body").arr.map(statement).toSeq
        )
      case "break" =>
        OxBreak(code = str(value, "code"), line = int(value, "line"))
      case "continue" =>
        OxContinue(code = str(value, "code"), line = int(value, "line"))
      case "goto" =>
        OxGoto(code = str(value, "code"), line = int(value, "line"), label = str(value, "label"))
      case "label" =>
        OxLabel(
          code = str(value, "code"),
          line = int(value, "line"),
          label = str(value, "label"),
          body = value("body").arr.map(statement).toSeq
        )
      case "switch" =>
        OxSwitch(
          code = str(value, "code"),
          line = int(value, "line"),
          condition = expression(value("condition")),
          body = value("body").arr.map(statement).toSeq
        )
      case "case" =>
        OxCase(
          code = str(value, "code"),
          line = int(value, "line"),
          value = value.obj.get("value").filter(!_.isNull).map(expression),
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
      case "unary" =>
        OxUnary(
          operator = str(value, "operator"),
          code = str(value, "code"),
          line = int(value, "line"),
          prefix = value("prefix").bool,
          argument = expression(value("argument"))
        )
      case "conditional" =>
        OxConditional(
          code = str(value, "code"),
          line = int(value, "line"),
          condition = expression(value("condition")),
          consequence = value.obj.get("consequence").filter(!_.isNull).map(expression),
          alternative = expression(value("alternative"))
        )
      case "cast" =>
        OxCast(
          typeName = str(value, "typeName"),
          code = str(value, "code"),
          line = int(value, "line"),
          value = expression(value("value"))
        )
      case "sizeOf" =>
        OxSizeOf(
          code = str(value, "code"),
          line = int(value, "line"),
          value = value.obj.get("value").filter(!_.isNull).map(expression),
          typeName = value.obj.get("typeName").filter(!_.isNull).map(_.str)
        )
      case "call" =>
        OxCall(
          name = str(value, "name"),
          code = str(value, "code"),
          line = int(value, "line"),
          callee = expression(value("callee")),
          arguments = value("arguments").arr.map(expression).toSeq
        )
      case "fieldAccess" =>
        OxFieldAccess(
          field = str(value, "field"),
          code = str(value, "code"),
          line = int(value, "line"),
          base = expression(value("base"))
        )
      case "indexAccess" =>
        OxIndexAccess(
          code = str(value, "code"),
          line = int(value, "line"),
          base = expression(value("base")),
          index = expression(value("index"))
        )
      case "initializerList" =>
        OxInitializerList(
          code = str(value, "code"),
          line = int(value, "line"),
          elements = value("elements").arr.map(expression).toSeq
        )
      case "designatedInitializer" =>
        OxDesignatedInitializer(
          code = str(value, "code"),
          line = int(value, "line"),
          designator = expression(value("designator")),
          value = expression(value("value"))
        )
      case "designator" =>
        OxDesignator(name = str(value, "name"), code = str(value, "code"), line = int(value, "line"))
      case other =>
        throw new IllegalArgumentException(s"unsupported oxidized cxxastgen expression kind '$other'")
    }
  }

  private def str(value: Value, key: String): String = value(key).str

  private def int(value: Value, key: String): Int = value(key).num.toInt

}
