package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.astcreation.RustOperators
import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class MacroTests extends Rust2CpgSuite(noSysRoot = true) {

  "a macro_rules declaration" should {
    val cpg = code("""
        |macro_rules! hello { () => { 1 }; }
        |
        |fn main() {
        | hello!();
        |}
        |""".stripMargin)

    "lower to a macro method declaration" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::hello!").l) { case hello :: Nil =>
        hello.fullName shouldBe "rust2cpgtest::hello!"
        hello.code shouldBe "macro_rules! hello { () => { 1 }; }"
        hello.methodReturn.typeFullName shouldBe "i32"
      }
    }

    "preserve the simple expansion body" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::hello!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "1"

        inside(ret.astChildren.l) { case (one: Literal) :: Nil =>
          one.code shouldBe "1"
          one.typeFullName shouldBe "i32"
        }
      }
    }

    "still lower calls to the macro" in {
      inside(cpg.call.nameExact("hello!").l) { case hello :: Nil =>
        hello.code shouldBe "hello!()"
      }
    }

    "attach the expansion at the call site" in {
      inside(cpg.call.nameExact("hello!").l) { case hello :: Nil =>
        hello.typeFullName shouldBe "i32"
        inside(hello.argument.l) { case (one: Literal) :: Nil =>
          one.code shouldBe "1"
          one.typeFullName shouldBe "i32"
        }
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("macro_rules! hello { () => { 1 }; }").l shouldBe empty
    }
  }

  "a declarative macro definition" should {
    val cpg = code("""
        |#![feature(decl_macro)]
        |
        |macro make_one() { 1 }
        |
        |fn main() {
        | make_one!();
        |}
        |""".stripMargin)

    "lower to a macro method declaration" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_one!").l) { case makeOne :: Nil =>
        makeOne.fullName shouldBe "rust2cpgtest::make_one!"
        makeOne.code shouldBe "macro make_one() { 1 }"
        makeOne.methodReturn.typeFullName shouldBe "i32"
      }
    }

    "preserve the simple expansion body" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_one!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "1"

        inside(ret.astChildren.l) { case (one: Literal) :: Nil =>
          one.code shouldBe "1"
          one.typeFullName shouldBe "i32"
        }
      }
    }

    "still lower calls to the macro" in {
      inside(cpg.call.nameExact("make_one!").l) { case makeOne :: Nil =>
        makeOne.code shouldBe "make_one!()"
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("macro make_one() { 1 }").l shouldBe empty
    }
  }

  "tuple macro expansion bodies" should {
    val cpg = code("""
        |#![feature(decl_macro)]
        |
        |macro make_pair() { (1, 2) }
        |macro_rules! rules_pair { () => { (1, 2) }; }
        |
        |fn main() {
        | make_pair!();
        | rules_pair!();
        |}
        |""".stripMargin)

    "type declarative macro tuple expansions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_pair!").l) { case makePair :: Nil =>
        makePair.methodReturn.typeFullName shouldBe "(i32, i32)"
      }
    }

    "preserve declarative macro tuple bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_pair!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "(1, 2)"

        inside(ret.astChildren.isCall.l) { case tuple :: Nil =>
          tuple.name shouldBe RustOperators.tupleLiteral
          tuple.typeFullName shouldBe "(i32, i32)"
          tuple.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }

    "type macro_rules tuple expansions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::rules_pair!").l) { case rulesPair :: Nil =>
        rulesPair.methodReturn.typeFullName shouldBe "(i32, i32)"
      }
    }

    "preserve macro_rules tuple bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::rules_pair!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "(1, 2)"

        inside(ret.astChildren.isCall.l) { case tuple :: Nil =>
          tuple.name shouldBe RustOperators.tupleLiteral
          tuple.typeFullName shouldBe "(i32, i32)"
          tuple.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }
  }

  "array macro expansion bodies" should {
    val cpg = code("""
        |#![feature(decl_macro)]
        |
        |macro make_array() { [1, 2] }
        |macro make_repeat() { [0; 5] }
        |macro_rules! rules_array { () => { [1, 2] }; }
        |
        |fn main() {
        | make_array!();
        | make_repeat!();
        | rules_array!();
        |}
        |""".stripMargin)

    "type declarative macro array expansions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_array!").l) { case makeArray :: Nil =>
        makeArray.methodReturn.typeFullName shouldBe "[i32; 2]"
      }
    }

    "preserve declarative macro array bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_array!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[1, 2]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe Operators.arrayInitializer
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }

    "type and preserve repeat array expansions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_repeat!").l) { case makeRepeat :: Nil =>
        makeRepeat.methodReturn.typeFullName shouldBe "[i32; 5]"
      }

      inside(cpg.method.fullNameExact("rust2cpgtest::make_repeat!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[0; 5]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe RustOperators.repeatInArray
          array.typeFullName shouldBe "[i32; 5]"
          array.argument.isLiteral.code.l shouldBe List("0", "5")
          array.argument.isLiteral.typeFullName.l shouldBe List("i32", "usize")
        }
      }
    }

    "type macro_rules array expansions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::rules_array!").l) { case rulesArray :: Nil =>
        rulesArray.methodReturn.typeFullName shouldBe "[i32; 2]"
      }
    }

    "preserve macro_rules array bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::rules_array!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[1, 2]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe Operators.arrayInitializer
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }
  }

  "expression macro expansion bodies" should {
    val cpg = code("""
        |#![feature(decl_macro)]
        |
        |fn helper(left: i32, right: i32) -> i32 { left + right }
        |
        |macro make_sum() { 1 + 2 * 3 }
        |macro make_not() { !false }
        |macro make_call() { helper(1, 2) }
        |macro_rules! rules_and { () => { true && false }; }
        |
        |fn main() {
        | make_sum!();
        | make_not!();
        | make_call!();
        | rules_and!();
        |}
        |""".stripMargin)

    "type arithmetic and boolean expression bodies" in {
      cpg.method.fullNameExact("rust2cpgtest::make_sum!").methodReturn.typeFullName.l shouldBe List("i32")
      cpg.method.fullNameExact("rust2cpgtest::make_not!").methodReturn.typeFullName.l shouldBe List("bool")
      cpg.method.fullNameExact("rust2cpgtest::rules_and!").methodReturn.typeFullName.l shouldBe List("bool")
    }

    "preserve binary expression bodies with precedence" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_sum!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "1 + 2 * 3"

        inside(ret.astChildren.isCall.l) { case add :: Nil =>
          add.name shouldBe Operators.addition
          add.typeFullName shouldBe "i32"

          inside(add.argument.l) { case (one: Literal) :: (multiply: Call) :: Nil =>
            one.code shouldBe "1"
            multiply.name shouldBe Operators.multiplication
            multiply.argument.isLiteral.code.l shouldBe List("2", "3")
          }
        }
      }
    }

    "preserve prefix expression bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_not!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "!false"

        inside(ret.astChildren.isCall.l) { case not :: Nil =>
          not.name shouldBe Operators.logicalNot
          not.typeFullName shouldBe "bool"
          not.argument.isLiteral.code.l shouldBe List("false")
        }
      }
    }

    "preserve simple call expression bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::make_call!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "helper(1, 2)"

        inside(ret.astChildren.isCall.nameExact("helper").l) { case helper :: Nil =>
          helper.code shouldBe "helper(1, 2)"
          helper.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }

    "preserve macro_rules expression bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::rules_and!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "true && false"

        inside(ret.astChildren.isCall.l) { case and :: Nil =>
          and.name shouldBe Operators.logicalAnd
          and.typeFullName shouldBe "bool"
          and.argument.isLiteral.code.l shouldBe List("true", "false")
        }
      }
    }
  }

  "macro_rules block expansion bodies" should {
    val cpg = code("""
        |macro_rules! twice { ($x:expr) => {{ sink($x); sink($x); }}; }
        |macro_rules! hold { ($x:expr) => {{ let tmp = $x; tmp }}; }
        |macro_rules! typed_hold { ($x:expr) => {{ let tmp: i32 = $x; tmp }}; }
        |macro_rules! typed_as { ($x:expr, $t:ty) => {{ let tmp: $t = $x; tmp }}; }
        |macro_rules! bail { ($x:expr) => {{ return $x; }}; }
        |macro_rules! ensure { ($cond:expr, $ret:expr) => {{ if !$cond { return $ret; } }}; }
        |macro_rules! choose_if { ($cond:expr, $a:expr, $b:expr) => {{ if $cond { $a } else { $b } }}; }
        |
        |fn sink(_: i32) {}
        |fn early() -> i32 {
        | bail!(1);
        | 0
        |}
        |fn guarded(flag: bool) -> i32 {
        | ensure!(flag, 2);
        | 1
        |}
        |
        |fn main() {
        | twice!(1);
        | hold!(1);
        | typed_hold!(1);
        | typed_as!(1, i32);
        | choose_if!(true, 1, 2);
        |}
        |""".stripMargin)

    "preserve statement blocks in macro definitions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::twice!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "{ sink($x); sink($x); }"

        inside(ret.astChildren.isBlock.l) { case block :: Nil =>
          inside(block.astChildren.isCall.nameExact("sink").l) { case firstSink :: secondSink :: Nil =>
            firstSink.code shouldBe "sink($x)"
            firstSink.argument.isIdentifier.code.l shouldBe List("$x")

            secondSink.code shouldBe "sink($x)"
            secondSink.argument.isIdentifier.code.l shouldBe List("$x")
          }
        }
      }
    }

    "preserve local bindings in macro definition blocks" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::hold!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "{ let tmp = $x; tmp }"

        inside(ret.astChildren.isBlock.l) { case block :: Nil =>
          inside(block.astChildren.l) { case (tmpLocal: Local) :: (assignment: Call) :: (tmp: Identifier) :: Nil =>
            tmpLocal.name shouldBe "tmp"
            tmpLocal.typeFullName shouldBe "ANY"

            assignment.name shouldBe Operators.assignment
            assignment.code shouldBe "let tmp = $x"

            inside(assignment.argument.l) { case (assignedTmp: Identifier) :: (x: Identifier) :: Nil =>
              assignedTmp.name shouldBe "tmp"
              assignedTmp.typeFullName shouldBe "ANY"
              x.name shouldBe "x"
              x.code shouldBe "$x"
              x.typeFullName shouldBe "ANY"
            }

            tmp.name shouldBe "tmp"
            tmp.typeFullName shouldBe "ANY"
          }
        }
      }
    }

    "preserve typed local bindings in macro definition blocks" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::typed_hold!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "{ let tmp: i32 = $x; tmp }"

        inside(ret.astChildren.isBlock.l) { case block :: Nil =>
          inside(block.astChildren.l) { case (tmpLocal: Local) :: (assignment: Call) :: (tmp: Identifier) :: Nil =>
            tmpLocal.name shouldBe "tmp"
            tmpLocal.typeFullName shouldBe "i32"

            assignment.name shouldBe Operators.assignment
            assignment.code shouldBe "let tmp: i32 = $x"

            inside(assignment.argument.l) { case (assignedTmp: Identifier) :: (x: Identifier) :: Nil =>
              assignedTmp.name shouldBe "tmp"
              assignedTmp.typeFullName shouldBe "i32"
              x.name shouldBe "x"
              x.code shouldBe "$x"
              x.typeFullName shouldBe "ANY"
            }

            tmp.name shouldBe "tmp"
            tmp.typeFullName shouldBe "i32"
          }
        }
      }
    }

    "preserve return statements in macro definition blocks" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::bail!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "{ return $x; }"

        inside(ret.astChildren.isBlock.l) { case block :: Nil =>
          inside(block.astChildren.isReturn.l) { case returned :: Nil =>
            returned.code shouldBe "return $x"

            inside(returned.astChildren.l) { case (x: Identifier) :: Nil =>
              x.name shouldBe "x"
              x.code shouldBe "$x"
              x.typeFullName shouldBe "ANY"
            }
          }
        }
      }
    }

    "preserve if-return guards in macro definition blocks" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::ensure!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "{ if !$cond { return $ret; } }"

        inside(ret.astChildren.isBlock.l) { case block :: Nil =>
          inside(block.astChildren.isControlStructure.controlStructureTypeExact(ControlStructureTypes.IF).l) {
            case ifNode :: Nil =>
              ifNode.code shouldBe "if !$cond { return $ret; }"

              inside(ifNode.condition.isCall.l) { case not :: Nil =>
                not.name shouldBe Operators.logicalNot
                not.argument.isIdentifier.code.l shouldBe List("$cond")
              }

              inside(ifNode.astChildren.isBlock.astChildren.isReturn.l) { case returned :: Nil =>
                returned.code shouldBe "return $ret"
                returned.astChildren.isIdentifier.code.l shouldBe List("$ret")
              }
          }
        }
      }
    }

    "preserve if-else expression bodies in macro definitions" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::choose_if!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "if $cond { $a } else { $b }"

        inside(ret.astChildren.isControlStructure.controlStructureTypeExact(ControlStructureTypes.IF).l) {
          case ifNode :: Nil =>
            ifNode.code shouldBe "if $cond { $a } else { $b }"
            ifNode.condition.isIdentifier.code.l shouldBe List("$cond")
            ifNode.astChildren.isBlock.astChildren.isIdentifier.code.l shouldBe List("$a")

            ifNode
              .astChildren
              .isControlStructure
              .controlStructureTypeExact(ControlStructureTypes.ELSE)
              .astChildren
              .isBlock
              .astChildren
              .isIdentifier
              .code
              .l shouldBe List("$b")
        }
      }
    }

    "substitute statement blocks at call sites" in {
      inside(cpg.call.nameExact("twice!").l) { case twice :: Nil =>
        twice.typeFullName shouldBe "()"

        inside(twice.argument.l) { case (rawArg: Literal) :: (block: Block) :: Nil =>
          rawArg.code shouldBe "1"

          inside(block.astChildren.isCall.nameExact("sink").l) { case firstSink :: secondSink :: Nil =>
            firstSink.argument.isLiteral.code.l shouldBe List("1")
            secondSink.argument.isLiteral.code.l shouldBe List("1")
          }
        }
      }
    }

    "substitute local binding blocks at call sites" in {
      inside(cpg.call.nameExact("hold!").l) { case hold :: Nil =>
        hold.typeFullName shouldBe "i32"

        inside(hold.argument.l) { case (rawArg: Literal) :: (block: Block) :: Nil =>
          rawArg.code shouldBe "1"

          inside(block.astChildren.l) { case (tmpLocal: Local) :: (assignment: Call) :: (tmp: Identifier) :: Nil =>
            tmpLocal.name shouldBe "tmp"
            tmpLocal.typeFullName shouldBe "i32"

            assignment.name shouldBe Operators.assignment
            assignment.code shouldBe "let tmp = $x"

            inside(assignment.argument.l) { case (assignedTmp: Identifier) :: (one: Literal) :: Nil =>
              assignedTmp.name shouldBe "tmp"
              assignedTmp.typeFullName shouldBe "i32"
              one.code shouldBe "1"
              one.typeFullName shouldBe "i32"
            }

            tmp.name shouldBe "tmp"
            tmp.typeFullName shouldBe "i32"
          }
        }
      }
    }

    "substitute typed local binding blocks at call sites" in {
      inside(cpg.call.nameExact("typed_hold!").l) { case typedHold :: Nil =>
        typedHold.typeFullName shouldBe "i32"

        inside(typedHold.argument.l) { case (_: Literal) :: (block: Block) :: Nil =>
          inside(block.astChildren.l) { case (tmpLocal: Local) :: (assignment: Call) :: (tmp: Identifier) :: Nil =>
            tmpLocal.name shouldBe "tmp"
            tmpLocal.typeFullName shouldBe "i32"
            assignment.name shouldBe Operators.assignment
            assignment.code shouldBe "let tmp: i32 = $x"
            assignment.argument.isIdentifier.typeFullName.l shouldBe List("i32")
            assignment.argument.isLiteral.typeFullName.l shouldBe List("i32")
            tmp.name shouldBe "tmp"
            tmp.typeFullName shouldBe "i32"
          }
        }
      }
    }

    "substitute type metavariables in typed local binding blocks at call sites" in {
      inside(cpg.call.nameExact("typed_as!").l) { case typedAs :: Nil =>
        typedAs.typeFullName shouldBe "i32"

        inside(typedAs.argument.l) { case (_: Literal) :: (typeArg: Identifier) :: (block: Block) :: Nil =>
          typeArg.code shouldBe "i32"

          inside(block.astChildren.l) { case (tmpLocal: Local) :: (assignment: Call) :: (tmp: Identifier) :: Nil =>
            tmpLocal.name shouldBe "tmp"
            tmpLocal.typeFullName shouldBe "i32"
            assignment.name shouldBe Operators.assignment
            assignment.code shouldBe "let tmp: $t = $x"
            assignment.argument.isIdentifier.typeFullName.l shouldBe List("i32")
            assignment.argument.isLiteral.typeFullName.l shouldBe List("i32")
            tmp.name shouldBe "tmp"
            tmp.typeFullName shouldBe "i32"
          }
        }
      }
    }

    "substitute return statement blocks at call sites" in {
      inside(cpg.call.nameExact("bail!").l) { case bail :: Nil =>
        bail.typeFullName shouldBe "!"

        inside(bail.argument.l) { case (rawArg: Literal) :: (block: Block) :: Nil =>
          rawArg.code shouldBe "1"

          inside(block.astChildren.isReturn.l) { case returned :: Nil =>
            returned.code shouldBe "return $x"

            inside(returned.astChildren.l) { case (one: Literal) :: Nil =>
              one.code shouldBe "1"
              one.typeFullName shouldBe "i32"
            }
          }
        }
      }
    }

    "substitute if-return guard blocks at call sites" in {
      inside(cpg.call.nameExact("ensure!").l) { case ensure :: Nil =>
        ensure.typeFullName shouldBe "()"

        inside(ensure.argument.l) { case (flag: Identifier) :: (rawRet: Literal) :: (block: Block) :: Nil =>
          flag.code shouldBe "flag"
          flag.typeFullName shouldBe "bool"
          rawRet.code shouldBe "2"

          inside(block.astChildren.isControlStructure.controlStructureTypeExact(ControlStructureTypes.IF).l) {
            case ifNode :: Nil =>
              ifNode.code shouldBe "if !$cond { return $ret; }"

              inside(ifNode.condition.isCall.l) { case not :: Nil =>
                not.name shouldBe Operators.logicalNot
                not.argument.isIdentifier.code.l shouldBe List("flag")
                not.argument.isIdentifier.typeFullName.l shouldBe List("bool")
              }

              inside(ifNode.astChildren.isBlock.astChildren.isReturn.l) { case returned :: Nil =>
                returned.code shouldBe "return $ret"

                inside(returned.astChildren.l) { case (two: Literal) :: Nil =>
                  two.code shouldBe "2"
                  two.typeFullName shouldBe "i32"
                }
              }
          }
        }
      }
    }

    "substitute if-else expression bodies at call sites" in {
      inside(cpg.call.nameExact("choose_if!").l) { case chooseIf :: Nil =>
        chooseIf.typeFullName shouldBe "i32"

        inside(chooseIf.argument.l) {
          case (conditionArg: Literal) :: (thenArg: Literal) :: (elseArg: Literal) :: (ifNode: ControlStructure) :: Nil =>
            conditionArg.code shouldBe "true"
            conditionArg.typeFullName shouldBe "bool"
            thenArg.code shouldBe "1"
            elseArg.code shouldBe "2"

            ifNode.code shouldBe "if $cond { $a } else { $b }"
            ifNode.condition.isLiteral.code.l shouldBe List("true")
            ifNode.astChildren.isBlock.astChildren.isLiteral.code.l shouldBe List("1")

            ifNode
              .astChildren
              .isControlStructure
              .controlStructureTypeExact(ControlStructureTypes.ELSE)
              .astChildren
              .isBlock
              .astChildren
              .isLiteral
              .code
              .l shouldBe List("2")
        }
      }
    }

    "not create unknown nodes for block macro definitions" in {
      cpg.all.collectAll[Unknown].codeExact(
        "macro_rules! twice { ($x:expr) => {{ sink($x); sink($x); }}; }",
        "macro_rules! hold { ($x:expr) => {{ let tmp = $x; tmp }}; }",
        "macro_rules! typed_hold { ($x:expr) => {{ let tmp: i32 = $x; tmp }}; }",
        "macro_rules! typed_as { ($x:expr, $t:ty) => {{ let tmp: $t = $x; tmp }}; }",
        "macro_rules! bail { ($x:expr) => {{ return $x; }}; }",
        "macro_rules! ensure { ($cond:expr, $ret:expr) => {{ if !$cond { return $ret; } }}; }",
        "macro_rules! choose_if { ($cond:expr, $a:expr, $b:expr) => {{ if $cond { $a } else { $b } }}; }"
      ).l shouldBe empty
    }
  }

  "macro_rules metavariables and multiple arms" should {
    val cpg = code("""
        |macro_rules! add_one { ($x:expr) => { $x + 1 }; }
        |macro_rules! choose { () => { 0 }; ($x:expr) => { $x }; }
        |macro_rules! flags { () => { true }; ($x:expr) => { false }; }
        |macro_rules! call_helper { () => { $crate::helper(1) }; }
        |macro_rules! trailing { ($x:expr $(,)?) => { [$x] }; }
        |
        |fn helper(x: i32) -> i32 { x }
        |
        |fn main() {
        | add_one!(41);
        | choose!();
        | choose!(7);
        | flags!();
        | flags!(1);
        | call_helper!();
        | trailing!(1);
        | trailing!(2,);
        |}
        |""".stripMargin)

    "preserve metavariable references in expression bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::add_one!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "$x + 1"

        inside(ret.astChildren.isCall.l) { case add :: Nil =>
          add.name shouldBe Operators.addition
          add.typeFullName shouldBe "ANY"

          inside(add.argument.l) { case (x: Identifier) :: (one: Literal) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            x.typeFullName shouldBe "ANY"

            one.code shouldBe "1"
            one.typeFullName shouldBe "i32"
          }
        }
      }
    }

    "preserve multiple simple macro_rules arms" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::choose!").l) { case choose :: Nil =>
        choose.methodReturn.typeFullName shouldBe "ANY"
        choose.block.astChildren.isReturn.code.l shouldBe List("0", "$x")
      }
    }

    "keep precise return types when all simple arms agree" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::flags!").l) { case flags :: Nil =>
        flags.methodReturn.typeFullName shouldBe "bool"
        flags.block.astChildren.isReturn.code.l shouldBe List("true", "false")
      }
    }

    "preserve $crate call paths in macro bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::call_helper!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "$crate::helper(1)"

        inside(ret.astChildren.isCall.nameExact("helper").l) { case helper :: Nil =>
          helper.code shouldBe "$crate::helper(1)"
          helper.methodFullName shouldBe "crate::helper"
          helper.argument.isLiteral.code.l shouldBe List("1")
        }
      }
    }

    "substitute simple macro expansions at call sites" in {
      inside(cpg.call.nameExact("add_one!").l) { case addOne :: Nil =>
        addOne.typeFullName shouldBe "i32"

        inside(addOne.argument.l) { case (rawArg: Literal) :: (add: Call) :: Nil =>
          rawArg.code shouldBe "41"

          add.name shouldBe Operators.addition
          add.typeFullName shouldBe "i32"
          add.argument.isLiteral.code.l shouldBe List("41", "1")
        }
      }
    }

    "match optional literal fragments in macro_rules patterns at call sites" in {
      inside(cpg.call.nameExact("trailing!").filter(_.code == "trailing!(1)").l) { case trailing :: Nil =>
        inside(trailing.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 1]"
          array.argument.isLiteral.code.l shouldBe List("1")
        }
      }

      inside(cpg.call.nameExact("trailing!").filter(_.code == "trailing!(2,)").l) { case trailing :: Nil =>
        inside(trailing.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 1]"
          array.argument.isLiteral.code.l shouldBe List("2")
        }
      }
    }

    "select the matching macro_rules arm at call sites" in {
      inside(cpg.call.nameExact("choose!").filter(_.code == "choose!()").l) { case chooseNoArg :: Nil =>
        chooseNoArg.argument.isLiteral.code.l shouldBe List("0")
      }

      inside(cpg.call.nameExact("choose!").filter(_.code == "choose!(7)").l) { case chooseOneArg :: Nil =>
        chooseOneArg.argument.isLiteral.code.l shouldBe List("7", "7")
      }
    }

    "not create unknown nodes for metavariable macro definitions" in {
      cpg.all.collectAll[Unknown].codeExact(
        "macro_rules! add_one { ($x:expr) => { $x + 1 }; }",
        "macro_rules! choose { () => { 0 }; ($x:expr) => { $x }; }",
        "macro_rules! flags { () => { true }; ($x:expr) => { false }; }",
        "macro_rules! call_helper { () => { $crate::helper(1) }; }",
        "macro_rules! trailing { ($x:expr $(,)?) => { [$x] }; }"
      ).l shouldBe empty
    }
  }

  "macro_rules repetition bodies" should {
    val cpg = code("""
        |macro_rules! list { ($($x:expr),*) => { [$($x),*] }; }
        |macro_rules! tupled { ($($x:expr),*) => { ($($x),*) }; }
        |macro_rules! calls { ($($x:expr),*) => { sink($($x),*) }; }
        |macro_rules! pair_list { ($($x:expr => $y:expr),*) => { [$($x, $y),*] }; }
        |macro_rules! pair_calls { ($($x:expr => $y:expr),*) => { sink4($($x, $y),*) }; }
        |macro_rules! sum_list { ($($x:expr),*) => { [$($x + 1),*] }; }
        |macro_rules! empty_list { ($($x:expr),*) => { [$($x),*] }; }
        |macro_rules! empty_calls { ($($x:expr),*) => { sink0($($x),*) }; }
        |macro_rules! plus_list { ($($x:expr),+ $(,)?) => { [$($x),*] }; }
        |macro_rules! semi_list { ($($x:expr);*) => { [$($x),*] }; }
        |macro_rules! semi_plus_list { ($($x:expr);+) => { [$($x),*] }; }
        |
        |fn sink(_: i32, _: i32) {}
        |fn sink0() {}
        |fn sink4(_: i32, _: i32, _: i32, _: i32) {}
        |
        |fn main() {
        | list!(1, 2);
        | tupled!(1, 2);
        | calls!(1, 2);
        | pair_list!(1 => 2, 3 => 4);
        | pair_calls!(1 => 2, 3 => 4);
        | sum_list!(1, 2);
        | empty_list!();
        | empty_calls!();
        | plus_list!(1, 2);
        | plus_list!(1, 2,);
        | semi_list!(1; 2);
        | semi_plus_list!(1; 2);
        |}
        |""".stripMargin)

    "preserve repeated metavariables inside array bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::list!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[$($x),*]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe Operators.arrayInitializer
          array.code shouldBe "[$($x),*]"
          array.typeFullName shouldBe "ANY"

          inside(array.argument.l) { case (x: Identifier) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            x.typeFullName shouldBe "ANY"
          }
        }
      }
    }

    "preserve repeated metavariables inside tuple bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::tupled!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "($($x),*)"

        inside(ret.astChildren.isCall.l) { case tuple :: Nil =>
          tuple.name shouldBe RustOperators.tupleLiteral
          tuple.code shouldBe "($($x),*)"
          tuple.typeFullName shouldBe "ANY"

          inside(tuple.argument.l) { case (x: Identifier) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            x.typeFullName shouldBe "ANY"
          }
        }
      }
    }

    "preserve repeated metavariables inside call bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::calls!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "sink($($x),*)"

        inside(ret.astChildren.isCall.nameExact("sink").l) { case sink :: Nil =>
          sink.code shouldBe "sink($($x),*)"

          inside(sink.argument.l) { case (x: Identifier) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            x.typeFullName shouldBe "ANY"
          }
        }
      }
    }

    "preserve repeated fragments with multiple expressions inside array bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::pair_list!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[$($x, $y),*]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe Operators.arrayInitializer
          array.code shouldBe "[$($x, $y),*]"
          array.typeFullName shouldBe "ANY"

          inside(array.argument.l) { case (x: Identifier) :: (y: Identifier) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            x.typeFullName shouldBe "ANY"

            y.name shouldBe "y"
            y.code shouldBe "$y"
            y.typeFullName shouldBe "ANY"
          }
        }
      }
    }

    "preserve repeated fragments with multiple expressions inside call bodies" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::pair_calls!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "sink4($($x, $y),*)"

        inside(ret.astChildren.isCall.nameExact("sink4").l) { case sink :: Nil =>
          sink.code shouldBe "sink4($($x, $y),*)"

          inside(sink.argument.l) { case (x: Identifier) :: (y: Identifier) :: Nil =>
            x.name shouldBe "x"
            x.code shouldBe "$x"
            y.name shouldBe "y"
            y.code shouldBe "$y"
          }
        }
      }
    }

    "preserve repeated expression fragments" in {
      inside(cpg.method.fullNameExact("rust2cpgtest::sum_list!").block.astChildren.isReturn.l) { case ret :: Nil =>
        ret.code shouldBe "[$($x + 1),*]"

        inside(ret.astChildren.isCall.l) { case array :: Nil =>
          array.name shouldBe Operators.arrayInitializer
          array.typeFullName shouldBe "ANY"

          inside(array.argument.l) { case (add: Call) :: Nil =>
            add.name shouldBe Operators.addition
            add.code shouldBe "$x + 1"

            inside(add.argument.l) { case (x: Identifier) :: (one: Literal) :: Nil =>
              x.name shouldBe "x"
              x.code shouldBe "$x"
              one.code shouldBe "1"
            }
          }
        }
      }
    }

    "substitute repeated macro expansions at call sites" in {
      inside(cpg.call.nameExact("list!").l) { case list :: Nil =>
        inside(list.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }

      inside(cpg.call.nameExact("pair_list!").l) { case pairList :: Nil =>
        inside(pairList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 4]"
          array.argument.isLiteral.code.l shouldBe List("1", "2", "3", "4")
        }
      }

      inside(cpg.call.nameExact("plus_list!").filter(_.code == "plus_list!(1, 2)").l) { case plusList :: Nil =>
        inside(plusList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }

      inside(cpg.call.nameExact("plus_list!").filter(_.code == "plus_list!(1, 2,)").l) { case plusList :: Nil =>
        inside(plusList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }

      inside(cpg.call.nameExact("semi_list!").l) { case semiList :: Nil =>
        inside(semiList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }

      inside(cpg.call.nameExact("semi_plus_list!").l) { case semiPlusList :: Nil =>
        inside(semiPlusList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.typeFullName shouldBe "[i32; 2]"
          array.argument.isLiteral.code.l shouldBe List("1", "2")
        }
      }
    }

    "substitute empty repeated macro expansions at call sites" in {
      inside(cpg.call.nameExact("empty_list!").l) { case emptyList :: Nil =>
        inside(emptyList.argument.isCall.nameExact(Operators.arrayInitializer).l) { case array :: Nil =>
          array.argument.l shouldBe empty
        }
      }

      inside(cpg.call.nameExact("empty_calls!").l) { case emptyCalls :: Nil =>
        inside(emptyCalls.argument.isCall.nameExact("sink0").l) { case sink0 :: Nil =>
          sink0.argument.l shouldBe empty
        }
      }
    }

    "not create unknown nodes for simple repetition macro definitions" in {
      cpg.all.collectAll[Unknown].codeExact(
        "macro_rules! list { ($($x:expr),*) => { [$($x),*] }; }",
        "macro_rules! tupled { ($($x:expr),*) => { ($($x),*) }; }",
        "macro_rules! calls { ($($x:expr),*) => { sink($($x),*) }; }",
        "macro_rules! pair_list { ($($x:expr => $y:expr),*) => { [$($x, $y),*] }; }",
        "macro_rules! pair_calls { ($($x:expr => $y:expr),*) => { sink4($($x, $y),*) }; }",
        "macro_rules! sum_list { ($($x:expr),*) => { [$($x + 1),*] }; }",
        "macro_rules! empty_list { ($($x:expr),*) => { [$($x),*] }; }",
        "macro_rules! empty_calls { ($($x:expr),*) => { sink0($($x),*) }; }",
        "macro_rules! plus_list { ($($x:expr),+ $(,)?) => { [$($x),*] }; }",
        "macro_rules! semi_list { ($($x:expr);*) => { [$($x),*] }; }",
        "macro_rules! semi_plus_list { ($($x:expr);+) => { [$($x),*] }; }"
      ).l shouldBe empty
    }
  }

  "macro calls through imported aliases" should {
    val cpg = code("""
        |use crate::macros::hello as hi;
        |use crate::tools as tools;
        |
        |fn main() {
        | hi!();
        | tools::make!();
        |}
        |""".stripMargin)

    "use the imported macro fullName for a renamed single-segment call" in {
      cpg.call.nameExact("hi!").methodFullName.l shouldBe List("crate::macros::hello!")
    }

    "rewrite a module alias at the head of a qualified macro call" in {
      cpg.call.nameExact("make!").methodFullName.l shouldBe List("crate::tools::make!")
    }
  }

  "macro calls through wildcard imports" should {
    val cpg = code("""
        |use crate::macros::*;
        |
        |fn main() {
        | make!();
        |}
        |""".stripMargin)

    "use the wildcard import prefix for an unqualified macro call" in {
      cpg.call.nameExact("make!").methodFullName.l shouldBe List("crate::macros::make!")
    }
  }

  "a block-local macro import" should {
    val cpg = code("""
        |use crate::outer::make;
        |
        |fn main() {
        | use crate::inner::make;
        | make!();
        |}
        |""".stripMargin)

    "shadow an outer module import" in {
      cpg.call.nameExact("make!").methodFullName.l shouldBe List("crate::inner::make!")
    }
  }

  "simple macro token-tree arguments" should {
    val cpg = code("""
        |fn main(x: i32) {
        | println!("{}", x);
        | let _ = vec![1, 2, x];
        | wrap!((1, 2), [3, 4]);
        | inspect!(x + 1, !false, helper(x, 1));
        |}
        |
        |fn helper(left: i32, right: i32) -> i32 { left + right }
        |""".stripMargin)

    "preserve literal and identifier arguments for a parenthesized macro call" in {
      inside(cpg.call.nameExact("println!").argument.l) { case (format: Literal) :: (x: Identifier) :: Nil =>
        format.code shouldBe "\"{}\""
        format.typeFullName shouldBe "&str"

        x.name shouldBe "x"
        x.typeFullName shouldBe "i32"
      }
    }

    "preserve literal and identifier arguments for a bracketed macro call" in {
      inside(cpg.call.nameExact("vec!").argument.l) {
        case (one: Literal) :: (two: Literal) :: (x: Identifier) :: Nil =>
          one.code shouldBe "1"
          one.typeFullName shouldBe "i32"

          two.code shouldBe "2"
          two.typeFullName shouldBe "i32"

          x.name shouldBe "x"
          x.typeFullName shouldBe "i32"
      }
    }

    "preserve tuple and array arguments for nested token-tree arguments" in {
      inside(cpg.call.nameExact("wrap!").argument.l) { case (tuple: Call) :: (array: Call) :: Nil =>
        tuple.name shouldBe RustOperators.tupleLiteral
        tuple.code shouldBe "(1, 2)"
        tuple.typeFullName shouldBe "(i32, i32)"
        tuple.argument.isLiteral.code.l shouldBe List("1", "2")

        array.name shouldBe Operators.arrayInitializer
        array.code shouldBe "[3, 4]"
        array.typeFullName shouldBe "[i32; 2]"
        array.argument.isLiteral.code.l shouldBe List("3", "4")
      }
    }

    "preserve expression arguments" in {
      inside(cpg.call.nameExact("inspect!").argument.l) { case (add: Call) :: (not: Call) :: (helper: Call) :: Nil =>
        add.name shouldBe Operators.addition
        add.typeFullName shouldBe "i32"
        inside(add.argument.l) { case (x: Identifier) :: (one: Literal) :: Nil =>
          x.name shouldBe "x"
          one.code shouldBe "1"
        }

        not.name shouldBe Operators.logicalNot
        not.typeFullName shouldBe "bool"
        not.argument.isLiteral.code.l shouldBe List("false")

        helper.name shouldBe "helper"
        helper.code shouldBe "helper(x, 1)"
        inside(helper.argument.l) { case (x: Identifier) :: (one: Literal) :: Nil =>
          x.name shouldBe "x"
          one.code shouldBe "1"
        }
      }
    }
  }
}
