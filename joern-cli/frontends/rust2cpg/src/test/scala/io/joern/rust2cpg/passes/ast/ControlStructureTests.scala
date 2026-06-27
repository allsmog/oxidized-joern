package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.astcreation.RustOperators
import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class ControlStructureTests extends Rust2CpgSuite(noSysRoot = true) {

  "an if without an else" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) {
        | if x > y {
        |  foo();
        | }
        |}
        |""".stripMargin)

    "have correct code" in {
      cpg.ifBlock.code.l shouldBe List("if x > y {\n  foo();\n }")
    }

    "lower the condition as a > call" in {
      inside(cpg.ifBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "x > y"
        condition.name shouldBe Operators.greaterThan
        condition.methodFullName shouldBe Operators.greaterThan
      }
    }

    "have x and y as arguments to the > call" in {
      cpg.ifBlock.condition.isCall.argument.isIdentifier.name.l shouldBe List("x", "y")
    }

    "place foo in the then-branch" in {
      cpg.ifBlock.whenTrue.isBlock.astChildren.isCall.name.l shouldBe List("foo")
    }

    "have no else-branch" in {
      cpg.ifBlock.whenFalse shouldBe empty
    }
  }

  "an if with an else" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) {
        | if x == y {
        |  foo();
        | } else {
        |  bar();
        | }
        |}
        |""".stripMargin)

    "lower the condition as an == call" in {
      inside(cpg.ifBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "x == y"
        condition.name shouldBe Operators.equals
        condition.methodFullName shouldBe Operators.equals
      }
    }

    "place foo in the then-branch" in {
      cpg.ifBlock.whenTrue.isBlock.astChildren.isCall.name.l shouldBe List("foo")
    }

    "place bar in the ELSE body" in {
      cpg.elseBlock.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("bar")
    }
  }

  "an else-if chain" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) {
        | if x < y {
        |  foo();
        | } else if x == y {
        |  bar();
        | } else {
        |  baz();
        | }
        |}
        |""".stripMargin)

    "have one IF per if" in {
      cpg.ifBlock.size shouldBe 2
    }

    "place the inner IF inside the outer ELSE" in {
      inside(cpg.ifBlock.condition("x < y").whenFalse.l) { case (outerElse: ControlStructure) :: Nil =>
        outerElse.controlStructureType shouldBe ControlStructureTypes.ELSE
        inside(outerElse.astChildren.l) { case (innerIf: ControlStructure) :: Nil =>
          innerIf.controlStructureType shouldBe ControlStructureTypes.IF
          innerIf.condition.code.l shouldBe List("x == y")
        }
      }
    }

    "place baz inside the inner ELSE" in {
      inside(cpg.ifBlock.condition("x == y").whenFalse.l) { case (innerElse: ControlStructure) :: Nil =>
        innerElse.controlStructureType shouldBe ControlStructureTypes.ELSE
        innerElse.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("baz")
      }
    }
  }

  "a nested if" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) {
        | if x < y {
        |  if x == 0 {
        |   foo();
        |  }
        | }
        |}
        |""".stripMargin)

    "have one IF per if" in {
      cpg.ifBlock.size shouldBe 2
    }

    "place the inner IF in the outer then-branch" in {
      cpg.ifBlock
        .condition("x < y")
        .whenTrue
        .isBlock
        .astChildren
        .isControlStructure
        .isIf
        .condition
        .code
        .l shouldBe List("x == 0")
    }
  }

  "an if let expression" should {
    val cpg = code("""
        |fn main(maybe: Option<i32>) {
        | if let Some(value) = maybe {
        |  sink(value);
        | }
        |}
        |""".stripMargin)

    "lower the condition as a pattern match call" in {
      inside(cpg.ifBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "let Some(value) = maybe"
        condition.name shouldBe RustOperators.matches
        condition.methodFullName shouldBe RustOperators.matches
        condition.typeFullName shouldBe "bool"
      }
    }

    "preserve the pattern and matched expression as arguments" in {
      inside(cpg.ifBlock.condition.isCall.argument.l) { case (pattern: Literal) :: (matched: Identifier) :: Nil =>
        pattern.code shouldBe "Some(value)"
        matched.code shouldBe "maybe"
      }
    }

    "place the body under the then branch" in {
      cpg.ifBlock.whenTrue.isBlock.astChildren.isCall.name.l shouldBe List("sink")
    }

    "not create an unknown node for the let expression" in {
      cpg.all.collectAll[Unknown].codeExact("let Some(value) = maybe").l shouldBe empty
    }
  }

  "a while loop" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) {
        | while x < y {
        |  foo();
        | }
        |}
        |""".stripMargin)

    "have correct code" in {
      cpg.whileBlock.code.l shouldBe List("while x < y {\n  foo();\n }")
    }

    "lower the condition as a < call" in {
      inside(cpg.whileBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "x < y"
        condition.name shouldBe Operators.lessThan
        condition.methodFullName shouldBe Operators.lessThan
      }
    }

    "have x and y as arguments to the < call" in {
      cpg.whileBlock.condition.isCall.argument.isIdentifier.name.l shouldBe List("x", "y")
    }

    "place foo in the loop body" in {
      cpg.whileBlock.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("foo")
    }
  }

  "a while let expression" should {
    val cpg = code("""
        |fn main(maybe: Option<i32>) {
        | while let Some(value) = maybe {
        |  sink(value);
        | }
        |}
        |""".stripMargin)

    "lower the condition as a pattern match call" in {
      inside(cpg.whileBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "let Some(value) = maybe"
        condition.name shouldBe RustOperators.matches
        condition.methodFullName shouldBe RustOperators.matches
        condition.typeFullName shouldBe "bool"
      }
    }

    "preserve the pattern and matched expression as arguments" in {
      inside(cpg.whileBlock.condition.isCall.argument.l) { case (pattern: Literal) :: (matched: Identifier) :: Nil =>
        pattern.code shouldBe "Some(value)"
        matched.code shouldBe "maybe"
      }
    }

    "place the body inside the loop" in {
      cpg.whileBlock.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("sink")
    }

    "not create an unknown node for the let expression" in {
      cpg.all.collectAll[Unknown].codeExact("let Some(value) = maybe").l shouldBe empty
    }
  }

  "a loop expression" should {
    val cpg = code("""
        |fn main() {
        | loop {
        |  foo();
        |  break;
        | }
        |}
        |""".stripMargin)

    "lower as a WHILE with correct code" in {
      cpg.whileBlock.code.l shouldBe List("loop {\n  foo();\n  break;\n }")
    }

    "have a fake true literal as condition" in {
      inside(cpg.whileBlock.condition.isLiteral.l) { case condition :: Nil =>
        condition.code shouldBe "true"
        condition.typeFullName shouldBe "bool"
      }
    }

    "place foo in the loop body" in {
      cpg.whileBlock.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("foo")
    }

    "place break in the loop body" in {
      cpg.whileBlock.astChildren.isBlock.astChildren.isControlStructure.isBreak.code.l shouldBe List("break")
    }
  }

  "a for loop over a range" should {
    val cpg = code("""
        |fn main() {
        | for i in 0..3 {
        |  sink(i);
        | }
        |}
        |""".stripMargin)

    "lower as a FOR control structure" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.FOR).l) { case forNode :: Nil =>
        forNode.code shouldBe "for i in 0..3 {\n  sink(i);\n }"
      }
    }

    "create a local for the loop variable" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.FOR).astChildren.isLocal.l) {
        case i :: Nil =>
          i.name shouldBe "i"
          i.typeFullName shouldBe "i32"
      }
    }

    "use the iterable range as the loop condition" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.FOR).condition.isCall.l) {
        case range :: Nil =>
          range.name shouldBe Operators.range
          range.code shouldBe "0..3"
      }
    }

    "place sink in the loop body" in {
      cpg.controlStructure
        .controlStructureTypeExact(ControlStructureTypes.FOR)
        .forBodyOut
        .isBlock
        .astChildren
        .isCall
        .name
        .l shouldBe List("sink")
    }

    "not create an unknown node for the for loop or range" in {
      cpg.all.collectAll[Unknown].codeExact("for i in 0..3 {\n  sink(i);\n }", "0..3").l shouldBe empty
    }
  }

  "a for loop with a destructuring pattern" should {
    val forCode = "for (left, right) in pairs {\n  sink(left);\n  sink(right);\n }"
    val cpg = code(s"""
        |fn main(pairs: [(i32, i32); 1]) {
        | $forCode
        |}
        |""".stripMargin)

    "create locals for each pattern binding" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.FOR).astChildren.isLocal.l) {
        case left :: right :: Nil =>
          left.name shouldBe "left"
          right.name shouldBe "right"
      }
    }

    "use the iterable as the loop condition" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.FOR).condition.isIdentifier.l) {
        case pairs :: Nil =>
          pairs.code shouldBe "pairs"
      }
    }

    "not create an unknown node for the destructuring pattern" in {
      cpg.all.collectAll[Unknown].codeExact(forCode, "(left, right)").l shouldBe empty
    }
  }

  "a match expression" should {
    val cpg = code("""
        |fn classify(x: i32) -> i32 {
        | match x {
        |  0 => 1,
        |  1 => 2,
        |  _ => 3,
        | }
        |}
        |""".stripMargin)

    "lower as a SWITCH control structure" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).l) { case switchNode :: Nil =>
        switchNode.code shouldBe "match x {\n  0 => 1,\n  1 => 2,\n  _ => 3,\n }"
      }
    }

    "use the matched expression as the switch condition" in {
      inside(cpg.controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).condition.isIdentifier.l) {
        case x :: Nil =>
          x.name shouldBe "x"
          x.typeFullName shouldBe "i32"
      }
    }

    "preserve arm labels" in {
      val labels = cpg.controlStructure
        .controlStructureTypeExact(ControlStructureTypes.SWITCH)
        .ast
        .collectAll[JumpTarget]
        .code
        .l
      labels shouldBe List("case 0", "case 1", "default")
    }

    "preserve arm expressions" in {
      cpg.controlStructure
        .controlStructureTypeExact(ControlStructureTypes.SWITCH)
        .ast
        .isLiteral
        .code
        .l should contain allOf ("1", "2", "3")
    }

    "not create an unknown node for the match expression" in {
      cpg.all.collectAll[Unknown].codeExact("match x {\n  0 => 1,\n  1 => 2,\n  _ => 3,\n }").l shouldBe empty
    }
  }

  "continue and break inside a loop" should {
    val cpg = code("""
        |fn foo() -> i32 {
        | let x = 0;
        | loop {
        |  if x == 5 {
        |   continue;
        |  }
        |  break 1;
        | }
        |}
        |""".stripMargin)

    "lower continue as a CONTINUE" in {
      cpg.continue.code.l shouldBe List("continue")
    }

    "lower break 1 as a BREAK with the value in code" in {
      cpg.break.code.l shouldBe List("break 1")
    }

    "preserve the break value as an AST child" in {
      inside(cpg.break.astChildren.isLiteral.l) { case value :: Nil =>
        value.code shouldBe "1"
        value.typeFullName shouldBe "i32"
      }
    }
  }

  "a logical not as a condition" should {
    val cpg = code("""
        |fn main(b: bool) {
        | if !b {
        |  foo();
        | }
        |}
        |""".stripMargin)

    "lower to a logicalNot" in {
      inside(cpg.ifBlock.condition.isCall.l) { case condition :: Nil =>
        condition.code shouldBe "!b"
        condition.name shouldBe Operators.logicalNot
        condition.methodFullName shouldBe Operators.logicalNot
        condition.typeFullName shouldBe "bool"
      }
    }

    "have b as the single argument" in {
      inside(cpg.ifBlock.condition.isCall.argument.l) { case (b: Identifier) :: Nil =>
        b.code shouldBe "b"
        b.name shouldBe "b"
        b.typeFullName shouldBe "bool"
      }
    }
  }
}
