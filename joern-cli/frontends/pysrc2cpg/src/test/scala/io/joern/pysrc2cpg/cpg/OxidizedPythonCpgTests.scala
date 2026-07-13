package io.joern.pysrc2cpg.cpg

import io.joern.pysrc2cpg.PythonParserBackend
import io.joern.pysrc2cpg.testfixtures.PySrc2CpgFixture
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*
import org.scalatest.matchers.should.Matchers

class OxidizedPythonCpgTests extends PySrc2CpgFixture(withParserBackend = PythonParserBackend.Oxidized) with Matchers {

  "create CPG for functions, defaults, calls, and binary operators through pyastgen" in {
    val cpg = code(
      """def add(x, y=1):
        |  return x + y
        |
        |z = add(2)
        |""".stripMargin,
      "test.py"
    )

    val methodNode = cpg.method.fullName("test.py:<module>.add").head
    methodNode.name shouldBe "add"
    methodNode.lineNumber shouldBe Some(1)
    methodNode.columnNumber shouldBe Some(1)

    cpg.method.fullName("test.py:<module>.add").parameter.order(1).name.head shouldBe "x"
    cpg.method.fullName("test.py:<module>.add").parameter.order(2).name.head shouldBe "y"

    val additionCall = cpg.call.methodFullName(Operators.addition).head
    additionCall.code shouldBe "x + y"
    additionCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

    cpg.call.name("add").code("add\\(2\\)").nonEmpty shouldBe true
    cpg.identifier.name("z").lineNumber(4).nonEmpty shouldBe true
  }

  "create CPG for classes, imports, and control flow through pyastgen" in {
    val cpg = code(
      """import os
        |
        |class Service:
        |  def run(self, xs):
        |    for x in xs:
        |      if x > 0:
        |        print(os.path.join("root", str(x)))
        |""".stripMargin,
      "service.py"
    )

    cpg.typeDecl.fullName("service.py:<module>.Service").nonEmpty shouldBe true
    cpg.method.fullName("service.py:<module>.Service.run").parameter.name("self").nonEmpty shouldBe true
    cpg.controlStructure.code("while ... : ...").nonEmpty shouldBe true
    cpg.controlStructure.code("if ... : ...").nonEmpty shouldBe true
    cpg.call.name("print").lineNumber(7).nonEmpty shouldBe true
  }

  "preserve adjacent string expression lists through pyastgen" in {
    val cpg = code(""""one" "two" "three"""", "strings.py")

    val callNode = cpg.call.methodFullName("<operator>.stringExpressionList").head
    callNode.code shouldBe """"one" "two" "three""""
    callNode.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

    callNode.astChildren.order(1).isLiteral.head.code shouldBe "\"one\""
    callNode.astChildren.order(2).isLiteral.head.code shouldBe "\"two\""
    callNode.astChildren.order(3).isLiteral.head.code shouldBe "\"three\""
  }

  "fall back to JavaCC for Python 2-only syntax rejected by pyastgen" in {
    val cpg = code("""print legacy""", "legacy.py")

    cpg.file.name("legacy.py").nonEmpty shouldBe true
    cpg.call.name("print").nonEmpty shouldBe true
    cpg.identifier.name("legacy").nonEmpty shouldBe true
  }

  "create CPG for broader Python 3 syntax through pyastgen" in {
    val cpg = code(
      """type Vector[T] = list[T]
        |
        |async def afetch(client, key):
        |  async with client.session() as s:
        |    return await s.get(key)
        |
        |@decorator(1)
        |def fmt(user, value):
        |  return f"{user.name!r}:{value:0.2f}"
        |
        |vals = [x * 2 for x in range(5) if x > 1]
        |try:
        |  risky()
        |except ValueError as err:
        |  raise RuntimeError(str(err)) from err
        |finally:
        |  cleanup()
        |""".stripMargin,
      "features.py"
    )

    val aliasAssignment = cpg.call.methodFullName(Operators.assignment).codeExact("Vector = list[T]").head
    aliasAssignment.argument.argumentIndex(1).isIdentifier.code.l shouldBe List("Vector")
    aliasAssignment.argument.argumentIndex(2).isCall.code.l shouldBe List("list[T]")
    cpg.method
      .fullName("features.py:<module>")
      .ast
      .collectAll[Unknown]
      .filter(_.parserTypeName.contains("TypeAlias"))
      .l shouldBe empty

    cpg.method.fullName("features.py:<module>.afetch").nonEmpty shouldBe true
    cpg.method.fullName("features.py:<module>.fmt").nonEmpty shouldBe true
    cpg.call.methodFullName(Operators.formatString).nonEmpty shouldBe true
    cpg.call.methodFullName(Operators.multiplication).nonEmpty shouldBe true
    cpg.call.name("range").nonEmpty shouldBe true
    cpg.controlStructure.controlStructureType(ControlStructureTypes.TRY).nonEmpty shouldBe true
    cpg.call.name("cleanup").nonEmpty shouldBe true
  }

  "preserve f-string segments, format specs, and debug fields through pyastgen" in {
    val cpg = code(
      """def fmt(user, value):
        |  return f"pre {user.name!r}:{value:0.2f}:{value=}"
        |
        |def plain(value):
        |  return f"value={value}"
        |""".stripMargin,
      "fmt.py"
    )

    val formatCall = cpg.method.nameExact("fmt").call.methodFullName(Operators.formatString).head
    formatCall.code shouldBe """f"pre {user.name!r}:{value:0.2f}:{value=}""""

    cpg.method.nameExact("fmt").call.methodFullName(Operators.formattedValue).code.l should contain allOf (
      "{user.name!r}",
      "{value:0.2f}",
      "{value=}"
    )
    val literalCodes = cpg.literal.code.l
    literalCodes should contain("pre ")
    literalCodes.count(_ == ":") shouldBe 2

    val plainFormatCall = cpg.method.nameExact("plain").call.methodFullName(Operators.formatString).head
    plainFormatCall.code shouldBe """f"value={value}""""
    cpg.method.nameExact("plain").call.methodFullName(Operators.formattedValue).code.l shouldBe List("{value}")
    cpg.method.nameExact("plain").literal.code.l should contain("value=")
  }
}
