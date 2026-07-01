package io.joern.rust2cpg.conformance

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, ModifierTypes, NodeTypes, Operators}
import io.shiftleft.semanticcpg.language.*

class LanguageNeutralConformanceTests extends Rust2CpgSuite(noSysRoot = true) {

  "function definition conformance" should {
    val cpg = code("""
        |fn add(x: i32) -> i32 {
        | x
        |}
        |""".stripMargin)

    "emit a METHOD with fullName, signature, body block, parameter, and METHOD_RETURN" in {
      inside(cpg.method.nameExact("add").l) { case method :: Nil =>
        method.label shouldBe NodeTypes.METHOD
        method.fullName shouldBe "rust2cpgtest::add"
        method.signature should not be null

        inside(method.parameter.l) { case param :: Nil =>
          param.label shouldBe NodeTypes.METHOD_PARAMETER_IN
          param.name shouldBe "x"
          param.typeFullName shouldBe "i32"
        }

        method.block.label shouldBe NodeTypes.BLOCK

        method.methodReturn.label shouldBe NodeTypes.METHOD_RETURN
        method.methodReturn.typeFullName shouldBe "i32"
      }
    }
  }

  "call conformance" should {
    val cpg = code("""
        |fn callee() -> i32 { 1 }
        |
        |fn main() {
        | callee();
        |}
        |""".stripMargin)

    "emit a CALL placed in the enclosing method body" in {
      inside(cpg.method.nameExact("main").block.astChildren.isCall.nameExact("callee").l) { case call :: Nil =>
        call.label shouldBe NodeTypes.CALL
        call.name shouldBe "callee"
        call.methodFullName shouldBe "rust2cpgtest::callee"
        call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }
  }

  "if/else conformance" should {
    val cpg = code("""
        |fn choose(x: i32) {
        | if x > 0 {
        |  positive();
        | } else {
        |  non_positive();
        | }
        |}
        |""".stripMargin)

    "emit an IF control structure with condition, true body, and else body" in {
      inside(cpg.ifBlock.l) { case ifNode :: Nil =>
        ifNode.label shouldBe NodeTypes.CONTROL_STRUCTURE
        ifNode.controlStructureType shouldBe ControlStructureTypes.IF
        ifNode.condition.isCall.name.l shouldBe List(Operators.greaterThan)
        ifNode.whenTrue.isBlock.astChildren.isCall.name.l shouldBe List("positive")
        ifNode.whenFalse.isControlStructure.controlStructureType.l shouldBe List(ControlStructureTypes.ELSE)
        ifNode.whenFalse.isControlStructure.astChildren.isBlock.astChildren.isCall.name.l shouldBe List("non_positive")
      }
    }
  }

  "loop conformance" should {
    val cpg = code("""
        |fn count(mut x: i32) {
        | while x < 3 {
        |  x = x + 1;
        | }
        |}
        |""".stripMargin)

    "emit a loop control structure with condition and body" in {
      inside(cpg.whileBlock.l) { case loop :: Nil =>
        loop.label shouldBe NodeTypes.CONTROL_STRUCTURE
        loop.controlStructureType shouldBe ControlStructureTypes.WHILE
        loop.condition.isCall.name.l shouldBe List(Operators.lessThan)
        loop.astChildren.isBlock.astChildren.isCall.name.l should contain(Operators.assignment)
      }
    }
  }

  "local and assignment conformance" should {
    val cpg = code("""
        |fn main() {
        | let value: i32 = 42;
        |}
        |""".stripMargin)

    "emit a LOCAL and an assignment CALL with identifier and literal arguments" in {
      inside(cpg.method.nameExact("main").block.local.nameExact("value").l) { case local :: Nil =>
        local.label shouldBe NodeTypes.LOCAL
        local.typeFullName shouldBe "i32"
      }

      inside(cpg.method.nameExact("main").block.assignment.l) { case assignment :: Nil =>
        assignment.label shouldBe NodeTypes.CALL
        assignment.name shouldBe Operators.assignment
        assignment.methodFullName shouldBe Operators.assignment
        inside(assignment.argument.l) { case (lhs: Identifier) :: (rhs: Literal) :: Nil =>
          lhs.name shouldBe "value"
          rhs.code shouldBe "42"
        }
      }
    }
  }

  "field access conformance" should {
    val cpg = code("""
        |struct Point { x: i32 }
        |
        |fn read(point: Point) -> i32 {
        | point.x
        |}
        |""".stripMargin)

    "emit a fieldAccess CALL with base and field arguments" in {
      inside(cpg.call.nameExact(Operators.fieldAccess).l) { case fieldAccess :: Nil =>
        fieldAccess.label shouldBe NodeTypes.CALL
        fieldAccess.code shouldBe "point.x"
        fieldAccess.methodFullName shouldBe Operators.fieldAccess
        inside(fieldAccess.argument.l) { case (base: Identifier) :: (field: FieldIdentifier) :: Nil =>
          base.code shouldBe "point"
          field.canonicalName shouldBe "x"
        }
      }
    }
  }

  "closure conformance" should {
    val closureCode = "|x: i32| -> i32 { x + 1 }"
    val cpg = code(s"""
        |fn main() {
        | let inc = $closureCode;
        |}
        |""".stripMargin)

    "emit a lambda METHOD and METHOD_REF" in {
      val lambdaName     = s"${Defines.ClosurePrefix}0"
      val lambdaFullName = s"rust2cpgtest::main::$lambdaName"

      inside(cpg.method.nameExact(lambdaName).l) { case lambda :: Nil =>
        lambda.label shouldBe NodeTypes.METHOD
        lambda.fullName shouldBe lambdaFullName
        lambda.modifier.modifierType.l shouldBe List(ModifierTypes.LAMBDA)
        lambda.parameter.name.l shouldBe List("x")
        lambda.methodReturn.typeFullName shouldBe "i32"
      }

      inside(cpg.methodRef.codeExact(closureCode).l) { case methodRef :: Nil =>
        methodRef.label shouldBe NodeTypes.METHOD_REF
        methodRef.methodFullName shouldBe lambdaFullName
      }
    }
  }

  "type declaration conformance" should {
    val cpg = code("struct Point { x: i32, y: i32 }")

    "emit a TYPE_DECL with fullName and MEMBER children" in {
      inside(cpg.typeDecl.nameExact("Point").l) { case typeDecl :: Nil =>
        typeDecl.label shouldBe NodeTypes.TYPE_DECL
        typeDecl.fullName shouldBe "rust2cpgtest::Point"
        typeDecl.member.name.l shouldBe List("x", "y")
        typeDecl.member.typeFullName.l shouldBe List("i32", "i32")
      }
    }
  }

  "return conformance" should {
    val cpg = code("""
        |fn id(x: i32) -> i32 {
        | return x;
        |}
        |""".stripMargin)

    "emit a RETURN under the method body with the returned expression as child" in {
      inside(cpg.method.nameExact("id").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.label shouldBe NodeTypes.RETURN
        ret.code shouldBe "return x"
        inside(ret.astChildren.isIdentifier.l) { case ident :: Nil =>
          ident.name shouldBe "x"
          ident.typeFullName shouldBe "i32"
        }
      }
    }
  }

  "literal conformance" should {
    val cpg = code("""
        |fn main() {
        | let number = 42;
        | let text = "hi";
        | let flag = true;
        |}
        |""".stripMargin)

    "emit typed LITERAL nodes" in {
      cpg.literal.code.toSet should contain allOf ("42", "\"hi\"", "true")
      inside(cpg.literal.codeExact("42").l) { case number :: Nil =>
        number.label shouldBe NodeTypes.LITERAL
        number.typeFullName shouldBe "i32"
      }
      cpg.literal.codeExact("\"hi\"").typeFullName.l shouldBe List("&str")
      cpg.literal.codeExact("true").typeFullName.l shouldBe List("bool")
    }
  }
}
