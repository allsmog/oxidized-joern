package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.nodes.{Call, Identifier, Literal, TypeRef, Unknown}
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, NodeTypes, Operators}
import io.shiftleft.semanticcpg.language.*

class PatternMatchingTests extends CSharpCode2CpgFixture {

  "Pattern matching to extract the non-null value in an if-statement" should {
    val cpg = code(basicBoilerplate("""
        |int? maybe = 12;
        |
        |if (maybe is int number)
        |{
        |    Console.WriteLine($"The nullable int 'maybe' has the value {number}");
        |}
        |else
        |{
        |    Console.WriteLine("The nullable int 'maybe' doesn't hold a value");
        |}
        |""".stripMargin))

    "lower an assignment from `maybe` to `number` as the first statement of the if-body" in {
      inside(cpg.assignment.where(_.target.isIdentifier.name("number")).headOption) { case Some(assignment) =>
        assignment.order shouldBe 1
        assignment.inAst.exists(_.label == NodeTypes.CONTROL_STRUCTURE) shouldBe true

        inside(assignment.argument.l) { case (number: Identifier) :: (maybe: Identifier) :: Nil =>
          number.name shouldBe "number"
          number.typeFullName shouldBe "System.Int32"

          maybe.name shouldBe "maybe"
          maybe._astIn.size shouldBe 1
          maybe.typeFullName shouldBe "System.Int32"
        }

      }
    }

    "have an instanceOf-style check as the if-condition" in {
      inside(cpg.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.headOption) {
        case Some(condition: Call) =>
          condition.name shouldBe Operators.instanceOf
          inside(condition.argument.l) { case (maybe: Identifier) :: (intType: TypeRef) :: Nil =>
            maybe.name shouldBe "maybe"
            maybe.typeFullName shouldBe "System.Int32"

            intType.typeFullName shouldBe "System.Int32"
          }

      }
    }
  }

  "Pattern matching with null type check" should {
    val cpg = code(basicBoilerplate("""
      |int? maybe = 12;
      |
      |if (maybe is null)
      |{
      |    Console.WriteLine($"The nullable int 'maybe' has the value {number}");
      |}
      |""".stripMargin))

    "have equals check in if statement" in {
      inside(cpg.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.headOption) {
        case Some(condition: Call) =>
          condition.name shouldBe Operators.equals

          inside(condition.argument.l) { case (maybe: Identifier) :: (nullType: Literal) :: Nil =>
            maybe.name shouldBe "maybe"
            maybe.typeFullName shouldBe "System.Int32"

            nullType.typeFullName shouldBe "null"
          }
      }
    }
  }

  "Pattern matching with and/or patterns" should {
    val cpg = code(basicBoilerplate("""
      |int i = 5;
      |
      |if (i is > 0 and < 10)
      |{
      |    Console.WriteLine(i);
      |}
      |
      |if (i is 0 or 10)
      |{
      |    Console.WriteLine(i);
      |}
      |""".stripMargin))

    "lower composed pattern conditions without unknown nodes" in {
      cpg.call.nameExact(Operators.logicalAnd).code.l should contain("> 0 and < 10")
      cpg.call.nameExact(Operators.logicalOr).code.l should contain("0 or 10")
      cpg.call.nameExact(Operators.greaterThan).code.l should contain("i > 0")
      cpg.call.nameExact(Operators.lessThan).code.l should contain("i < 10")
      cpg.call.nameExact(Operators.equals).code.l should contain allOf ("i == 0", "i == 10")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "Pattern matching with parenthesized patterns" should {
    val cpg = code(basicBoilerplate("""
      |int i = 5;
      |
      |if (i is (> 0))
      |{
      |    Console.WriteLine(i);
      |}
      |""".stripMargin))

    "lower the inner pattern condition without unknown nodes" in {
      cpg.call.nameExact(Operators.greaterThan).code.l should contain("i > 0")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "Pattern matching with list patterns" should {
    val cpg = code(basicBoilerplate("""
      |int[] values = new[] { 1, 2 };
      |
      |if (values is [1, ..])
      |{
      |    Console.WriteLine(values[0]);
      |}
      |""".stripMargin))

    "lower list pattern conditions without unknown nodes" in {
      cpg.call.nameExact(Operators.greaterEqualsThan).code.l should contain("values.Length >= 1")
      cpg.call.nameExact(Operators.indexAccess).code.l should contain("values[0]")
      cpg.call.nameExact(Operators.equals).code.l should contain("values[0] == 1")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "Pattern matching with recursive patterns" should {
    val cpg = code(basicBoilerplate("""
      |string text = "abcd";
      |var pair = (1, 2);
      |
      |if (text is { Length: > 3 })
      |{
      |    Console.WriteLine(text);
      |}
      |
      |if (pair is (1, > 0))
      |{
      |    Console.WriteLine(pair);
      |}
      |""".stripMargin))

    "lower property and positional pattern conditions without unknown nodes" in {
      cpg.call.nameExact(Operators.fieldAccess).code.l should contain allOf ("text.Length", "pair.Item1", "pair.Item2")
      cpg.call.nameExact(Operators.greaterThan).code.l should contain allOf ("text.Length > 3", "pair.Item2 > 0")
      cpg.call.nameExact(Operators.equals).code.l should contain("pair.Item1 == 1")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

  "Pattern matching with tuple designations and type patterns" should {
    val cpg = code(basicBoilerplate("""
      |var pair = ((object)1, (object)"two");
      |var (left, right) = pair;
      |
      |var result = pair switch
      |{
      |    (var x, _) => 1,
      |    (_, string) => 2,
      |    _ => 0,
      |};
      |""".stripMargin))

    "lower deconstruction bindings and tuple type patterns without unknown nodes" in {
      cpg.local.nameExact("left").size shouldBe 1
      cpg.local.nameExact("right").size shouldBe 1
      cpg.assignment.code.l should contain allOf ("left = pair.Item1", "right = pair.Item2")
      cpg.call.nameExact(Operators.instanceOf).code.l should contain("pair.Item2 is string")
      cpg.all.collectAll[Unknown].code.l shouldBe Nil
    }
  }

}
