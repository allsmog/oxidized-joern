package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.CSharpOperators
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.codepropertygraph.generated.nodes.{Identifier, Literal, TypeRef, Unknown}
import io.shiftleft.semanticcpg.language.*

class CallTests extends CSharpCode2CpgFixture {

  "builtin calls" should {

    val cpg = code(basicBoilerplate())

    "create a call node with arguments" in {
      inside(cpg.call.nameExact("WriteLine").headOption) { case Some(writeLine) =>
        writeLine.name shouldBe "WriteLine"
        writeLine.methodFullName shouldBe "System.Console.WriteLine:System.Void(System.String)"
        writeLine.typeFullName shouldBe "System.Void"
        writeLine.code shouldBe "Console.WriteLine(\"Hello, world!\")"

        inside(writeLine.argument.l) { case (base: Identifier) :: (strArg: Literal) :: Nil =>
          base.typeFullName shouldBe "System.Console"
          base.name shouldBe "Console"
          base.code shouldBe "Console"
          base.argumentIndex shouldBe 0

          strArg.typeFullName shouldBe "System.String"
          strArg.code shouldBe "\"Hello, world!\""
          strArg.argumentIndex shouldBe 1
        }
      }
    }

  }

  "with expressions" should {
    val cpg = code("""
        |namespace Foo;
        |
        |public record Person(int Age);
        |
        |public class Bar {
        | public static void Main() {
        |   var p = new Person(1);
        |   var older = p with { Age = 2 };
        | }
        |}
        |""".stripMargin)

    "create a with operator call with initializer assignments" in {
      inside(cpg.call.nameExact(CSharpOperators.withExpression).l) { case withCall :: Nil =>
        withCall.code shouldBe "p with { Age = 2 }"
        withCall.typeFullName shouldBe "Foo.Person"
        withCall.argument.code.l shouldBe List("p", "Age = 2")
      }
      cpg.call.nameExact(Operators.assignment).code.l should contain("Age = 2")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "null-coalescing expressions" should {
    val cpg = code("""
        |class C
        |{
        |  string M(string a, string b)
        |  {
        |    a ??= b;
        |    return a ?? b;
        |  }
        |}
        |""".stripMargin)

    "create elvis and assignment calls without unknown nodes" in {
      inside(cpg.call.nameExact(Operators.elvis).l) { case elvis :: Nil =>
        elvis.code shouldBe "a ?? b"
        elvis.typeFullName shouldBe "System.String"
        elvis.argument.isIdentifier.name.l shouldBe List("a", "b")
      }

      cpg.call.nameExact(Operators.assignment).code.l should contain("a ??= b")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "type operator expressions" should {
    val cpg = code("""
        |using System;
        |namespace Foo;
        |
        |public class Bar {
        | public static void Main(object value) {
        |   string casted = value as string;
        |   bool ok = value is string;
        |   var typ = typeof(string);
        |   var size = sizeof(int);
        |   var name = nameof(value);
        |   string text = default;
        |   var fallback = default(string);
        |   TypedReference typed = __makeref(value);
        |   var refType = __reftype(typed);
        |   var refValue = __refvalue(typed, object);
        |   Func<int> factory = () => throw new Exception();
        | }
        |}
        |""".stripMargin)

    "create cast, instanceOf, typeOf, sizeOf, nameOf, default, and throw calls" in {
      inside(cpg.call.nameExact(Operators.cast).l) { case castCall :: Nil =>
        castCall.code shouldBe "value as string"
        castCall.typeFullName shouldBe "System.String"
        inside(castCall.argument.l) { case (stringType: TypeRef) :: (value: Identifier) :: Nil =>
          stringType.code shouldBe "string"
          stringType.typeFullName shouldBe "System.String"
          value.name shouldBe "value"
        }
      }

      inside(cpg.call.nameExact(Operators.instanceOf).l) { case isCall :: Nil =>
        isCall.code shouldBe "value is string"
        isCall.typeFullName shouldBe "System.Boolean"
        inside(isCall.argument.l) { case (value: Identifier) :: (stringType: TypeRef) :: Nil =>
          value.name shouldBe "value"
          stringType.typeFullName shouldBe "System.String"
        }
      }

      inside(cpg.call.nameExact(CSharpOperators.typeOf).l) { case typeOfCall :: Nil =>
        typeOfCall.code shouldBe "typeof(string)"
        typeOfCall.typeFullName shouldBe "System.Type"
        typeOfCall.argument.isTypeRef.typeFullName.l shouldBe List("System.String")
      }

      inside(cpg.call.nameExact(Operators.sizeOf).l) { case sizeOfCall :: Nil =>
        sizeOfCall.code shouldBe "sizeof(int)"
        sizeOfCall.typeFullName shouldBe "System.Int32"
        sizeOfCall.argument.isTypeRef.typeFullName.l shouldBe List("System.Int32")
      }

      inside(cpg.call.nameExact(CSharpOperators.nameOf).l) { case nameOfCall :: Nil =>
        nameOfCall.code shouldBe "nameof(value)"
        nameOfCall.typeFullName shouldBe "System.String"
        nameOfCall.argument.isIdentifier.name.l shouldBe List("value")
      }
      cpg.call.nameExact("nameof").l shouldBe Nil

      val defaultCalls = cpg.call.nameExact(CSharpOperators.defaultValue).map(call => call.code -> call).toMap
      defaultCalls.keySet shouldBe Set("default", "default(string)")
      defaultCalls("default(string)").typeFullName shouldBe "System.String"
      defaultCalls("default(string)").argument.isTypeRef.typeFullName.l shouldBe List("System.String")

      inside(cpg.call.nameExact(CSharpOperators.throws).l) { case throwCall :: Nil =>
        throwCall.code shouldBe "throw new Exception()"
        throwCall.argument.code.l shouldBe List("new Exception()")
      }

      inside(cpg.call.nameExact(CSharpOperators.makeRef).l) { case makeRefCall :: Nil =>
        makeRefCall.code shouldBe "__makeref(value)"
        makeRefCall.typeFullName shouldBe "System.TypedReference"
        makeRefCall.argument.isIdentifier.name.l shouldBe List("value")
      }

      inside(cpg.call.nameExact(CSharpOperators.refType).l) { case refTypeCall :: Nil =>
        refTypeCall.code shouldBe "__reftype(typed)"
        refTypeCall.typeFullName shouldBe "System.Type"
        refTypeCall.argument.isIdentifier.name.l shouldBe List("typed")
      }

      inside(cpg.call.nameExact(CSharpOperators.refValue).l) { case refValueCall :: Nil =>
        refValueCall.code shouldBe "__refvalue(typed, object)"
        refValueCall.typeFullName shouldBe "System.Object"
        inside(refValueCall.argument.l) { case (typed: Identifier) :: (objectType: TypeRef) :: Nil =>
          typed.name shouldBe "typed"
          objectType.typeFullName shouldBe "System.Object"
        }
      }

      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "stackalloc expressions" should {
    val cpg = code("""
        |namespace Foo;
        |
        |public class Bar {
        | public static void Main() {
        |   unsafe {
        |     int* values = stackalloc int[3];
        |   }
        | }
        |}
        |""".stripMargin)

    "create a stackalloc operator call with type and rank arguments" in {
      inside(cpg.call.nameExact(CSharpOperators.stackAlloc).l) { case stackAlloc :: Nil =>
        stackAlloc.code shouldBe "stackalloc int[3]"
        stackAlloc.typeFullName shouldBe "System.Int32[]"
        stackAlloc.argument.code.l shouldBe List("int[3]", "3")
      }
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "ref and spread expressions" should {
    val cpg = code("""
        |namespace Foo;
        |
        |public class Bar {
        | public static void Main(int[] xs) {
        |   int[] ys = [0, .. xs, 3];
        |   ref int r = ref xs[0];
        |   r = ref xs[1];
        | }
        |}
        |""".stripMargin)

    "create explicit ref and spread operator calls" in {
      inside(cpg.call.nameExact(CSharpOperators.spread).l) { case spreadCall :: Nil =>
        spreadCall.code shouldBe ".. xs"
        spreadCall.argument.isIdentifier.name.l shouldBe List("xs")
      }

      cpg.call.nameExact(CSharpOperators.ref).code.toSet shouldBe Set("ref xs[0]", "ref xs[1]")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "query expressions" should {
    val cpg = code("""
        |using System.Linq;
        |namespace Foo;
        |
        |public class Bar {
        | public static void Main(int[] codes) {
        |   var queried =
        |     from code in codes
        |     where code > 1
        |     select code;
        | }
        |}
        |""".stripMargin)

    "create a query operator call with clause expression arguments" in {
      inside(cpg.call.nameExact(CSharpOperators.queryExpression).l) { case query :: Nil =>
        query.code.replaceAll("\\s+", " ").trim shouldBe "from code in codes where code > 1 select code"
        query.argument.code.l shouldBe List("int[] codes", "code > 1", "code")
      }
      cpg.call.nameExact(Operators.greaterThan).code.l should contain("code > 1")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "query orderby clauses" should {
    val cpg = code("""
        |using System.Linq;
        |namespace Foo;
        |
        |public class Bar {
        | public static void Main(int[] codes) {
        |   var queried =
        |     from code in codes
        |     orderby code descending, code + 1 ascending
        |     select code;
        | }
        |}
        |""".stripMargin)

    "preserve ordering directions as query arguments" in {
      inside(cpg.call.nameExact(CSharpOperators.queryExpression).l) { case query :: Nil =>
        query.argument.code.l shouldBe List("int[] codes", "code", "descending", "code + 1", "ascending", "code")
      }
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "method invocations with await expression" should {
    val cpg = code("""
        |namespace Foo;
        |
        |public class Bar {
        | public int mBar(int pBar) {
        |   var getP = await new Baz().mBaz("hello");
        | }
        |}
        |
        |public class Baz {
        | public string mBaz(string pBaz) {
        |   return pBaz;
        | }
        |}
        |""".stripMargin)

    "create a call node for mBaz" in {
      inside(cpg.call.nameExact("mBaz").l) { case mBazCall :: Nil =>
        mBazCall.code shouldBe "new Baz().mBaz(\"hello\")"
        mBazCall.methodFullName shouldBe "Foo.Baz.mBaz:System.String(System.String)"
        mBazCall.typeFullName shouldBe "System.String"

      }
    }
  }

  "method invocations with this expression" should {
    val cpg = code("""
        |namespace Foo;
        |
        |public class Bar {
        | public int mBar(int pBar) {
        |   var getBaz = this.mBaz("hello");
        | }
        | public string mBaz(string pBaz) {
        |   return pBaz;
        | }
        |}
        |
        |
        |""".stripMargin)

    "create a call node for mBaz" in {
      inside(cpg.call.nameExact("mBaz").l) { case mBazCall :: Nil =>
        mBazCall.code shouldBe "this.mBaz(\"hello\")"
        mBazCall.methodFullName shouldBe "Foo.Bar.mBaz:System.String(System.String)"
        mBazCall.typeFullName shouldBe "System.String"
      }
    }

  }

  "hierarchical namespace calls" should {
    val cpg = code("""
        |namespace HelloWorld {
        |public class Foo {
        |}
        |
        |public class Bar: Foo {}
        |
        |public class Baz {}
        |}
        |""".stripMargin).moreCode("""
        |namespace HelloWorld.Foo {
        | public class A {
        |   static void main() {
        |     Bar c = new Bar();
        | }
        | }
        |}
        |""".stripMargin)

    "resolve type for Bar in a hierarchical namespace" in {
      inside(cpg.identifier.nameExact("c").l) { case c :: Nil =>
        c.typeFullName shouldBe "HelloWorld.Bar"
      }
    }

  }

  "resolve a call with no receiver on a type sharing a base method inherited from a type in a common namespace" in {
    val cpg = code("""
        |namespace Foo.Bar.Bar {
        |  public class Baz: SomeClass {
        |     public async int SomeMethod() {
        |       var a = await SomeOtherMethod();
        |     }
        |  }
        |}
        |""".stripMargin).moreCode("""
        |namespace Foo.Bar.Bar {
        | public class SomeClass {
        |   protected int SomeOtherMethod() {
        |     return 1;
        |   }
        | }
        |}
        |""".stripMargin)

    cpg.typeDecl.nameExact("Baz").inheritsFromTypeFullName.l shouldBe List("Foo.Bar.Bar.SomeClass")

    inside(cpg.call.nameExact("SomeOtherMethod").l) { case callNode :: Nil =>
      callNode.code shouldBe "SomeOtherMethod()"
      callNode.typeFullName shouldBe "System.Int32"
      callNode.methodFullName shouldBe "Foo.Bar.Bar.SomeClass.SomeOtherMethod:System.Int32()"
    }
  }

  "builtin types" should {
    val cpg = code("""
        |namespace Baz
        |{
        |  class Foo
        |  {
        |    static void Bar()
        |    {
        |      "".ToLower();
        |    }
        |  }
        |}
        |""".stripMargin)
    "resolve the ToLower call even without `using System`" in {
      inside(cpg.call.name("ToLower").methodFullName.l) { case x :: Nil =>
        x shouldBe "System.String.ToLower:System.String()"
      }
    }
  }

  "fully qualified names" should {
    val cpg = code("""
        |namespace Baz
        |{
        |  class Foo
        |  {
        |    static void Bar()
        |    {
        |      System.String x;
        |      x.ToLower();
        |    }
        |  }
        |}
        |""".stripMargin)
    "resolve the ToLower call even without `using System`" in {
      inside(cpg.call.name("ToLower").methodFullName.l) { case x :: Nil =>
        x shouldBe "System.String.ToLower:System.String()"
      }
    }
  }

  "call expression statements with surrounding comments" should {
    val cpg = code("""
        |/* Hey! */
        |System.Console.WriteLine("Foo");
        |// Hey2!
        |System.Console.WriteLine("Bar");
        |System.Console.WriteLine(0);
        |// Hey3!
        |
        |System.Console.WriteLine(1); // Hey4!
        |""".stripMargin)

    "have correct code for call with block comment above it" in {
      cpg.call.nameExact("WriteLine").code.headOption shouldBe Some("System.Console.WriteLine(\"Foo\")")
    }

    "have correct code for call with line comment above it" in {
      cpg.literal("\"Bar\"").inCall.nameExact("WriteLine").code.headOption shouldBe Some(
        "System.Console.WriteLine(\"Bar\")"
      )
    }

    "have correct code for call with line comment below it" in {
      cpg.literal("0").inCall.nameExact("WriteLine").code.headOption shouldBe Some("System.Console.WriteLine(0)")
    }

    "have correct code for call with line comment immediately after it (same-line)" in {
      cpg.literal("1").inCall.nameExact("WriteLine").code.headOption shouldBe Some("System.Console.WriteLine(1)")
    }
  }

  "call expression split into multiple statements" should {
    val cpg = code("""
        |System.
        |  Code.
        |    WriteLine("Foo");
        |""".stripMargin)

    "have correct code" in {
      cpg.call.nameExact("WriteLine").code.headOption shouldBe Some("""System.
        |  Code.
        |    WriteLine("Foo")""".stripMargin)
    }
  }

}
