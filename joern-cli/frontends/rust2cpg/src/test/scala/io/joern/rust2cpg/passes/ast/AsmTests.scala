package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.DispatchTypes
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class AsmTests extends Rust2CpgSuite(noSysRoot = true) {

  "a compiler builtin asm expression" should {
    val asmCode =
      """builtin # asm("mov {0}, {1}", out(reg) _, in(reg) input, const 7, sym main, options(nomem), clobber_abi("C"))"""

    val cpg = code(s"""
        |fn main(input: i32) {
        | unsafe { $asmCode; }
        |}
        |""".stripMargin)

    "lower to a static asm call" in {
      inside(cpg.call.nameExact("asm!").l) { case asm :: Nil =>
        asm.code shouldBe asmCode
        asm.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::asm!"
        asm.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "preserve template and operand expressions" in {
      inside(cpg.call.nameExact("asm!").argument.l) {
        case (template: Literal) :: (discard: Literal) :: (input: Identifier) :: (constant: Literal) :: (sym: Identifier) :: Nil =>
          template.code shouldBe """"mov {0}, {1}""""
          discard.code shouldBe "_"
          input.name shouldBe "input"
          constant.code shouldBe "7"
          sym.name shouldBe "main"
      }
    }

    "not create unknown nodes for asm or discard operands" in {
      cpg.all.collectAll[Unknown].codeExact(asmCode, "_").l shouldBe empty
    }
  }

  "a compiler builtin global_asm item" should {
    val globalAsmCode = """builtin # global_asm(".globl _start", options(raw))"""
    val cpg           = code(s"$globalAsmCode;")

    "lower to a static global_asm call" in {
      inside(cpg.call.nameExact("global_asm!").l) { case globalAsm :: Nil =>
        globalAsm.code shouldBe globalAsmCode
        globalAsm.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::global_asm!"
        globalAsm.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "preserve the template argument" in {
      inside(cpg.call.nameExact("global_asm!").argument.l) { case (template: Literal) :: Nil =>
        template.code shouldBe """".globl _start""""
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact(globalAsmCode).l shouldBe empty
    }
  }
}
