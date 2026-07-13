package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.CSharpModifiers
import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes
import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes.DotNetTypeMap
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.ModifierTypes
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*

class TypeDeclTests extends CSharpCode2CpgFixture {

  "a basic class declaration" should {
    val cpg = code("public class Container {  }", "Container.cs")

    "generate a type declaration with the correct properties" in {
      val x = cpg.typeDecl.nameExact("Container").head
      x.code shouldBe "public class Container {  }"
      x.fullName shouldBe "Container"
      x.filename shouldBe "Container.cs"
      x.aliasTypeFullName shouldBe None
      x.inheritsFromTypeFullName shouldBe Seq.empty
    }

    "generate a type declaration with the correct modifiers" in {
      val x = cpg.typeDecl.nameExact("Container").head
      x.modifier.modifierType.head shouldBe ModifierTypes.PUBLIC
    }
  }

  "a basic class declaration within a namespace" should {
    val cpg = code(
      """namespace SampleNamespace
        |{
        |    private class SampleClass { }
        |}
        |""".stripMargin,
      "SampleClass.cs"
    )

    "generate a type declaration with the correct properties" in {
      val x = cpg.typeDecl.nameExact("SampleClass").head
      x.code shouldBe "private class SampleClass { }"
      x.fullName shouldBe "SampleNamespace.SampleClass"
      x.filename shouldBe "SampleClass.cs"
      x.aliasTypeFullName shouldBe None
      x.inheritsFromTypeFullName shouldBe Seq.empty
    }

    "generate a type declaration with the correct modifiers" in {
      val x = cpg.typeDecl.nameExact("SampleClass").head
      x.modifier.modifierType.head shouldBe ModifierTypes.PRIVATE
    }
  }

  "a basic struct declaration" should {
    val cpg = code("""
        |public struct Coords
        |{
        |    public double y;
        |}
        |""".stripMargin)

    "generate a type declaration with correct properties" in {
      inside(cpg.typeDecl.nameExact("Coords").headOption) { case Some(struct) =>
        struct.fullName shouldBe "Coords"

      }
    }

    "generate a type declaration with correct modifiers" in {
      inside(cpg.typeDecl.nameExact("Coords").headOption) { case Some(struct) =>
        struct.modifier.modifierType.head shouldBe ModifierTypes.PUBLIC
      }
    }

    "generate a type declaration with correct member" in {
      inside(cpg.typeDecl.nameExact("Coords").headOption) { case Some(struct) =>
        struct.member.name.head shouldBe "y"
      }
    }
  }

