package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.CSharpModifiers
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.EvaluationStrategies
import io.shiftleft.codepropertygraph.generated.ModifierTypes
import io.shiftleft.semanticcpg.language.*

class ParameterTests extends CSharpCode2CpgFixture {

  "a default static main method" should {
    val cpg = code(basicBoilerplate(), "Program.cs")

    "generate a method node with a string[] args parameter" in {
      val x          = cpg.method.nameExact("Main").head
      val List(args) = x.parameter.l: @unchecked
      args.name shouldBe "args"
      args.typeFullName shouldBe "System.String[]"
      args.code shouldBe "string[] args"
      args.index shouldBe 1
      args.isVariadic shouldBe false
    }

  }

  "virtual method with multiple parameters" should {
    val cpg = code(
      """using System;
        |
        |namespace HelloWorld
        |{
        |  class Program
        |  {
        |    void Foo(string a, int b)
        |    {
        |      return a;
        |    }
        |  }
        |
        |}
        |""".stripMargin,
      "Program.cs"
    )

    "generate a method node with an implicit this parameter, as well as the declared parameters" in {
      val x                    = cpg.method.nameExact("Foo").head
      val List(thisNode, a, b) = x.parameter.l: @unchecked

      thisNode.name shouldBe "this"
      thisNode.typeFullName shouldBe "HelloWorld.Program"
      thisNode.code shouldBe "this"
      thisNode.index shouldBe 0
      thisNode.isVariadic shouldBe false

      a.name shouldBe "a"
      a.typeFullName shouldBe "System.String"
      a.code shouldBe "string a"
      a.index shouldBe 1
      a.isVariadic shouldBe false

      b.name shouldBe "b"
      b.typeFullName shouldBe "System.Int32"
      b.code shouldBe "int b"
      b.index shouldBe 2
      b.isVariadic shouldBe false
    }

  }

  "parameter modifiers" should {
    val cpg = code("""
        |static class Ext
        |{
        |  public static void M(this string text, ref int value, out int written, in int read, params string[] rest)
        |  {
        |    written = value + read;
        |  }
        |}
        |""".stripMargin)

    "preserve modifier nodes, variadic params, and by-reference evaluation" in {
      inside(cpg.method.nameExact("M").parameter.l) { case text :: value :: written :: read :: rest :: Nil =>
        text.name shouldBe "text"
        text.astChildren.isModifier.modifierType.l shouldBe List(CSharpModifiers.THIS)
        text.evaluationStrategy shouldBe EvaluationStrategies.BY_SHARING
        text.isVariadic shouldBe false

        value.name shouldBe "value"
        value.astChildren.isModifier.modifierType.l shouldBe List(CSharpModifiers.REF)
        value.evaluationStrategy shouldBe EvaluationStrategies.BY_REFERENCE
        value.isVariadic shouldBe false

        written.name shouldBe "written"
        written.astChildren.isModifier.modifierType.l shouldBe List(CSharpModifiers.OUT)
        written.evaluationStrategy shouldBe EvaluationStrategies.BY_REFERENCE
        written.isVariadic shouldBe false

        read.name shouldBe "read"
        read.astChildren.isModifier.modifierType.l shouldBe List(CSharpModifiers.IN)
        read.evaluationStrategy shouldBe EvaluationStrategies.BY_REFERENCE
        read.isVariadic shouldBe false

        rest.name shouldBe "rest"
        rest.astChildren.isModifier.modifierType.l shouldBe List(CSharpModifiers.PARAMS)
        rest.evaluationStrategy shouldBe EvaluationStrategies.BY_SHARING
        rest.isVariadic shouldBe true
        rest.typeFullName shouldBe "System.String[]"
      }
    }
  }

}
