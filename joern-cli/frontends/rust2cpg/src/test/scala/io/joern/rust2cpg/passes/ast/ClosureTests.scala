package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{ModifierTypes, NodeTypes, Operators}
import io.shiftleft.semanticcpg.language.*

class ClosureTests extends Rust2CpgSuite(noSysRoot = true) {

  "closures" should {
    val typedClosureCode   = "|n: i32| -> i32 { n * 2 }"
    val untypedClosureCode = "|x| x + 1"
    val cpg = code(s"""
        |fn main() {
        | let double = $typedClosureCode;
        | let inc: fn(i32) -> i32 = $untypedClosureCode;
        |}
        |""".stripMargin)

    "lower a closure expression to a lambda method and method ref" in {
      val lambdaName     = s"${Defines.ClosurePrefix}0"
      val lambdaFullName = s"rust2cpgtest::main::$lambdaName"

      inside(cpg.method.nameExact(lambdaName).l) { case lambda :: Nil =>
        lambda.code shouldBe typedClosureCode
        lambda.fullName shouldBe lambdaFullName
        lambda.astParentType shouldBe NodeTypes.METHOD
        lambda.astParentFullName shouldBe "rust2cpgtest::main"
      }

      cpg.method.nameExact(lambdaName).modifier.modifierType.l shouldBe List(ModifierTypes.LAMBDA)

      inside(cpg.methodRef.codeExact(typedClosureCode).l) { case methodRef :: Nil =>
        methodRef.methodFullName shouldBe lambdaFullName
        methodRef.referencedMethod.fullName.shouldBe(lambdaFullName)
      }
    }

    "preserve explicit closure parameter and return types" in {
      val lambdaName = s"${Defines.ClosurePrefix}0"

      inside(cpg.method.nameExact(lambdaName).parameter.l) { case (param: MethodParameterIn) :: Nil =>
        param.name shouldBe "n"
        param.code shouldBe "n: i32"
        param.index shouldBe 1
        param.typeFullName shouldBe "i32"
      }

      cpg.method.nameExact(lambdaName).methodReturn.typeFullName.l shouldBe List("i32")
    }

    "lower closure bodies into method returns" in {
      val typedLambdaName   = s"${Defines.ClosurePrefix}0"
      val untypedLambdaName = s"${Defines.ClosurePrefix}1"

      inside(cpg.method.nameExact(typedLambdaName).block.astChildren.l) { case (ret: Return) :: Nil =>
        ret.code shouldBe "n * 2"

        inside(ret.astChildren.l) { case (multiplication: Call) :: Nil =>
          multiplication.name shouldBe Operators.multiplication
          multiplication.code shouldBe "n * 2"
        }
      }

      inside(cpg.method.nameExact(untypedLambdaName).block.astChildren.l) { case (ret: Return) :: Nil =>
        ret.code shouldBe "x + 1"

        inside(ret.astChildren.l) { case (addition: Call) :: Nil =>
          addition.name shouldBe Operators.addition
          addition.code shouldBe "x + 1"
        }
      }
    }

    "accept untyped closure parameters without creating unknown nodes" in {
      val lambdaName = s"${Defines.ClosurePrefix}1"

      inside(cpg.method.nameExact(lambdaName).parameter.l) { case (param: MethodParameterIn) :: Nil =>
        param.name shouldBe "x"
        param.code shouldBe "x"
        param.index shouldBe 1
      }

      cpg.all.collectAll[Unknown].codeExact(typedClosureCode, untypedClosureCode, "x").l shouldBe empty
    }
  }

  "closure destructuring parameters" should {
    val closureCode = "|(left, right), _| left + right"
    val cpg = code(s"""
        |fn main() {
        | let add_pair = $closureCode;
        |}
        |""".stripMargin)

    "create one lambda parameter per binding" in {
      val lambdaName = s"${Defines.ClosurePrefix}0"

      inside(cpg.method.nameExact(lambdaName).parameter.sortBy(_.index).l) { case left :: right :: ignored :: Nil =>
        left.name shouldBe "left"
        left.index shouldBe 1

        right.name shouldBe "right"
        right.index shouldBe 2

        ignored.name shouldBe "_"
        ignored.index shouldBe 3
      }
    }

    "not create unknown nodes for closure parameter patterns" in {
      cpg.all.collectAll[Unknown].codeExact(closureCode, "(left, right)", "_").l shouldBe empty
    }
  }
}