  "modern type and member modifiers" should {
    val cpg = code("""
        |file class Hidden { }
        |public sealed partial class Closed { }
        |public ref struct Buffer
        |{
        |    public required string Name { get; init; }
        |}
        |public readonly record struct Point(int X, int Y);
        |""".stripMargin)

    "preserve C#-specific modifiers" in {
      cpg.typeDecl.nameExact("Hidden").modifier.modifierType.toSet shouldBe Set(CSharpModifiers.FILE)
      cpg.typeDecl.nameExact("Closed").modifier.modifierType.toSet shouldBe Set(
        ModifierTypes.PUBLIC,
        ModifierTypes.FINAL,
        CSharpModifiers.PARTIAL
      )
      cpg.typeDecl.nameExact("Buffer").modifier.modifierType.toSet shouldBe Set(
        ModifierTypes.PUBLIC,
        CSharpModifiers.REF
      )
      cpg.typeDecl.nameExact("Point").modifier.modifierType.toSet shouldBe Set(
        ModifierTypes.PUBLIC,
        ModifierTypes.READONLY,
        CSharpModifiers.STRUCT
      )
      cpg.typeDecl
        .nameExact("Buffer")
        .member
        .nameExact("Name")
        .modifier
        .modifierType
        .toSet shouldBe Set(ModifierTypes.PUBLIC, CSharpModifiers.REQUIRED)
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "basic records declaration" should {
    val cpg = code("""
        |private record Person(string Name, string Mood);
        |
        |public record Car
        |{
        |    public string Model;
        |    public string Year;
        |};
        |""".stripMargin)

    "generate a type declaration with properties for first declaration style" in {
      inside(cpg.typeDecl.nameExact("Person").headOption) { case Some(rec) =>
        rec.fullName shouldBe "Person"
        rec.member.name.head shouldBe "Name"
        rec.member.name.last shouldBe "Mood"
        rec.modifier.modifierType.head shouldBe ModifierTypes.PRIVATE

      }
    }

    "generate a type declaration with properties for second declaration style" in {
      inside(cpg.typeDecl.nameExact("Car").headOption) { case Some(rec) =>
        rec.fullName shouldBe "Car"
        rec.member.name.head shouldBe "Model"
        rec.member.name.last shouldBe "Year"
        rec.modifier.modifierType.head shouldBe ModifierTypes.PUBLIC

      }
    }
  }

  "generic declarations with constraints" should {
    val cpg = code("""
        |public class Box<T, U> where T : class where U : new()
        |{
        |  public T Echo<V>(T item, V other) where V : struct { return item; }
        |}
        |
        |public delegate TResult Projector<T, TResult>(T item) where T : class;
        |""".stripMargin)

    "preserve type declaration generic signatures" in {
      cpg.typeDecl.nameExact("Box").genericSignature.l shouldBe List("<T,U> where T : class where U : new()")
      cpg.typeDecl.nameExact("Projector").genericSignature.l shouldBe List("<T,TResult> where T : class")
    }

    "preserve method generic signatures" in {
      cpg.method.nameExact("Echo").genericSignature.l shouldBe List("<V> where V : struct")
    }
  }

  "basic enum types" should {

    val cpg = code(
      """
        |enum Season
        |{
        |    Spring,
        |    Summer,
        |    Autumn,
        |    Winter
        |}
        |""".stripMargin,
      "Season.cs"
    )

    "generate a type declaration enum members" in {
      inside(cpg.typeDecl.nameExact("Season").headOption) { case Some(season) =>
        season.fullName shouldBe "Season"
        inside(season.member.l) { case spring :: summer :: autumn :: winter :: Nil =>
          spring.name shouldBe "Spring"
          spring.code shouldBe "Spring"
          spring.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.Int)

          summer.name shouldBe "Summer"
          summer.code shouldBe "Summer"
          summer.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.Int)

          autumn.name shouldBe "Autumn"
          autumn.code shouldBe "Autumn"
          autumn.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.Int)

          winter.name shouldBe "Winter"
          winter.code shouldBe "Winter"
          winter.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.Int)
        }
      }
    }
  }

  "a basic delegate declaration" should {
    val cpg = code(
      """
        |namespace Sample
        |{
        |    public delegate string Transformer(int value);
        |}
        |""".stripMargin,
      "Delegates.cs"
    )

    "generate a delegate type declaration and Invoke signature" in {
      inside(cpg.typeDecl.nameExact("Transformer").headOption) { case Some(delegate) =>
        delegate.fullName shouldBe "Sample.Transformer"
        delegate.filename shouldBe "Delegates.cs"
        delegate.inheritsFromTypeFullName shouldBe Seq("System.MulticastDelegate")
        delegate.modifier.modifierType.l should contain(ModifierTypes.PUBLIC)
      }

      inside(cpg.method.fullNameExact("Sample.Transformer.Invoke:System.String(System.Int32)").headOption) {
        case Some(invoke) =>
          invoke.name shouldBe "Invoke"
          invoke.signature shouldBe "System.String(System.Int32)"
          invoke.methodReturn.typeFullName shouldBe "System.String"
          invoke.parameter.sortBy(_.index).map(p => p.name -> p.typeFullName).l shouldBe List(
            "this"  -> "Sample.Transformer",
            "value" -> "System.Int32"
          )
      }
    }
  }

  "enum types cast as an integer type" should {

    val cpg = code("""
        |enum ErrorCode : ushort
        |{
        |    None = 0,
        |    Unknown = 1,
        |    ConnectionLost = 100,
        |    OutlierReading = 200
        |}
        |
        |""".stripMargin)

    "generate a type declaration enum members" in {
      inside(cpg.typeDecl.nameExact("ErrorCode").headOption) { case Some(errCode) =>
        errCode.fullName shouldBe "ErrorCode"
        inside(errCode.member.l) { case none :: unknown :: connectionLost :: outlierReading :: Nil =>
          none.name shouldBe "None"
          none.code shouldBe "None = 0"
          none.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.UShort)

          unknown.name shouldBe "Unknown"
          unknown.code shouldBe "Unknown = 1"
          unknown.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.UShort)

          connectionLost.name shouldBe "ConnectionLost"
          connectionLost.code shouldBe "ConnectionLost = 100"
          connectionLost.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.UShort)

          outlierReading.name shouldBe "OutlierReading"
          outlierReading.code shouldBe "OutlierReading = 200"
          outlierReading.typeFullName shouldBe DotNetTypeMap(BuiltinTypes.UShort)
        }
      }
    }

    "initialize the members in a <clinit> class" in {
      inside(cpg.typeDecl.nameExact("ErrorCode").method.nameExact(Defines.StaticInitMethodName).l) { case m :: Nil =>
        m.fullName shouldBe s"ErrorCode.${Defines.StaticInitMethodName}:System.Void()"
        inside(m.assignment.l) { case none :: unknown :: connectionLost :: outlierReading :: Nil =>
          none.code shouldBe "None = 0"
          unknown.code shouldBe "Unknown = 1"
          connectionLost.code shouldBe "ConnectionLost = 100"
          outlierReading.code shouldBe "OutlierReading = 200"
        }
      }
    }

  }

  "an interface" should {

    val cpg = code("""
        |namespace Foo {
        |
        | interface ISampleInterface
        | {
        |     void SampleMethod();
        | }
        |
        | class ImplementationClass : ISampleInterface
        | {
        |     // Explicit interface member implementation:
        |     void ISampleInterface.SampleMethod()
        |     {
        |         // Method implementation.
        |      }
        |
        |      static void Main()
        |      {
        |          // Declare an interface instance.
        |          ISampleInterface obj = new ImplementationClass();
        |
        |         // Call the member.
        |          obj.SampleMethod();
        |      }
        | }
        |
        |}
        |""".stripMargin)

    "have a corresponding TYPE_DECL node" in {
      inside(cpg.typeDecl.name("ISampleInterface").headOption) { case Some(typeDecl) =>
        typeDecl.fullName shouldBe "Foo.ISampleInterface"
        typeDecl.code shouldBe
          """interface ISampleInterface
              | {
              |     void SampleMethod();
              | }""".stripMargin
      }
    }

    "have a child method" in {
      inside(cpg.typeDecl.name("ISampleInterface").method.l) { case sampleMethod :: Nil =>
        sampleMethod.name shouldBe "SampleMethod"
        sampleMethod.code shouldBe "void SampleMethod();"
      }
    }

    "be inherited by the implementation class" in {
      inside(cpg.typeDecl.name("ISampleInterface", "ImplementationClass").l) {
        case interface :: implementation :: Nil =>
          implementation.inheritsFromTypeFullName shouldBe Seq(interface.fullName)
      }
    }
  }

  "an anonymous type with primitive type members" should {
    val cpg = code(basicBoilerplate("""
        |var Foo = new { Bar = 10, Baz = "Hello, World" };
        |""".stripMargin))

    "create a TypeDecl node" in {
      inside(cpg.method("Main").astChildren.isTypeDecl.l) { case anonType :: Nil =>
        anonType.fullName shouldBe "HelloWorld.Program.Main.<anon>0"
        anonType.astParentType shouldBe "METHOD"
        anonType.astParentFullName shouldBe "HelloWorld.Program.Main:System.Void(System.String[])"
      }
    }

    "propagate type to the LHS" in {
      inside(cpg.method("Main").astChildren.isBlock.astChildren.isLocal.nameExact("Foo").l) { case loc :: Nil =>
        loc.typeFullName shouldBe "HelloWorld.Program.Main.<anon>0"
      }
    }

    "have correct members" in {
      inside(cpg.method("Main").astChildren.isTypeDecl.l) { case anonType :: Nil =>
        inside(anonType.astChildren.isMember.l) { case bar :: baz :: Nil =>
          bar.code shouldBe "Bar = 10"
          baz.code shouldBe "Baz = \"Hello, World\""

          bar.typeFullName shouldBe "System.Int32"
          baz.typeFullName shouldBe "System.String"

          bar.astParent shouldBe anonType
          baz.astParent shouldBe anonType
        }
      }
    }
  }

  "an anonymous type with custom type members" should {
    val cpg = code("""
        |namespace Foo {
        | public class Qux {}
        | public class Bar {
        |   public static void Main() {
        |     var q = new Qux();
        |     var Fred = new { MBar = 10, q };
        |   }
        | }
        |
        |}
        |""".stripMargin)

    "create a TypeDecl node" in {
      inside(cpg.method("Main").astChildren.isTypeDecl.l) { case anonType :: Nil =>
        anonType.fullName shouldBe "Foo.Bar.Main.<anon>0"
        anonType.astParentType shouldBe "METHOD"
        anonType.astParentFullName shouldBe "Foo.Bar.Main:System.Void()"
      }
    }

    "propagate type to the LHS" in {
      inside(cpg.method("Main").astChildren.isBlock.astChildren.isLocal.nameExact("Fred").l) { case loc :: Nil =>
        loc.typeFullName shouldBe "Foo.Bar.Main.<anon>0"
      }
    }

    "have correct members" in {
      inside(cpg.method("Main").astChildren.isTypeDecl.l) { case anonType :: Nil =>
        inside(anonType.astChildren.isMember.l) { case bar :: q :: Nil =>
          bar.code shouldBe "MBar = 10"
          q.code shouldBe "q"

          bar.typeFullName shouldBe "System.Int32"
          q.typeFullName shouldBe "Foo.Qux"

          bar.astParent shouldBe anonType
          q.astParent shouldBe anonType
        }
      }
    }
  }

  "multiple anonymous types" should {
    val cpg = code(basicBoilerplate("""
          |var Foo = new { Bar = 10, Baz = "Hello, World" };
          |var Qux = new { Fred = 5 };
          |""".stripMargin))

    "have correct attributes" in {
      inside(cpg.method("Main").astChildren.isTypeDecl.l) { case anonType :: anonType2 :: Nil =>
        anonType.fullName shouldBe "HelloWorld.Program.Main.<anon>0"
        anonType.astParentType shouldBe "METHOD"
        anonType.astParentFullName shouldBe "HelloWorld.Program.Main:System.Void(System.String[])"

        anonType2.fullName shouldBe "HelloWorld.Program.Main.<anon>1"
        anonType2.astParentType shouldBe "METHOD"
        anonType2.astParentFullName shouldBe "HelloWorld.Program.Main:System.Void(System.String[])"
      }
    }

    "propagate type to the LHS" in {
      inside(cpg.method("Main").astChildren.isBlock.astChildren.isLocal.l) { case loc :: loc2 :: Nil =>
        loc.typeFullName shouldBe "HelloWorld.Program.Main.<anon>0"
        loc2.typeFullName shouldBe "HelloWorld.Program.Main.<anon>1"
      }
    }
  }

  "preprocessor directives" should {
    val cpg = code("""#!/usr/bin/env dotnet-script
        |#define FEATURE
        |#pragma warning disable CS0168
        |#nullable enable
        |#region R
        |#if FEATURE
        |namespace Foo {
        |  public class Enabled {
        |    public void M() {
        |#if FEATURE
        |      int value = 1;
        |#endif
        |    }
        |  }
        |}
        |#else
        |public class Disabled {}
        |#endif
        |#endregion
        |#undef FEATURE
        |#line 200 "Generated.cs"
        |#warning generated
        |""".stripMargin)

    "flatten branch members without unknown nodes" in {
      cpg.typeDecl.nameExact("Enabled").fullName.l shouldBe List("Foo.Enabled")
      cpg.method.nameExact("M").local.nameExact("value").typeFullName.l shouldBe List("System.Int32")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

}
