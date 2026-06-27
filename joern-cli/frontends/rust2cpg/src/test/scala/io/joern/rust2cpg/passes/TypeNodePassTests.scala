package io.joern.rust2cpg.passes

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.DispatchTypes
import io.shiftleft.codepropertygraph.generated.nodes.{Call, Unknown}
import io.shiftleft.semanticcpg.language.*

class TypeNodePassTests extends Rust2CpgSuite(noSysRoot = true) {

  "TypeNodePass" should {

    "create correct types for locals" in {
      val cpg = code("""
          |fn foo() {
          | let x: usize = 10;
          |}
          |""".stripMargin)
      inside(cpg.method.name("foo").block.local.name("x").l) { case local :: Nil =>
        local.evalType.l shouldBe List("usize")

        inside(local.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.name shouldBe "usize"
          typ.fullName shouldBe "usize"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for integer literals" in {
      val cpg = code("""
          |fn main() {
          | let x = 42;
          |}
          |""".stripMargin)
      inside(cpg.literal.l) { case lit :: Nil =>
        lit.evalType.l shouldBe List("i32")

        inside(lit.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "i32"
          typ.name shouldBe "i32"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for byte string literals" in {
      val cpg = code("""
          |fn main() {
          | let s = b"hi";
          |}
          |""".stripMargin)
      inside(cpg.literal.l) { case lit :: Nil =>
        lit.evalType.l shouldBe List("&[u8; 2]")

        inside(lit.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "&[u8; 2]"
          typ.name shouldBe "&[u8; 2]"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for parameters" in {
      val cpg = code("""
          |fn id(x: i32) -> i32 {
          | x
          |}
          |""".stripMargin)
      inside(cpg.method.name("id").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("i32")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "i32"
          typ.name shouldBe "i32"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for parameters with struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn foo(y: Foo) {}
          |""".stripMargin)
      inside(cpg.method.name("foo").parameter.name("y").l) { case param :: Nil =>
        param.evalType.l shouldBe List("rust2cpgtest::Foo")
        param.typ.referencedTypeDecl.l shouldBe cpg.typeDecl.fullNameExact("rust2cpgtest::Foo").l
      }
    }

    "create correct types for imported type aliases" in {
      val cpg = code("""
          |fn consume(
          | x: Renamed,
          | y: models::Thing,
          | xs: Vec<Renamed>,
          | cb: fn(Renamed) -> models::Thing
          |) -> Option<Renamed> {
          | todo!()
          |}
          |
          |trait Wrap<T> {}
          |
          |fn bounded(x: &dyn Wrap<Renamed>) -> impl Wrap<Renamed> {
          | todo!()
          |}
          |
          |fn callable(f: impl Fn(Renamed) -> models::Thing) -> impl Fn(Renamed) -> models::Thing {
          | todo!()
          |}
          |
          |use crate::models::Thing as Renamed;
          |use crate::models as models;
          |
          |struct Holder {
          | direct: Renamed,
          | qualified: models::Thing,
          | nested: Option<Renamed>,
          | callback: fn(Renamed) -> models::Thing,
          |}
          |""".stripMargin)

      cpg.method.nameExact("consume").parameter.nameExact("x").evalType.l shouldBe List("crate::models::Thing")
      cpg.method.nameExact("consume").parameter.nameExact("y").evalType.l shouldBe List("crate::models::Thing")
      cpg.method.nameExact("consume").parameter.nameExact("xs").evalType.l shouldBe List("Vec<crate::models::Thing>")
      cpg.method.nameExact("consume").parameter.nameExact("cb").evalType.l shouldBe List(
        "fn(crate::models::Thing) -> crate::models::Thing"
      )
      cpg.method.nameExact("consume").methodReturn.typeFullName.l shouldBe List("Option<crate::models::Thing>")

      cpg.method.nameExact("bounded").parameter.nameExact("x").evalType.l shouldBe List("&dyn Wrap<crate::models::Thing>")
      cpg.method.nameExact("bounded").methodReturn.typeFullName.l shouldBe List("impl Wrap<crate::models::Thing>")

      cpg.method.nameExact("callable").parameter.nameExact("f").evalType.l shouldBe List(
        "impl Fn(crate::models::Thing) -> crate::models::Thing"
      )
      cpg.method.nameExact("callable").methodReturn.typeFullName.l shouldBe List(
        "impl Fn(crate::models::Thing) -> crate::models::Thing"
      )

      cpg.typeDecl.nameExact("Holder").member.nameExact("direct").typeFullName.l shouldBe List("crate::models::Thing")
      cpg.typeDecl.nameExact("Holder").member.nameExact("qualified").typeFullName.l shouldBe List("crate::models::Thing")
      cpg.typeDecl.nameExact("Holder").member.nameExact("nested").typeFullName.l shouldBe List("Option<crate::models::Thing>")
      cpg.typeDecl.nameExact("Holder").member.nameExact("callback").typeFullName.l shouldBe List(
        "fn(crate::models::Thing) -> crate::models::Thing"
      )

      inside(cpg.typ.fullNameExact("crate::models::Thing").l) { case typ :: Nil =>
        typ.fullName shouldBe "crate::models::Thing"
        typ.name shouldBe "Thing"
      }
    }

    "create correct types for wildcard-imported type aliases" in {
      val cpg = code("""
          |fn consume(x: WildThing, y: Option<WildThing>) -> WildThing {
          | todo!()
          |}
          |
          |use crate::models::*;
          |""".stripMargin)

      cpg.method.nameExact("consume").parameter.nameExact("x").evalType.l shouldBe List("crate::models::WildThing")
      cpg.method.nameExact("consume").parameter.nameExact("y").evalType.l shouldBe List("Option<crate::models::WildThing>")
      cpg.method.nameExact("consume").methodReturn.typeFullName.l shouldBe List("crate::models::WildThing")

      inside(cpg.typ.fullNameExact("crate::models::WildThing").l) { case typ :: Nil =>
        typ.name shouldBe "WildThing"
      }
    }

    "create correct types for references" in {
      val cpg = code("""
          |fn f(x: &i32) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("&i32")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "&i32"
          typ.name shouldBe "&i32"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for references to struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn f(p: &Foo) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("p").l) { case param :: Nil =>
        param.evalType.l shouldBe List("&rust2cpgtest::Foo")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "&rust2cpgtest::Foo"
          typ.name shouldBe "&rust2cpgtest::Foo"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for tuples" in {
      val cpg = code("""
          |fn f(x: (i32, bool)) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("(i32, bool)")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "(i32, bool)"
          typ.name shouldBe "(i32, bool)"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for tuples of struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn f(x: (Foo, i32)) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("(rust2cpgtest::Foo, i32)")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "(rust2cpgtest::Foo, i32)"
          typ.name shouldBe "(rust2cpgtest::Foo, i32)"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for arrays" in {
      val cpg = code("""
          |fn f(x: [i32; 4]) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("[i32; 4]")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "[i32; 4]"
          typ.name shouldBe "[i32; 4]"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for arrays of struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn f(x: [Foo; 4]) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("[rust2cpgtest::Foo; 4]")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "[rust2cpgtest::Foo; 4]"
          typ.name shouldBe "[rust2cpgtest::Foo; 4]"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for slices" in {
      val cpg = code("""
          |fn f(x: &[i32]) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("&[i32]")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "&[i32]"
          typ.name shouldBe "&[i32]"
          typ.isExternal shouldBe true
        }
      }

    }

    "create correct types for slices of struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn f(x: &[Foo]) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("x").l) { case param :: Nil =>
        param.evalType.l shouldBe List("&[rust2cpgtest::Foo]")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "&[rust2cpgtest::Foo]"
          typ.name shouldBe "&[rust2cpgtest::Foo]"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for raw pointers" in {
      val cpg = code("""
          |fn f(p: *const i32) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("p").l) { case param :: Nil =>
        param.evalType.l shouldBe List("*const i32")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "*const i32"
          typ.name shouldBe "*const i32"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for raw pointers to struct type" in {
      val cpg = code("""
          |struct Foo;
          |fn f(p: *const Foo) {}
          |""".stripMargin)
      inside(cpg.method.name("f").parameter.name("p").l) { case param :: Nil =>
        param.evalType.l shouldBe List("*const rust2cpgtest::Foo")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "*const rust2cpgtest::Foo"
          typ.name shouldBe "*const rust2cpgtest::Foo"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for the never type" in {
      val cpg = code("""
          |fn f() -> ! { loop {} }
          |""".stripMargin)
      inside(cpg.method.name("f").methodReturn.l) { case ret :: Nil =>
        ret.evalType.l shouldBe List("!")

        inside(ret.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "!"
          typ.name shouldBe "!"
          typ.isExternal shouldBe true
        }
      }

    }

    "create correct types for unit returns" in {
      val cpg = code("""
          |fn f() {}
          |""".stripMargin)
      inside(cpg.method.name("f").methodReturn.l) { case ret :: Nil =>
        ret.evalType.l shouldBe List("()")

        inside(ret.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "()"
          typ.name shouldBe "()"
          typ.isExternal shouldBe true
        }
      }
    }
  }
}

class TypeNodePassTestsWithSysroot extends Rust2CpgSuite(noSysRoot = false) {

  "TypeNodePass" should {

    "create correct types for vec! macro locals" in {
      val cpg = code("""
          |fn foo() {
          | let v = vec![1, 2, 3];
          |}
          |""".stripMargin)
      inside(cpg.method.name("foo").block.local.name("v").l) { case local :: Nil =>
        local.evalType.l shouldBe List("alloc::vec::Vec<i32, alloc::alloc::Global>")

        inside(local.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "alloc::vec::Vec<i32, alloc::alloc::Global>"
          typ.name shouldBe "Vec"
          typ.isExternal shouldBe true
        }
      }

      inside(cpg.assignment.argument(2).l) { case (macroCall: Call) :: Nil =>
        macroCall.name shouldBe "vec!"
        macroCall.code shouldBe "vec![1, 2, 3]"
        macroCall.methodFullName shouldBe s"${Defines.UnresolvedNamespace}::vec!"
        macroCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        macroCall.typeFullName shouldBe "alloc::vec::Vec<i32, alloc::alloc::Global>"
      }

      cpg.all.collectAll[Unknown].codeExact("vec![1, 2, 3]").l shouldBe empty
    }

    "create concrete types for inferred generic let annotations" in {
      val cpg = code("""
          |fn foo() {
          | let xs: Vec<_> = Vec::<i32>::new();
          |}
          |""".stripMargin)

      inside(cpg.method.name("foo").block.local.name("xs").l) { case local :: Nil =>
        local.evalType.l shouldBe List("alloc::vec::Vec<i32, alloc::alloc::Global>")

        inside(local.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "alloc::vec::Vec<i32, alloc::alloc::Global>"
          typ.name shouldBe "Vec"
          typ.isExternal shouldBe true
        }
      }

      inside(cpg.assignment.l) { case assignment :: Nil =>
        assignment.typeFullName shouldBe "alloc::vec::Vec<i32, alloc::alloc::Global>"
      }
    }

    "create correct types for generic Vec parameters" in {
      val cpg = code("""
          |fn foo(xs: Vec<i32>) {}
          |""".stripMargin)
      inside(cpg.method.name("foo").parameter.name("xs").l) { case param :: Nil =>
        param.evalType.l shouldBe List("alloc::vec::Vec<i32>")

        inside(param.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "alloc::vec::Vec<i32>"
          typ.name shouldBe "Vec"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for String locals" in {
      val cpg = code("""
          |fn foo() {
          | let s = String::new();
          |}
          |""".stripMargin)
      inside(cpg.method.name("foo").block.local.name("s").l) { case local :: Nil =>
        local.evalType.l shouldBe List("alloc::string::String")

        inside(local.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "alloc::string::String"
          typ.name shouldBe "String"
          typ.isExternal shouldBe true
        }
      }
    }

    "create correct types for String::from locals" in {
      val cpg = code("""
          |fn foo() {
          | let s = String::from("hello");
          |}
          |""".stripMargin)
      inside(cpg.method.name("foo").block.local.name("s").l) { case local :: Nil =>
        local.evalType.l shouldBe List("alloc::string::String")

        inside(local.typ.referencedTypeDecl.l) { case typ :: Nil =>
          typ.fullName shouldBe "alloc::string::String"
          typ.name shouldBe "String"
          typ.isExternal shouldBe true
        }
      }

      inside(cpg.call.nameExact("from").l) { case from :: Nil =>
        from.methodFullName shouldBe "core::convert::From<T>::from"
        from.typeFullName shouldBe "alloc::string::String"
      }
    }
  }
}
