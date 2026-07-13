package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.semanticcpg.language.*

class TypeTests extends CSharpCode2CpgFixture {
  "type resolution for nullable types" should {

    "resolve types for primitive type identifiers" in {
      val cpg = code(basicBoilerplate("""
          |int? a = 10;
          |string? b = "Foo";
          |var c = null;
          |""".stripMargin))

      inside(cpg.identifier.nameExact("a").l) { case a :: Nil =>
        a.typeFullName shouldBe "System.Int32"
      }

      inside(cpg.identifier.nameExact("b").l) { case a :: Nil =>
        a.typeFullName shouldBe "System.String"
      }

      inside(cpg.identifier.nameExact("c").l) { case a :: Nil =>
        a.typeFullName shouldBe "null"
      }
    }

    "resolve types for custom type identifiers" in {
      val cpg = code("""
          |namespace Foo {
          | public class Bar {}
          | public class Baz {
          |   static void mBaz() {
          |     Bar? iBar = new Bar();
          |   }
          | }
          |}
          |""".stripMargin)

      inside(cpg.identifier.nameExact("iBar").l) { case iBar :: Nil =>
        iBar.typeFullName shouldBe "Foo.Bar"
      }
    }
  }

  "resolve function pointer types" in {
    val cpg = code("""
        |class C {
        |  delegate* unmanaged[Cdecl]<int, ref string, void> callback;
        |
        |  void M(delegate*<int, void> localCallback) {
        |    delegate*<int, void> fp = localCallback;
        |  }
        |}
        |""".stripMargin)

    cpg.member.nameExact("callback").typeFullName.l shouldBe List(
      "delegate* unmanaged[Cdecl]<System.Int32, ref System.String, System.Void>"
    )
    cpg.method.nameExact("M").parameter.nameExact("localCallback").typeFullName.l shouldBe List(
      "delegate*<System.Int32, System.Void>"
    )
    cpg.local.nameExact("fp").typeFullName.l shouldBe List("delegate*<System.Int32, System.Void>")
  }

  "resolve scoped types" in {
    val cpg = code("""
        |using System;
        |
        |class C {
        |  void M(scoped System.Span<int> span) {
        |    scoped ref int first = ref span[0];
        |  }
        |}
        |""".stripMargin)

    cpg.method.nameExact("M").parameter.nameExact("span").typeFullName.l shouldBe List("System.Span")
    cpg.local.nameExact("first").typeFullName.l shouldBe List("ref System.Int32")
  }

  "resolve types for operators and propagate to others" in {
    val cpg = code(basicBoilerplate("""
        |int a = 10;
        |var b = a > 1;
        |""".stripMargin))

    inside(cpg.local.nameExact("b").l) { case b :: Nil =>
      b.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)
    }

    inside(cpg.call.nameExact(Operators.greaterThan).l) { case opCall :: Nil =>
      opCall.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)
    }
  }
}
