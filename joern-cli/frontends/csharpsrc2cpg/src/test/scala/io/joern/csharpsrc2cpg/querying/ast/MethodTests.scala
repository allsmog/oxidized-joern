package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.CSharpModifiers
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.{ModifierTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.{Return, Unknown}
import io.shiftleft.semanticcpg.language.*

class MethodTests extends CSharpCode2CpgFixture {

  "a basic class declaration with method" should {
    val cpg = code(basicBoilerplate(), "Program.cs")

    "generate a method node with type decl parent" in {
      val x = cpg.method.nameExact("Main").head
      x.fullName should startWith("HelloWorld.Program.Main:System.Void")
      x.fullName shouldBe "HelloWorld.Program.Main:System.Void(System.String[])"
      x.signature shouldBe "System.Void(System.String[])"
      x.filename shouldBe "Program.cs"
      x.code shouldBe
        """static void Main(string[] args)
          |    {
          |      Console.WriteLine("Hello, world!");
          |    }""".stripMargin
      inside(x.typeDecl) { case Some(typeDecl) =>
        typeDecl.name shouldBe "Program"
      }
    }

    "generate a method node with the correct modifiers" in {
      val List(x, y) = cpg.method.nameExact("Main").modifier.l: @unchecked
      x.modifierType shouldBe ModifierTypes.INTERNAL
      y.modifierType shouldBe ModifierTypes.STATIC
    }

    "generate a method node with a parameter" in {
      val List(x) = cpg.method.nameExact("Main").parameter.l: @unchecked
      x.name shouldBe "args"
    }

    "generate a method node with a block" in {
      cpg.method.nameExact("Main").body.l should not be empty
    }
  }

  "basic method with a return statement" should {
    val cpg = code("""
        |using System;
        |namespace HelloWorld
        |{
        |  class Program
        |  {
        |    static void Main(string[] args) {}
        |    
        |    public int getInt(int foo) {
        |       return foo;
        |    }
        |  }
        |}
        |""".stripMargin)

    "have correct method properties" in {
      inside(cpg.method("getInt").l) { case methodNode :: Nil =>
        methodNode.name shouldBe "getInt"
        methodNode.fullName shouldBe "HelloWorld.Program.getInt:System.Int32(System.Int32)"
        methodNode.code should startWith("public int getInt(int foo)")
        methodNode.signature shouldBe "System.Int32(System.Int32)"
        methodNode.isExternal shouldBe false

        methodNode.order shouldBe 3
        methodNode.filename shouldBe "Test0.cs"
        methodNode.lineNumber shouldBe Option(8)
        methodNode.lineNumberEnd shouldBe Option(10)
      }
    }

    "have correct return information" in {
      val List(methodReturnNode) = cpg.method.name("getInt").methodReturn.l
      methodReturnNode.typeFullName shouldBe "System.Int32"
    }
  }

  "empty public abstract method" should {
    val cpg = code("""
        |abstract class C
        |{
        | public abstract void DoStuff();
        |}
        |""".stripMargin)

    "have correct modifiers" in {
      cpg.method.nameExact("DoStuff").modifier.modifierType.sorted.l shouldBe List(
        ModifierTypes.ABSTRACT,
        ModifierTypes.PUBLIC
      )
    }
  }

  "empty protected abstract method" should {
    val cpg = code("""
        |abstract class C
        |{
        |  protected abstract void DoStuff();
        |}""".stripMargin)

    "have correct modifiers" in {
      cpg.method.nameExact("DoStuff").modifier.modifierType.sorted.l shouldBe List(
        ModifierTypes.ABSTRACT,
        ModifierTypes.PROTECTED
      )
    }
  }

  "overriding method" should {
    val cpg = code("""
        |abstract class Base
        |{
        |  public abstract string Name();
        |}
        |
        |class Derived : Base
        |{
        |  public override string Name() { return "derived"; }
        |}
        |""".stripMargin)

    "have correct modifiers" in {
      cpg.method.nameExact("Name").code(".*override.*").modifier.modifierType.toSet shouldBe Set(
        CSharpModifiers.OVERRIDE,
        ModifierTypes.PUBLIC
      )
    }
  }

  "standalone method declaration inside a top-level method" should {
    val cpg = code("""
        |int MyMain()
        |{
        |   int MySubMethod() {return 1;}
        |}
        |""".stripMargin)

    "have correct properties for the nested method" in {
      inside(cpg.method.nameExact("MySubMethod").l) { case sub :: Nil =>
        sub.fullName shouldBe "Test0_cs_Program.<Main>$.MyMain.MySubMethod:System.Int32()"
        sub.signature shouldBe "System.Int32()"
        sub.modifier.modifierType.sorted.l shouldBe List(ModifierTypes.INTERNAL)
        sub.methodReturn.typeFullName shouldBe "System.Int32"
        sub.parentBlock.method.l shouldBe cpg.method.fullNameExact("Test0_cs_Program.<Main>$.MyMain:System.Int32()").l
      }
    }

    "have correct body for the nested method" in {
      inside(cpg.method.nameExact("MySubMethod").block.astChildren.l) { case (ret: Return) :: Nil =>
        ret.code shouldBe "return 1;"
      }
    }
  }

  "standalone method declaration inside a class method" should {
    val cpg = code("""
        |class MyClass
        |{
        |   int MyMain()
        |   {
        |     int MySubMethod() {return 1;}
        |   }
        |}
        |""".stripMargin)

    "have correct properties for the nested method" in {
      inside(cpg.method.nameExact("MySubMethod").l) { case sub :: Nil =>
        sub.fullName shouldBe "MyClass.MyMain.MySubMethod:System.Int32()"
        sub.signature shouldBe "System.Int32()"
        sub.modifier.modifierType.sorted.l shouldBe List(ModifierTypes.INTERNAL)
        sub.methodReturn.typeFullName shouldBe "System.Int32"
        sub.parentBlock.method.l shouldBe cpg.method.fullNameExact("MyClass.MyMain:System.Int32()").l
      }
    }

    "have correct body for the nested method" in {
      inside(cpg.method.nameExact("MySubMethod").block.astChildren.l) { case (ret: Return) :: Nil =>
        ret.code shouldBe "return 1;"
      }
    }
  }

  "constructor initializers, explicit interface implementations, and catch filters" should {
    val cpg = code("""
        |using System;
        |
        |interface IWorker
        |{
        |  void Work();
        |  int Count { get; }
        |  int this[int index] { get; }
        |}
        |
        |class WorkerBase
        |{
        |  public WorkerBase(int seed) { }
        |}
        |
        |class Worker(int seed) : WorkerBase(seed), IWorker
        |{
        |  public Worker() : this(1) { }
        |  public Worker(string text) : base(text.Length) { }
        |  void IWorker.Work() { }
        |  int IWorker.Count => seed;
        |  int IWorker.this[int index] => index + seed;
        |
        |  public void Guard(Action action)
        |  {
        |    try { action(); }
        |    catch (InvalidOperationException ex) when (ex.Message != null)
        |    {
        |      Console.WriteLine(ex.Message);
        |    }
        |  }
        |}
        |""".stripMargin)

    "keep primary constructor base arguments attached to the base type" in {
      cpg.typeDecl.nameExact("Worker").inheritsFromTypeFullName.l shouldBe List("WorkerBase", "IWorker")
    }

    "create a method for the primary constructor" in {
      inside(cpg.method.fullNameExact(s"Worker.${Defines.ConstructorMethodName}:System.Void(System.Int32)").l) {
        case ctor :: Nil =>
          ctor.modifier.modifierType.toSet should contain(ModifierTypes.CONSTRUCTOR)
          ctor.parameter.name.l shouldBe List("this", "seed")
          ctor.parameter.nameExact("seed").typeFullName.l shouldBe List("System.Int32")
          ctor.body.astChildren.isCall.nameExact(Defines.ConstructorMethodName).code.l should contain("WorkerBase(seed)")
      }
    }

    "create calls for constructor initializers" in {
      cpg.call.nameExact(Defines.ConstructorMethodName).code.l should contain allOf (": this(1)", ": base(text.Length)")
    }

    "preserve explicit interface prefixes in emitted members and methods" in {
      cpg.method.nameExact("IWorker.Work").size shouldBe 1
      cpg.method.nameExact("get_IWorker.Count").size shouldBe 1
      cpg.method.nameExact("get_IWorker.Item").size shouldBe 1
      cpg.member.nameExact("IWorker.Count").typeFullName.l shouldBe List("System.Int32")
      cpg.member.nameExact("IWorker.Item").typeFullName.l shouldBe List("System.Int32")
    }

    "lower catch filters without unknown nodes" in {
      cpg.call.nameExact(Operators.notEquals).code.l should contain("ex.Message != null")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "record primary constructors" should {
    val cpg = code("""
        |public record Person(string Name);
        |public record Employee(string Name, int Id) : Person(Name);
        |""".stripMargin)

    "preserve primary-constructor base inheritance" in {
      cpg.typeDecl.nameExact("Employee").inheritsFromTypeFullName.l shouldBe List("Person")
    }

    "create a constructor method for positional record parameters" in {
      inside(cpg.method.fullNameExact(s"Employee.${Defines.ConstructorMethodName}:System.Void(System.String,System.Int32)").l) {
        case ctor :: Nil =>
          ctor.modifier.modifierType.toSet should contain(ModifierTypes.CONSTRUCTOR)
          ctor.parameter.name.l shouldBe List("this", "Name", "Id")
          ctor.parameter.nameExact("Name").typeFullName.l shouldBe List("System.String")
          ctor.parameter.nameExact("Id").typeFullName.l shouldBe List("System.Int32")
          ctor.body.astChildren.isCall.nameExact(Defines.ConstructorMethodName).code.l should contain("Person(Name)")
      }
    }
  }
}
