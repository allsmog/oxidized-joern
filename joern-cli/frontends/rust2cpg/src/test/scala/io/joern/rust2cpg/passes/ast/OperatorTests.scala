package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.astcreation.RustOperators
import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class OperatorTests extends Rust2CpgSuite(noSysRoot = true) {

  "an `as` expression" should {
    val cpg = code("""
        |fn main(x: i32) {
        | let y = x as i64;
        |}
        |""".stripMargin)

    "lower to a cast call with the target type as typeFullName" in {
      inside(cpg.call.nameExact(Operators.cast).l) { case cast :: Nil =>
        cast.code shouldBe "x as i64"
        cast.methodFullName shouldBe Operators.cast
        cast.typeFullName shouldBe "i64"
      }
    }

    "have a TypeRef to the target type as the first argument" in {
      inside(cpg.call.nameExact(Operators.cast).argument(1).l) { case (typeRef: TypeRef) :: Nil =>
        typeRef.code shouldBe "i64"
        typeRef.typeFullName shouldBe "i64"
      }
    }

    "have the cast operand as the second argument" in {
      inside(cpg.call.nameExact(Operators.cast).argument(2).l) { case (x: Identifier) :: Nil =>
        x.name shouldBe "x"
        x.typeFullName shouldBe "i32"
      }
    }
  }

  "a single index expression" should {
    val cpg = code("""
        |fn foo(xs: Vec<i32>, i: usize) -> i32 {
        | xs[i]
        |}
        |""".stripMargin)

    "lower to an indexAccess call with the element type as typeFullName" in {
      inside(cpg.call.nameExact(Operators.indexAccess).l) { case indexAccess :: Nil =>
        indexAccess.code shouldBe "xs[i]"
        indexAccess.methodFullName shouldBe Operators.indexAccess
        indexAccess.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        indexAccess.typeFullName shouldBe "i32"
      }
    }

    "have the base as the first argument" in {
      inside(cpg.call.nameExact(Operators.indexAccess).argument(1).l) { case (xs: Identifier) :: Nil =>
        xs.name shouldBe "xs"
        xs.typeFullName shouldBe "Vec<i32>"
      }
    }

    "have the index as the second argument" in {
      inside(cpg.call.nameExact(Operators.indexAccess).argument(2).l) { case (i: Identifier) :: Nil =>
        i.name shouldBe "i"
        i.typeFullName shouldBe "usize"
      }
    }
  }

  "a nested index expression" should {
    val cpg = code("""
        |fn foo(xs: Vec<Vec<i32>>, i: usize, j: usize) -> i32 {
        | xs[i][j]
        |}
        |""".stripMargin)

    "lower to two indexAccess calls" in {
      cpg.call.nameExact(Operators.indexAccess).code.toSet shouldBe Set("xs[i]", "xs[i][j]")
    }

    "have an i32 typeFullName for the outer indexAccess" in {
      cpg.call.nameExact(Operators.indexAccess).codeExact("xs[i][j]").typeFullName.l shouldBe List("i32")
    }

    "have the vector element typeFullName for the inner indexAccess" in {
      cpg.call.nameExact(Operators.indexAccess).codeExact("xs[i]").typeFullName.l shouldBe List("Vec<i32>")
    }

    "have the inner indexAccess as the first argument of the outer indexAccess" in {
      inside(cpg.call.nameExact(Operators.indexAccess).codeExact("xs[i][j]").argument(1).l) {
        case (inner: Call) :: Nil =>
          inner.name shouldBe Operators.indexAccess
          inner.code shouldBe "xs[i]"
      }
    }

    "have the index as the second argument of the outer indexAccess" in {
      inside(cpg.call.nameExact(Operators.indexAccess).codeExact("xs[i][j]").argument(2).l) {
        case (j: Identifier) :: Nil =>
          j.name shouldBe "j"
          j.typeFullName shouldBe "usize"
      }
    }
  }

  "unary operators" should {
    val cpg = code("""
        |fn main(x: i32, b: bool, p: *const i32) {
        | let a = -x;
        | let c = !b;
        | let d = *p;
        |}
        |""".stripMargin)

    "lower `-x` to a minus call with i32 typeFullName" in {
      inside(cpg.call.nameExact(Operators.minus).l) { case minus :: Nil =>
        minus.code shouldBe "-x"
        minus.methodFullName shouldBe Operators.minus
        minus.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        minus.typeFullName shouldBe "i32"
      }
    }

    "have the i32 operand as the argument of `-x`" in {
      inside(cpg.call.nameExact(Operators.minus).argument.l) { case (x: Identifier) :: Nil =>
        x.name shouldBe "x"
        x.typeFullName shouldBe "i32"
      }
    }

    "lower `!b` to a logicalNot call with bool typeFullName" in {
      inside(cpg.call.nameExact(Operators.logicalNot).l) { case logicalNot :: Nil =>
        logicalNot.code shouldBe "!b"
        logicalNot.methodFullName shouldBe Operators.logicalNot
        logicalNot.typeFullName shouldBe "bool"
      }
    }

    "have the bool operand as the argument of `!b`" in {
      inside(cpg.call.nameExact(Operators.logicalNot).argument.l) { case (b: Identifier) :: Nil =>
        b.name shouldBe "b"
        b.typeFullName shouldBe "bool"
      }
    }

    "lower `*p` to an indirection call with i32 typeFullName" in {
      inside(cpg.call.nameExact(Operators.indirection).l) { case indirection :: Nil =>
        indirection.code shouldBe "*p"
        indirection.methodFullName shouldBe Operators.indirection
        indirection.typeFullName shouldBe "i32"
      }
    }

    "have the pointer operand as the argument of `*p`" in {
      inside(cpg.call.nameExact(Operators.indirection).argument.l) { case (p: Identifier) :: Nil =>
        p.name shouldBe "p"
        p.typeFullName shouldBe "*const i32"
      }
    }
  }

  "a reference expression" should {
    val cpg = code("""
        |fn main(x: i32) {
        | let y = &x;
        |}
        |""".stripMargin)

    "lower to an addressOf call" in {
      inside(cpg.call.nameExact(Operators.addressOf).l) { case addressOf :: Nil =>
        addressOf.code shouldBe "&x"
        addressOf.methodFullName shouldBe Operators.addressOf
        addressOf.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "have the referenced expression as its argument" in {
      inside(cpg.call.nameExact(Operators.addressOf).argument.l) { case (x: Identifier) :: Nil =>
        x.name shouldBe "x"
        x.typeFullName shouldBe "i32"
      }
    }

    "not create an unknown node for the reference expression" in {
      cpg.all.collectAll[Unknown].codeExact("&x").l shouldBe empty
    }
  }

  "range expressions" should {
    val cpg = code("""
        |fn main() {
        | let inclusive = 1..=5;
        | let half_open = 1..5;
        | let from_start = ..5;
        | let to_end = 1..;
        | let full = ..;
        |}
        |""".stripMargin)

    "lower each range to a range call" in {
      cpg.call.nameExact(Operators.range).code.toSet shouldBe Set("1..=5", "1..5", "..5", "1..", "..")
    }

    "keep both bounds for two-sided ranges" in {
      inside(cpg.call.nameExact(Operators.range).codeExact("1..=5").argument.l) {
        case (one: Literal) :: (five: Literal) :: Nil =>
          one.code shouldBe "1"
          one.argumentIndex shouldBe 1
          five.code shouldBe "5"
          five.argumentIndex shouldBe 2
      }
    }

    "keep the available bound for open-ended ranges" in {
      inside(cpg.call.nameExact(Operators.range).codeExact("..5").argument.l) { case (five: Literal) :: Nil =>
        five.code shouldBe "5"
      }

      inside(cpg.call.nameExact(Operators.range).codeExact("1..").argument.l) { case (one: Literal) :: Nil =>
        one.code shouldBe "1"
      }

      cpg.call.nameExact(Operators.range).codeExact("..").argument.l shouldBe empty
    }

    "not create unknown nodes for range expressions" in {
      cpg.all.collectAll[Unknown].codeExact("1..=5", "1..5", "..5", "1..", "..").l shouldBe empty
    }
  }

  "an await expression" should {
    val cpg = code("""
        |async fn fetch() -> i32 {
        | 1
        |}
        |
        |async fn main() {
        | let x = fetch().await;
        |}
        |""".stripMargin)

    "lower to an await operator call" in {
      inside(cpg.call.nameExact(RustOperators.await).l) { case awaitCall :: Nil =>
        awaitCall.code shouldBe "fetch().await"
        awaitCall.methodFullName shouldBe RustOperators.await
        awaitCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "wrap the awaited expression as its argument" in {
      inside(cpg.call.nameExact(RustOperators.await).argument.l) { case (fetch: Call) :: Nil =>
        fetch.name shouldBe "fetch"
        fetch.code shouldBe "fetch()"
      }
    }

    "not create an unknown node for the await expression" in {
      cpg.all.collectAll[Unknown].codeExact("fetch().await").l shouldBe empty
    }
  }

  "a try-propagation expression" should {
    val cpg = code("""
        |fn parse_one(text: &str) -> Result<i32, ()> {
        | Ok(1)
        |}
        |
        |fn run(text: &str) -> Result<i32, ()> {
        | let value = parse_one(text)?;
        | Ok(value)
        |}
        |""".stripMargin)

    "lower to a try-propagation operator call" in {
      inside(cpg.call.nameExact(RustOperators.tryPropagate).l) { case tryCall :: Nil =>
        tryCall.code shouldBe "parse_one(text)?"
        tryCall.methodFullName shouldBe RustOperators.tryPropagate
        tryCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "wrap the fallible expression as its argument" in {
      inside(cpg.call.nameExact(RustOperators.tryPropagate).argument.l) { case (parseOne: Call) :: Nil =>
        parseOne.name shouldBe "parse_one"
        parseOne.code shouldBe "parse_one(text)"
      }
    }

    "not create an unknown node for the try-propagation expression" in {
      cpg.all.collectAll[Unknown].codeExact("parse_one(text)?").l shouldBe empty
    }
  }

  "yield and yeet expressions" should {
    val cpg = code("""
        |#![feature(generators, yeet_expr)]
        |fn main() {
        | || { yield 1; do yeet 2; };
        |}
        |""".stripMargin)

    "lower yield to an operator call" in {
      inside(cpg.call.nameExact(RustOperators.yieldValue).l) { case yieldCall :: Nil =>
        yieldCall.code shouldBe "yield 1"
        yieldCall.methodFullName shouldBe RustOperators.yieldValue
        yieldCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "wrap the yielded expression as its argument" in {
      inside(cpg.call.nameExact(RustOperators.yieldValue).argument.l) { case (value: Literal) :: Nil =>
        value.code shouldBe "1"
      }
    }

    "lower yeet to an operator call" in {
      inside(cpg.call.nameExact(RustOperators.yeet).l) { case yeetCall :: Nil =>
        yeetCall.code shouldBe "do yeet 2"
        yeetCall.methodFullName shouldBe RustOperators.yeet
        yeetCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "wrap the yeeted expression as its argument" in {
      inside(cpg.call.nameExact(RustOperators.yeet).argument.l) { case (value: Literal) :: Nil =>
        value.code shouldBe "2"
      }
    }

    "not create unknown nodes for yield or yeet" in {
      cpg.all.collectAll[Unknown].codeExact("yield 1", "do yeet 2").l shouldBe empty
    }
  }

  "a become expression" should {
    val cpg = code("""
        |#![feature(explicit_tail_calls)]
        |
        |fn target(x: i32) -> i32 {
        | x
        |}
        |
        |fn main(x: i32) -> i32 {
        | become target(x)
        |}
        |""".stripMargin)

    "lower to a become operator call" in {
      inside(cpg.call.nameExact(RustOperators.become).l) { case becomeCall :: Nil =>
        becomeCall.code shouldBe "become target(x)"
        becomeCall.methodFullName shouldBe RustOperators.become
        becomeCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "wrap the target expression as its argument" in {
      inside(cpg.call.nameExact(RustOperators.become).argument.l) { case (target: Call) :: Nil =>
        target.name shouldBe "target"
        target.code shouldBe "target(x)"
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("become target(x)").l shouldBe empty
    }
  }

  "nested arithmetic and comparison operators" should {
    val cpg = code("""
        |fn main(x: i32, y: i32) -> bool {
        | (x + y) > 0
        |}
        |""".stripMargin)

    "have an i32 typeFullName for the addition" in {
      cpg.call.nameExact(Operators.addition).typeFullName.l shouldBe List("i32")
    }

    "have a bool typeFullName for the comparison" in {
      cpg.call.nameExact(Operators.greaterThan).typeFullName.l shouldBe List("bool")
    }
  }
}
