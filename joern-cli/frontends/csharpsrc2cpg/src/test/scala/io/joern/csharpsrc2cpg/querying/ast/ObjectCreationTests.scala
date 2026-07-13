package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*

class ObjectCreationTests extends CSharpCode2CpgFixture {

  "assignment to an object creation for a known class" should {
    val cpg = code("""
        |using System.Text;
        |var x = new StringBuilder();
        |""".stripMargin)

    "have correct constructor call properties" in {
      inside(cpg.call.nameExact(Defines.ConstructorMethodName).headOption) { case Some(ctor) =>
        ctor.typeFullName shouldBe "System.Text.StringBuilder"
        ctor.methodFullName shouldBe "System.Text.StringBuilder.<init>"
      }
    }

    "have correct typeFullName for the assigned variable" in {
      cpg.assignment.target.isIdentifier.nameExact("x").typeFullName.l shouldBe List("System.Text.StringBuilder")
    }
  }

  "assignment to a fully-qualified object creation for a known class" should {
    val cpg = code("""
        |var x = new System.Text.StringBuilder();
        |""".stripMargin)

    "have correct constructor call properties" in {
      inside(cpg.call.nameExact(Defines.ConstructorMethodName).headOption) { case Some(ctor) =>
        ctor.typeFullName shouldBe "System.Text.StringBuilder"
        ctor.methodFullName shouldBe "System.Text.StringBuilder.<init>"
      }
    }

    "have correct typeFullName for the assigned variable" in {
      cpg.assignment.target.isIdentifier.nameExact("x").typeFullName.l shouldBe List("System.Text.StringBuilder")
    }
  }

  "object creation with an initializer" should {
    val cpg = code("""
        |class Widget
        |{
        |  public int X;
        |  public string Name;
        |  public Widget(int seed) { }
        |}
        |
        |class C
        |{
        |  void M()
        |  {
        |    var widget = new Widget(7) { X = 1, Name = "a" };
        |  }
        |}
        |""".stripMargin)

    "attach initializer assignments to the constructor call" in {
      inside(cpg.call.nameExact(Defines.ConstructorMethodName).codeExact("""new Widget(7) { X = 1, Name = "a" }""").l) {
        case ctor :: Nil =>
          ctor.typeFullName shouldBe "Widget"
          ctor.methodFullName shouldBe "Widget.<init>"
          ctor.argument.code.l shouldBe List("this", "7", "X = 1", "Name = \"a\"")
      }

      cpg.call.nameExact(Operators.assignment).code.l should contain allOf ("X = 1", "Name = \"a\"")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }
}
