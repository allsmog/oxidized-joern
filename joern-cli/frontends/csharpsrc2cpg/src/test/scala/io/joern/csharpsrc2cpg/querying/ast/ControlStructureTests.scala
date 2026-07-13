package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.CSharpOperators
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.Block
import io.shiftleft.codepropertygraph.generated.nodes.Call
import io.shiftleft.codepropertygraph.generated.nodes.JumpTarget
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*

class ControlStructureTests extends CSharpCode2CpgFixture {

  "the throw statement" should {
    val cpg = code(basicBoilerplate("""
        |throw new Exception("Error!");
        |""".stripMargin))

    "create a throw operation with exception constructor" in {
      inside(cpg.call.nameExact(CSharpOperators.throws).headOption) { case Some(x: Call) =>
        x.methodFullName shouldBe CSharpOperators.throws
        x.code shouldBe "throw new Exception(\"Error!\");"
        x.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        inside(x.argumentOption(1)) { case Some(exp: Call) =>
          exp.code shouldBe "new Exception(\"Error!\")"
          exp.name shouldBe Defines.ConstructorMethodName
          exp.typeFullName shouldBe "System.Exception"
        }

      }
    }
  }

  "exception handling statements" should {

    val cpg = code(basicBoilerplate("""
        |var Busy = true;
        |try
        |{
        |  Console.WriteLine("Hello");
        |}
        |catch (Exception e)
        |{
        |  Console.WriteLine("Uh, oh!");
        |} finally
        |{
        | Busy = false;
        |}
        |""".stripMargin))

    val tryElements = cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).astChildren.l

