package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.semanticcpg.language.*

class AnnotationTests extends CSharpCode2CpgFixture {
  "annotations for methods" should {
    "have correct attributes" in {
      val cpg = code("""
          |using System;
          |
          |namespace Foo {
          | public class Bar {
          |   [Obsolete("Dep Method", false)]
          |   public static void Main() {}
          | }
          |}
          |""".stripMargin)

      inside(cpg.method("Main").annotation.l) { case obsolete :: Nil =>
        obsolete.code shouldBe "Obsolete(\"Dep Method\", false)"
        obsolete.name shouldBe "Obsolete"
        obsolete.lineNumber shouldBe Some(5)
        obsolete.columnNumber shouldBe Some(4)
        obsolete.fullName shouldBe "System.ObsoleteAttribute"
      }
    }

    "preserve target specifiers" in {
      val cpg = code("""
          |using System;
          |[assembly: CLSCompliant(true)]
          |
          |namespace Foo {
          | public class Bar {
          |   [return: Sample]
          |   public static int M([param: Sample] int x) { return x; }
          | }
          |
          | public class SampleAttribute : Attribute {}
          |}
          |""".stripMargin)

      inside(cpg.annotation.codeExact("assembly: CLSCompliant(true)").l) { case assembly :: Nil =>
        assembly.name shouldBe "CLSCompliant"
      }

      inside(cpg.method("M").annotation.codeExact("return: Sample").l) { case returnSample :: Nil =>
        returnSample.name shouldBe "Sample"
      }

      inside(cpg.method("M").parameter.nameExact("x").annotation.l) { case paramSample :: Nil =>
        paramSample.code shouldBe "param: Sample"
        paramSample.name shouldBe "Sample"
      }
    }

    "create parameter assignments for named arguments" in {
      val cpg = code("""
          |using System;
          |
          |namespace Foo {
          | public class Bar {
          |   [Example(positional: "ctor", Name = "prop")]
          |   public static void Main() {}
          | }
          |
          | public class ExampleAttribute : Attribute {
          |   public ExampleAttribute(string positional) {}
          |   public string Name { get; set; }
          | }
          |}
          |""".stripMargin)

      inside(cpg.method("Main").annotation.l) { case example :: Nil =>
        example.parameterAssign.code.l.toSet shouldBe Set("positional: \"ctor\"", "Name = \"prop\"")
        example.parameterAssign.parameter.code.l.toSet shouldBe Set("positional", "Name")
      }
    }
  }

  "annotations for classes" should {
    "have correct attributes" in {
      val cpg = code("""
          |using System;
          |
          |namespace Foo {
          | [Obsolete("Dep Class", false)]
          | public class Bar {
          |   public static void Main() {}
          | }
          |}
          |""".stripMargin)

      inside(cpg.typeDecl("Bar").annotation.l) { case obsolete :: Nil =>
        obsolete.code shouldBe "Obsolete(\"Dep Class\", false)"
        obsolete.name shouldBe "Obsolete"
        obsolete.lineNumber shouldBe Some(4)
        obsolete.columnNumber shouldBe Some(2)
        obsolete.fullName shouldBe "System.ObsoleteAttribute"
      }
    }

    "have correct code for Route attribute" in {
      val cpg = code("""
          |using System;
          |
          |namespace Foo {
          | [Route("api/v{version:number}/some/[controller]")]
          | public class Controller {
          |   public static void Main() {}
          | }
          |}
          |""".stripMargin)

      inside(cpg.typeDecl("Controller").annotation.l) { case route :: Nil =>
        route.code shouldBe "Route(\"api/v{version:number}/some/[controller]\")"
        route.name shouldBe "Route"
        route.fullName shouldBe "RouteAttribute"
      }

    }
  }

  "annotations for members" should {
    "have correct attributes" in {
      val cpg = code("""
          |using System;
          |
          |namespace Foo {
          | public class Bar {
          |   [Serializable] public string firstName;
          | }
          |}
          |""".stripMargin)

      inside(cpg.member("firstName").annotation.l) { case serializable :: Nil =>
        serializable.code shouldBe "Serializable"
        serializable.name shouldBe "Serializable"
        serializable.lineNumber shouldBe Some(5)
        serializable.columnNumber shouldBe Some(4)
        serializable.fullName shouldBe "System.SerializableAttribute"
      }
    }
  }
}
