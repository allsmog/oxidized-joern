package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.DispatchTypes
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.semanticcpg.language.*

class CallTests extends Rust2CpgSuite(noSysRoot = true) {

  "`let x = foo()`" should {
    val cpg = code("""
        |fn main() {
        | let x = foo();
        |}
        |""".stripMargin)

    "create a local for the binding" in {
      inside(cpg.method.name("main").block.local.name("x").l) { case local :: Nil =>
        local.typeFullName shouldBe Defines.Any
        local.code shouldBe "x"
      }
    }

    "have the call as the assignment's second argument" in {
      inside(cpg.assignment.argument(2).l) { case (rhs: Call) :: Nil =>
        rhs.name shouldBe "foo"
        rhs.code shouldBe "foo()"
        rhs.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::foo"
        rhs.typeFullName shouldBe Defines.Any
      }
    }
  }

  "an unresolved fully-qualified call" should {
    val cpg = code("""
        |fn main() {
        | a::b::c();
        |}
        |""".stripMargin)

    "preserve the full path in methodFullName" in {
      inside(cpg.call.name("c").l) { case cCall :: Nil =>
        cCall.code shouldBe "a::b::c()"
        cCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        cCall.argument shouldBe empty
        cCall.methodFullName shouldBe "a::b::c"
      }
    }
  }

  "calls through imported aliases" should {
    val cpg = code("""
        |use crate::util::do_it as run;
        |use std::fs as filesystem;
        |extern crate serde_json as json;
        |
        |fn main() {
        | run();
        | filesystem::read("Cargo.toml");
        | json::from_str("{}");
        |}
        |""".stripMargin)

    "use the imported function fullName for a renamed single-segment call" in {
      cpg.call.nameExact("run").methodFullName.l shouldBe List("crate::util::do_it")
    }

    "rewrite a module alias at the head of a qualified call" in {
      cpg.call.nameExact("read").methodFullName.l shouldBe List("std::fs::read")
    }

    "rewrite an extern-crate alias at the head of a qualified call" in {
      cpg.call.nameExact("from_str").methodFullName.l shouldBe List("serde_json::from_str")
    }
  }

  "calls through wildcard imports" should {
    val cpg = code("""
        |use crate::tools::*;
        |
        |fn main() {
        | run();
        |}
        |""".stripMargin)

    "use the wildcard import prefix for an unqualified call" in {
      cpg.call.nameExact("run").methodFullName.l shouldBe List("crate::tools::run")
    }
  }

  "a block-local wildcard import" should {
    val cpg = code("""
        |use crate::outer::*;
        |
        |fn main() {
        | use crate::inner::*;
        | run();
        |}
        |""".stripMargin)

    "shadow an outer module wildcard import" in {
      cpg.call.nameExact("run").methodFullName.l shouldBe List("crate::inner::run")
    }
  }

  "ambiguous wildcard imports" should {
    val cpg = code("""
        |use crate::left::*;
        |use crate::right::*;
        |
        |fn main() {
        | run();
        |}
        |""".stripMargin)

    "leave the call unresolved" in {
      cpg.call.nameExact("run").methodFullName.l shouldBe List(s"${Defines.UnresolvedNamespace}::run")
    }
  }

  "a module-level import declared after a function" should {
    val cpg = code("""
        |fn main() {
        | helper();
        |}
        |
        |use crate::util::helper;
        |""".stripMargin)

    "apply to calls in the same module" in {
      cpg.call.nameExact("helper").methodFullName.l shouldBe List("crate::util::helper")
    }
  }

  "a block-local import" should {
    val cpg = code("""
        |use crate::outer::run;
        |
        |fn main() {
        | use crate::inner::run;
        | run();
        |}
        |""".stripMargin)

    "shadow an outer module import" in {
      cpg.call.nameExact("run").methodFullName.l shouldBe List("crate::inner::run")
    }
  }