    "generate a try control structure with three children correctly" in {
      val List(tryBlock) = tryElements.isBlock.l
      tryBlock.astChildren.isCall.code.l shouldBe List("""Console.WriteLine("Hello")""")
      val List(catchBlock) = tryElements.isControlStructure.isCatch.astChildren.l
      catchBlock.astChildren.isCall.code.l shouldBe List("""Console.WriteLine("Uh, oh!")""")
      val List(finallyBlock) = tryElements.isControlStructure.isFinally.astChildren.l
      finallyBlock.astChildren.isCall.code.l shouldBe List("""Busy = false""")
    }
  }

  "catch declarations without variable names" should {
    val cpg = code(basicBoilerplate("""
        |try
        |{
        |  Console.WriteLine("Hello");
        |}
        |catch (InvalidOperationException)
        |{
        |  Console.WriteLine("Typed");
        |}
        |catch (Exception ex) when (ex.Message != null)
        |{
        |  Console.WriteLine(ex.Message);
        |}
        |catch
        |{
        |  Console.WriteLine("Any");
        |}
        |""".stripMargin))

    "not create a bogus local for the exception type" in {
      cpg.local.nameExact("InvalidOperationException").size shouldBe 0
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "the switch statement" should {
    val cpg = code(basicBoilerplate("""
        |switch (i) {
        | case > 0:
        |   i++;
        |   break;
        | case < 0:
        |   i--;
        |   break;
        | default:
        |  i += 10;
        |  break;
        |}
        |""".stripMargin))

    "create a control structure node and contain correct astChildren" in {
      inside(cpg.method("Main").controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).l) {
        case switchNode :: Nil =>
          switchNode.code shouldBe "switch (i)";
          switchNode.controlStructureType shouldBe ControlStructureTypes.SWITCH

          val List(switchBody) = switchNode.astChildren.isBlock.l
          switchNode.trueBodyOut.isBlock.l shouldBe List(switchBody)

          inside(switchBody.astChildren.isBlock.l) { case case1 :: case2 :: case3 :: Nil =>
            val List(incCall) = case1.astChildren.isCall.l;
            incCall.code shouldBe "i++"

            val List(decCall) = case2.astChildren.isCall.l;
            decCall.code shouldBe "i--"

            val List(plusEqualsCall) = case3.astChildren.isCall.l;
            plusEqualsCall.code shouldBe "i += 10"
          }

          inside(switchBody.astChildren.collect { case j: JumpTarget => j }.l) {
            case case1 :: case2 :: defaultCase :: Nil =>
              case1.code shouldBe "case > 0:"
              case2.code shouldBe "case < 0:"
              defaultCase.code shouldBe "default:"
          }
      }
    }
  }

  "the switch expression" should {
    val cpg = code(basicBoilerplate("""
        |var result = i switch { (> 10) => 3, > 0 and < 10 => 1, 0 or 10 => 2, _ => 0 };
        |""".stripMargin))

    "create a switch operator call with arm condition/result pairs" in {
      inside(cpg.call.nameExact(CSharpOperators.switchExpression).l) { case switchCall :: Nil =>
        switchCall.code shouldBe "i switch { (> 10) => 3, > 0 and < 10 => 1, 0 or 10 => 2, _ => 0 }"
        switchCall.typeFullName shouldBe "System.Int32"
        switchCall.argument.code.l shouldBe List("i", "i > 10", "3", "> 0 and < 10", "1", "0 or 10", "2", "true", "0")
      }
      cpg.call.nameExact(Operators.logicalAnd).code.l should contain("> 0 and < 10")
      cpg.call.nameExact(Operators.logicalOr).code.l should contain("0 or 10")
      cpg.call.nameExact(Operators.greaterThan).code.l should contain allOf ("i > 10", "i > 0")
      cpg.call.nameExact(Operators.lessThan).code.l should contain("i < 10")
      cpg.call.nameExact(Operators.equals).code.l should contain allOf ("i == 0", "i == 10")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "the switch expression with list patterns" should {
    val cpg = code(basicBoilerplate("""
        |int[] values = new[] { 1, 2 };
        |var result = values switch { [1, 2] => 1, [1, ..] => 2, [] => 0, _ => -1 };
        |""".stripMargin))

    "create list pattern length and element conditions without unknown nodes" in {
      inside(cpg.call.nameExact(CSharpOperators.switchExpression).l) { case switchCall :: Nil =>
        switchCall.code shouldBe "values switch { [1, 2] => 1, [1, ..] => 2, [] => 0, _ => -1 }"
      }

      cpg.call.nameExact(Operators.indexAccess).code.l should contain allOf ("values[0]", "values[1]")
      cpg.call.nameExact(Operators.equals).code.l should contain allOf (
        "values.Length == 2",
        "values[0] == 1",
        "values[1] == 2",
        "values.Length == 0"
      )
      cpg.call.nameExact(Operators.greaterEqualsThan).code.l should contain("values.Length >= 1")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "the switch expression with recursive patterns" should {
    val cpg = code(basicBoilerplate("""
        |string text = "abcd";
        |var pair = (1, 2);
        |var tupleResult = pair switch { (1, > 0) => 1, (_, _) => 2, _ => 0 };
        |var propertyResult = text switch { { Length: > 3 } => 1, _ => 0 };
        |""".stripMargin))

    "create recursive pattern member conditions without unknown nodes" in {
      cpg.call.nameExact(CSharpOperators.switchExpression).code.l should contain allOf (
        "pair switch { (1, > 0) => 1, (_, _) => 2, _ => 0 }",
        "text switch { { Length: > 3 } => 1, _ => 0 }"
      )
      cpg.call.nameExact(Operators.fieldAccess).code.l should contain allOf ("pair.Item1", "pair.Item2", "text.Length")
      cpg.call.nameExact(Operators.equals).code.l should contain("pair.Item1 == 1")
      cpg.call.nameExact(Operators.greaterThan).code.l should contain allOf ("pair.Item2 > 0", "text.Length > 3")
      cpg.all.collectAll[Unknown].code.l.shouldBe(Nil)
    }
  }

  "switch statement with multiple labels" should {
    val cpg = code(basicBoilerplate("""
        |switch (i) {
        | case > 0:
        | case < 10:
        |   i++;
        |   break;
        | case 10:
        |   i--;
        |   break;
        | default:
        |  i += 10;
        |  break;
        |}
        |""".stripMargin))

    "create a control structure node with correct label and case clauses" in {

      inside(cpg.method("Main").controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).l) {
        case switchNode :: Nil =>
          switchNode.code shouldBe "switch (i)";
          switchNode.controlStructureType shouldBe ControlStructureTypes.SWITCH

          val List(switchBody) = switchNode.astChildren.isBlock.l

          inside(switchBody.astChildren.collect { case j: JumpTarget => j }.l) {
            case case1 :: case1_1 :: case2 :: defaultCase :: Nil =>
              case1.code shouldBe "case > 0:"
              case2.code shouldBe "case 10:"
              case1_1.code shouldBe "case < 10:"
              defaultCase.code shouldBe "default:"
          }
      }
    }

  }

  "a using statement" should {
    val cpg = code(basicBoilerplate("""
        |var numbers = new List<int>();
        |using (StreamReader reader = File.OpenText("numbers.txt"), backup = File.OpenText("backup.txt"))
        |{
        |    string line;
        |    while ((line = reader.ReadLine()) is not null)
        |    {
        |        if (int.TryParse(line, out int number))
        |        {
        |            numbers.Add(number);
        |        }
        |    }
        |}
        |""".stripMargin))

    val tryElements = cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).astChildren.l

    "generate a try control structure with two children correctly" in {
      val List(tryBlock) = tryElements.isBlock.l
      tryBlock.code shouldBe "try"
      val List(finallyBlock) = tryElements.isControlStructure.isFinally.astChildren.l
      finallyBlock.astChildren.isCall.code.l shouldBe List("backup.Dispose()", "reader.Dispose()")
      finallyBlock.astChildren.isCall.name.l shouldBe List("Dispose", "Dispose")
      finallyBlock.astChildren.isCall.methodFullName.l shouldBe List(
        "System.Disposable.Dispose:System.Void()",
        "System.Disposable.Dispose:System.Void()"
      )
    }

  }

  "a using declaration" should {
    val cpg = code(basicBoilerplate("""
        |using StreamReader reader = File.OpenText("numbers.txt"), backup = File.OpenText("backup.txt");
        |reader.ReadLine();
        |Console.WriteLine("done");
        |""".stripMargin))

    "lower the remaining scope as a try-finally with a dispose call" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).l) { case usingTry :: Nil =>
        usingTry.code shouldBe """using StreamReader reader = File.OpenText("numbers.txt"), backup = File.OpenText("backup.txt");"""
        inside(usingTry.astChildren.isBlock.l) { case tryBlock :: Nil =>
          tryBlock.code shouldBe "try"
          tryBlock.astChildren.isCall.code.l should contain theSameElementsAs List(
            "reader.ReadLine()",
            """Console.WriteLine("done")"""
          )
        }

        inside(usingTry.astChildren.isControlStructure.isFinally.astChildren.isBlock.l) { case finallyBlock :: Nil =>
          finallyBlock.astChildren.isCall.code.l shouldBe List("backup.Dispose()", "reader.Dispose()")
          finallyBlock.astChildren.isCall.name.l shouldBe List("Dispose", "Dispose")
          finallyBlock.astChildren.isCall.methodFullName.l shouldBe List(
            "System.Disposable.Dispose:System.Void()",
            "System.Disposable.Dispose:System.Void()"
          )
        }
      }
    }
  }

  "an await using declaration" should {
    val cpg = code("""
        |using System;
        |using System.Threading.Tasks;
        |
        |namespace Foo;
        |
        |class Bar {
        |  async Task Main() {
        |    await using IAsyncDisposable reader = Make(), backup = Make();
        |    reader.ToString();
        |  }
        |
        |  IAsyncDisposable Make() => null;
        |}
        |""".stripMargin)

    "lower the remaining scope as a try-finally with awaited async dispose calls" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).l) { case usingTry :: Nil =>
        usingTry.code shouldBe "await using IAsyncDisposable reader = Make(), backup = Make();"
        inside(usingTry.astChildren.isControlStructure.isFinally.astChildren.isBlock.l) { case finallyBlock :: Nil =>
          finallyBlock.astChildren.isCall.code.l shouldBe List(
            "await backup.DisposeAsync()",
            "await reader.DisposeAsync()"
          )
          finallyBlock.astChildren.isCall.name.l shouldBe List(CSharpOperators.await, CSharpOperators.await)
          cpg.call.nameExact("DisposeAsync").code.l shouldBe List("backup.DisposeAsync()", "reader.DisposeAsync()")
        }
      }
    }
  }

  "an await using statement" should {
    val cpg = code("""
        |using System;
        |using System.Threading.Tasks;
        |
        |namespace Foo;
        |
        |class Bar {
        |  async Task Main() {
        |    await using (IAsyncDisposable reader = Make(), backup = Make()) {
        |      reader.ToString();
        |    }
        |  }
        |
        |  IAsyncDisposable Make() => null;
        |}
        |""".stripMargin)

    "lower the using scope as a try-finally with awaited async dispose calls" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).l) { case usingTry :: Nil =>
        usingTry.code shouldBe "await using (IAsyncDisposable reader = Make(), backup = Make()) {\n      reader.ToString();\n    }"
        inside(usingTry.astChildren.isControlStructure.isFinally.astChildren.isBlock.l) { case finallyBlock :: Nil =>
          finallyBlock.astChildren.isCall.code.l shouldBe List(
            "await backup.DisposeAsync()",
            "await reader.DisposeAsync()"
          )
          finallyBlock.astChildren.isCall.name.l shouldBe List(CSharpOperators.await, CSharpOperators.await)
          cpg.call.nameExact("DisposeAsync").code.l shouldBe List("backup.DisposeAsync()", "reader.DisposeAsync()")
        }
      }
    }
  }

  "a variable defined within a using statement" should {
    val cpg = code("""
        |namespace other
        |{
        |    public class General
        |    {
        |        public static void Call(string name)
        |        {
        |            using (SqlConnection connection = new SqlConnection(name))
        |            {
        |                try
        |                {
        |                    connection.Open();
        |                }
        |                catch (Exception ex)
        |                {
        |                    Console.WriteLine(ex.Message);
        |                    connection.Close();
        |                }
        |            }
        |        }
        |    }
        |}
        |""".stripMargin)

    "partially resolve calls on the defined variable" in {
      inside(cpg.call.name("Open").methodFullName.l) { case x :: Nil =>
        x shouldBe "SqlConnection.Open:<unresolvedSignature>"
      }
    }
  }

  "a lock statement" should {
    val cpg = code(basicBoilerplate("""
        |object gate = new object();
        |lock (gate)
        |{
        |    Console.WriteLine("locked");
        |}
        |""".stripMargin))

    "generate a synchronized block with the lock expression and body" in {
      inside(cpg.method("Main").ast.isBlock.where(_.astChildren.isModifier.modifierType("SYNCHRONIZED")).l) {
        case syncBlock :: Nil =>
          syncBlock.astChildren.isModifier.modifierType.l shouldBe List("SYNCHRONIZED")
          syncBlock.astChildren.isIdentifier.code.l shouldBe List("gate")
          inside(syncBlock.astChildren.isBlock.l) { case body :: Nil =>
            body.astChildren.isCall.code.l shouldBe List("""Console.WriteLine("locked")""")
          }
      }
    }
  }

  "checked and unchecked statements" should {
    val cpg = code(basicBoilerplate("""
        |int value = 0;
        |checked
        |{
        |    value += 1;
        |}
        |unchecked
        |{
        |    value -= 1;
        |}
        |int result = checked(value + 1);
        |""".stripMargin))

    "generate overflow-context blocks and preserve checked expressions" in {
      inside(cpg.method("Main").ast.isBlock.where(_.astChildren.isModifier.modifierType("CHECKED")).l) {
        case checkedBlock :: Nil =>
          checkedBlock.astChildren.isModifier.modifierType.l shouldBe List("CHECKED")
          inside(checkedBlock.astChildren.isBlock.l) { case body :: Nil =>
            body.astChildren.isCall.code.l shouldBe List("value += 1")
          }
      }

      inside(cpg.method("Main").ast.isBlock.where(_.astChildren.isModifier.modifierType("UNCHECKED")).l) {
        case uncheckedBlock :: Nil =>
          uncheckedBlock.astChildren.isModifier.modifierType.l shouldBe List("UNCHECKED")
          inside(uncheckedBlock.astChildren.isBlock.l) { case body :: Nil =>
            body.astChildren.isCall.code.l shouldBe List("value -= 1")
          }
      }

      cpg.call.codeExact("value + 1").size shouldBe 1
    }
  }

  "an unsafe statement" should {
    val cpg = code(basicBoilerplate("""
        |int value = 0;
        |unsafe
        |{
        |    value += 1;
        |}
        |""".stripMargin))

    "generate an unsafe block with its body" in {
      inside(cpg.method("Main").ast.isBlock.where(_.astChildren.isModifier.modifierType("UNSAFE")).l) {
        case unsafeBlock :: Nil =>
          unsafeBlock.astChildren.isModifier.modifierType.l shouldBe List("UNSAFE")
          inside(unsafeBlock.astChildren.isBlock.l) { case body :: Nil =>
            body.astChildren.isCall.code.l shouldBe List("value += 1")
          }
      }
    }
  }

  "a fixed statement" should {
    val cpg = code(basicBoilerplate("""
        |int[] values = null;
        |int total = 0;
        |unsafe
        |{
        |    fixed (int* p = values)
        |    {
        |        total += 1;
        |    }
        |}
        |""".stripMargin))

    "generate a fixed block with its pinned declaration and body" in {
      inside(cpg.method("Main").ast.isBlock.where(_.astChildren.isModifier.modifierType("FIXED")).l) {
        case fixedBlock :: Nil =>
          fixedBlock.astChildren.isModifier.modifierType.l shouldBe List("FIXED")
          fixedBlock.astChildren.isLocal.nameExact("p").typeFullName.l shouldBe List("System.Int32*")
          fixedBlock.astChildren.isCall.code.l shouldBe List("p = values")
          inside(fixedBlock.astChildren.isBlock.l) { case body :: Nil =>
            body.astChildren.isCall.code.l shouldBe List("total += 1")
          }
      }
    }
  }

}
