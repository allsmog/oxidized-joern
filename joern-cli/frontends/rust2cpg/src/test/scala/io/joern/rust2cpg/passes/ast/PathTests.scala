package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class PathTests extends Rust2CpgSuite(noSysRoot = true) {

  "qualified path expressions" should {
    val cpg = code("""
        |mod config {
        | pub const VALUE: i32 = 1;
        |}
        |
        |fn main() {
        | let local = config::VALUE;
        | let absolute = crate::config::VALUE;
        |}
        |""".stripMargin)

    "lower a two-part path to field access" in {
      inside(cpg.call.nameExact(Operators.fieldAccess).codeExact("config::VALUE").argument.l) {
        case (base: Identifier) :: (field: FieldIdentifier) :: Nil =>
          base.code shouldBe "config"
          field.code shouldBe "VALUE"
      }
    }

    "lower a nested path to nested field access" in {
      inside(cpg.call.nameExact(Operators.fieldAccess).codeExact("crate::config::VALUE").argument.l) {
        case (base: Call) :: (field: FieldIdentifier) :: Nil =>
          base.code shouldBe "crate::config"
          base.name shouldBe Operators.fieldAccess
          field.code shouldBe "VALUE"
      }
    }

    "not create unknown nodes for qualified paths" in {
      cpg.all.collectAll[Unknown].codeExact("config::VALUE", "crate::config::VALUE").l shouldBe empty
    }
  }
}