  "a type-qualified associated function call" should {
    val callCode = "<Worker as Make>::make()"
    val cpg = code(s"""
        |trait Make {
        | fn make() -> i32;
        |}
        |
        |struct Worker;
        |
        |impl Make for Worker {
        | fn make() -> i32 { 1 }
        |}
        |
        |fn main() {
        | let value = $callCode;
        |}
        |""".stripMargin)

    "lower as a static call" in {
      inside(cpg.call.nameExact("make").codeExact(callCode).l) { case make :: Nil =>
        make.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        make.methodFullName == Defines.DynamicCallUnknownFullName shouldBe false
      }
    }

    "not create unknown nodes for the type-qualified path" in {
      cpg.all.collectAll[Unknown].codeExact(callCode, "<Worker as Make>", "make").l shouldBe empty
    }
  }

  "local receiver method calls without sysroot" should {
    val cpg = code("""
        |struct Point { x: i32 }
        |
        |impl Point {
        | fn value(&self) -> i32 { self.x }
        |}
        |
        |fn main() {
        | let p: Point = Point { x: 1 };
        | let r: &Point = &p;
        | p.value();
        | r.value();
        |}
        |""".stripMargin)

    "resolve value receiver calls to the local impl method" in {
      inside(cpg.call.nameExact("value").codeExact("p.value()").l) { case value :: Nil =>
        value.methodFullName shouldBe "rust2cpgtest::Point::value"
        value.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "resolve reference receiver calls to the local impl method" in {
      inside(cpg.call.nameExact("value").codeExact("r.value()").l) { case value :: Nil =>
        value.methodFullName shouldBe "rust2cpgtest::Point::value"
        value.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "preserve concrete receiver types" in {
      inside(cpg.call.nameExact("value").codeExact("p.value()").receiver.l) { case (receiver: Identifier) :: Nil =>
        receiver.typeFullName shouldBe "rust2cpgtest::Point"
      }
      inside(cpg.call.nameExact("value").codeExact("r.value()").receiver.l) { case (receiver: Identifier) :: Nil =>
        receiver.typeFullName shouldBe "&rust2cpgtest::Point"
      }
    }
  }

  "an unresolved chained method call" should {
    val cpg = code("""
        |fn main() {
        | external().chain().tail();
        |}
        |""".stripMargin)

    "have DynamicCallUnknownFullName for the inner method call" in {
      cpg.call.nameExact("chain").methodFullName.l shouldBe List(Defines.DynamicCallUnknownFullName)
    }

    "have DynamicCallUnknownFullName for the outer method call" in {
      cpg.call.nameExact("tail").methodFullName.l shouldBe List(Defines.DynamicCallUnknownFullName)
    }

    "have an unresolved-namespace methodFullName for the function call" in {
      cpg.call.nameExact("external").methodFullName.l shouldBe List(s"${Defines.UnresolvedNamespace}::external")
    }

    "have STATIC_DISPATCH for the function call" in {
      cpg.call.nameExact("external").dispatchType.l shouldBe List(DispatchTypes.STATIC_DISPATCH)
    }

    "have DYNAMIC_DISPATCH for each method call in the chain" in {
      cpg.call.nameExact("chain", "tail").dispatchType.toSet shouldBe Set(DispatchTypes.DYNAMIC_DISPATCH)
    }

    "have Any as typeFullName for each call in the chain" in {
      cpg.call.nameExact("external", "chain", "tail").typeFullName.toSet shouldBe Set(Defines.Any)
    }
  }

  "a call to a function defined in the same file" should {
    val cpg = code("""
        |fn callee() {}
        |fn main() { callee(); }
        |""".stripMargin)

    "have a crate-prefixed methodFullName" in {
      cpg.call.name("callee").methodFullName.l shouldBe List("rust2cpgtest::callee")
    }
  }

  "a `Vec::push` call" should {
    val cpg = code("""
        |fn foo(xs: Vec<i32>) {
        | xs.push(1);
        |}
        |""".stripMargin)

    "have DynamicCallUnknownFullName as methodFullName" in {
      inside(cpg.call.nameExact("push").l) { case push :: Nil =>
        push.code shouldBe "xs.push(1)"
        push.methodFullName shouldBe Defines.DynamicCallUnknownFullName
        push.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        push.typeFullName shouldBe Defines.Any
      }
    }

    "have the variable as receiver" in {
      inside(cpg.call.nameExact("push").receiver.l) { case (receiver: Identifier) :: Nil =>
        receiver.code shouldBe "xs"
        receiver.argumentIndex shouldBe 0
        receiver.typeFullName shouldBe "Vec<i32>"
      }
    }

    "have the receiver and the literal as arguments" in {
      inside(cpg.call.nameExact("push").argument.l) { case (receiver: Identifier) :: (lit: Literal) :: Nil =>
        receiver shouldBe cpg.call.nameExact("push").receiver.head

        lit.code shouldBe "1"
        lit.argumentIndex shouldBe 1
        lit.typeFullName shouldBe "i32"
      }
    }
  }

  "a compiler builtin format_args expression" should {
    val cpg = code("""
        |fn main(name: &str) {
        | let args = builtin # format_args("hello {}", name);
        |}
        |""".stripMargin)

    "lower to a static call" in {
      inside(cpg.call.nameExact("format_args!").l) { case formatArgs :: Nil =>
        formatArgs.code shouldBe """builtin # format_args("hello {}", name)"""
        formatArgs.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::format_args!"
        formatArgs.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      }
    }

    "preserve the format string and formatted arguments" in {
      inside(cpg.call.nameExact("format_args!").argument.l) { case (format: Literal) :: (name: Identifier) :: Nil =>
        format.code shouldBe """"hello {}""""
        name.name shouldBe "name"
      }
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact("""builtin # format_args("hello {}", name)""")
        .l shouldBe empty
    }
  }

  "a compiler builtin offset_of expression" should {
    val cpg = code("""
        |struct Point { x: i32, y: i32 }
        |
        |fn main() {
        | let offset = builtin # offset_of(Point, x);
        |}
        |""".stripMargin)

    "lower to a static call returning usize" in {
      inside(cpg.call.nameExact("offset_of!").l) { case offsetOf :: Nil =>
        offsetOf.code shouldBe "builtin # offset_of(Point, x)"
        offsetOf.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::offset_of!"
        offsetOf.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        offsetOf.typeFullName shouldBe "usize"
      }
    }

    "preserve the target type and field path" in {
      inside(cpg.call.nameExact("offset_of!").argument.l) {
        case (targetType: TypeRef) :: (field: FieldIdentifier) :: Nil =>
          targetType.code shouldBe "Point"
          field.code shouldBe "x"
      }
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact("builtin # offset_of(Point, x)")
        .l shouldBe empty
    }
  }

  "a call on a returned callable expression" should {
    val cpg = code("""
        |fn id(x: i32) -> i32 {
        | x
        |}
        |
        |fn make_adder() -> fn(i32) -> i32 {
        | id
        |}
        |
        |fn main() {
        | let value = make_adder()(1);
        |}
        |""".stripMargin)

    "lower the outer call as dynamic" in {
      inside(cpg.call.codeExact("make_adder()(1)").l) { case returnedCall :: Nil =>
        returnedCall.name shouldBe "make_adder()"
        returnedCall.methodFullName shouldBe Defines.DynamicCallUnknownFullName
        returnedCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      }
    }

    "preserve the callee expression as receiver" in {
      inside(cpg.call.codeExact("make_adder()(1)").receiver.l) { case (callee: Call) :: Nil =>
        callee.name shouldBe "make_adder"
        callee.code shouldBe "make_adder()"
      }
    }

    "preserve explicit arguments" in {
      inside(cpg.call.codeExact("make_adder()(1)").argument(1).l) { case (one: Literal) :: Nil =>
        one.code shouldBe "1"
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("make_adder()(1)").l shouldBe empty
    }
  }

  "a `String` method chain" should {
    val cpg = code("""
        |fn foo() {
        | String::from(" hello ").trim().to_string();
        |}
        |""".stripMargin)

    "lower `trim` as a method call on the result of `from`" in {
      inside(cpg.call.nameExact("trim").l) { case trim :: Nil =>
        trim.code shouldBe """String::from(" hello ").trim()"""
        trim.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        trim.arguments(1) shouldBe empty
        trim.methodFullName shouldBe Defines.DynamicCallUnknownFullName

        inside(trim.receiver.l) { case (from: Call) :: Nil =>
          from.name shouldBe "from"
          from.code shouldBe """String::from(" hello ")"""
          from.argumentIndex shouldBe 0
        }
      }
    }

    "lower `to_string` as a method call on the result of `trim`" in {
      inside(cpg.call.nameExact("to_string").l) { case toString :: Nil =>
        toString.code shouldBe """String::from(" hello ").trim().to_string()"""
        toString.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        toString.arguments(1) shouldBe empty
        toString.methodFullName shouldBe Defines.DynamicCallUnknownFullName

        inside(toString.receiver.l) { case (trim: Call) :: Nil =>
          trim.name shouldBe "trim"
          trim.code shouldBe """String::from(" hello ").trim()"""
          trim.argumentIndex shouldBe 0
        }
      }
    }
  }
}

class CallTestsWithSysroot extends Rust2CpgSuite(noSysRoot = false) {

  "a `Vec` method call resolved against the sysroot" should {
    val cpg = code("""
        |fn foo(xs: Vec<i32>) -> usize {
        | xs.push(1);
        | xs.len()
        |}
        |""".stripMargin)

    "resolve `push` to alloc::vec::Vec" in {
      inside(cpg.call.nameExact("push").l) { case push :: Nil =>
        push.methodFullName shouldBe "alloc::vec::Vec<T, A>::push"
        push.typeFullName shouldBe "()"
      }
    }

    "resolve `len` to alloc::vec::Vec" in {
      inside(cpg.call.nameExact("len").l) { case len :: Nil =>
        len.methodFullName shouldBe "alloc::vec::Vec<T, A>::len"
        len.typeFullName shouldBe "usize"
      }
    }
  }

  "a resolved method call through a dyn trait receiver" should {
    val cpg = code("""
        |trait Draw {
        | fn draw(&self);
        |}
        |
        |fn run(x: &dyn Draw) {
        | x.draw();
        |}
        |""".stripMargin)

    "use dynamic dispatch even when astgen resolves the trait method" in {
      inside(cpg.call.nameExact("draw").l) { case draw :: Nil =>
        draw.methodFullName shouldBe "rust2cpgtest::Draw::draw"
        draw.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        draw.typeFullName shouldBe "()"
      }
    }

    "preserve the dyn receiver as the call receiver" in {
      inside(cpg.call.nameExact("draw").receiver.l) { case (receiver: Identifier) :: Nil =>
        receiver.name shouldBe "x"
        receiver.typeFullName shouldBe "&dyn Draw"
        receiver.argumentIndex shouldBe 0
      }
    }
  }

  "a `String` method chain resolved against the sysroot" should {
    val cpg = code("""
        |fn foo() -> String {
        | String::from(" hello ").trim().to_string()
        |}
        |""".stripMargin)

    "resolve `from` to core::convert::From" in {
      inside(cpg.call.nameExact("from").l) { case from :: Nil =>
        from.methodFullName shouldBe "core::convert::From<T>::from"
        from.typeFullName shouldBe "alloc::string::String"
      }
    }

    "resolve `trim` to str::trim" in {
      inside(cpg.call.nameExact("trim").l) { case trim :: Nil =>
        trim.code shouldBe """String::from(" hello ").trim()"""
        trim.methodFullName shouldBe "str::trim"
        trim.typeFullName shouldBe "&str"
      }
    }

    "resolve `to_string` to alloc::string::ToString" in {
      inside(cpg.call.nameExact("to_string").l) { case toString :: Nil =>
        toString.methodFullName shouldBe "<T as alloc::string::ToString>::to_string"
        toString.typeFullName shouldBe "alloc::string::String"
      }
    }
  }
}
