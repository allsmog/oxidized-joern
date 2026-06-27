package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.NodeTypes
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*

class TypeDeclarationTests extends Rust2CpgSuite(noSysRoot = true) {

  "a type alias" should {
    val cpg = code("type UserId = u64;")

    "lower to a TYPE_DECL with alias metadata" in {
      inside(cpg.typeDecl.nameExact("UserId").l) { case userId :: Nil =>
        userId.fullName shouldBe "rust2cpgtest::UserId"
        userId.code shouldBe "type UserId = u64;"
        userId.aliasTypeFullName shouldBe Option("u64")
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("type UserId = u64;").l shouldBe empty
    }
  }

  "a union declaration" should {
    val cpg = code("""
        |union Number {
        | i: i32,
        | f: f32,
        |}
        |""".stripMargin)

    "lower to a TYPE_DECL with members" in {
      inside(cpg.typeDecl.nameExact("Number").l) { case number :: Nil =>
        number.fullName shouldBe "rust2cpgtest::Number"
        number.code should startWith("union Number")
      }

      inside(cpg.typeDecl.nameExact("Number").member.sortBy(_.name).l) { case i :: f :: Nil =>
        i.name shouldBe "f"
        i.typeFullName shouldBe "f32"
        f.name shouldBe "i"
        f.typeFullName shouldBe "i32"
      }
    }

    "not create an unknown node" in {
      cpg.all.collectAll[Unknown].codeExact("union Number {\n i: i32,\n f: f32,\n}").l shouldBe empty
    }
  }

  "an enum declaration" should {
    val cpg = code("""
        |enum Message {
        | Quit,
        | Move { x: i32, y: i32 },
        | Write(String),
        |}
        |""".stripMargin)

    "lower to a TYPE_DECL with one member per variant" in {
      inside(cpg.typeDecl.nameExact("Message").l) { case message :: Nil =>
        message.fullName shouldBe "rust2cpgtest::Message"
        message.code should startWith("enum Message")
      }

      inside(cpg.typeDecl.nameExact("Message").member.sortBy(_.name).l) { case move :: quit :: write :: Nil =>
        move.name shouldBe "Move"
        move.typeFullName shouldBe "rust2cpgtest::Message"

        quit.name shouldBe "Quit"
        quit.typeFullName shouldBe "rust2cpgtest::Message"

        write.name shouldBe "Write"
        write.typeFullName shouldBe "rust2cpgtest::Message"
      }
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact("enum Message {\n Quit,\n Move { x: i32, y: i32 },\n Write(String),\n}")
        .l shouldBe empty
    }
  }

  "a trait declaration" should {
    val cpg = code("""
        |trait Drawable {
        | type Error;
        | const KIND: u8;
        | fn draw(&self) -> Result<(), Self::Error>;
        |}
        |""".stripMargin)

    "lower to a TYPE_DECL" in {
      inside(cpg.typeDecl.nameExact("Drawable").l) { case drawable :: Nil =>
        drawable.fullName shouldBe "rust2cpgtest::Drawable"
        drawable.code should startWith("trait Drawable")
      }
    }

    "parent associated types and methods by the trait" in {
      cpg.typeDecl.nameExact("Error").fullName.l shouldBe List("rust2cpgtest::Drawable::Error")

      inside(cpg.method.nameExact("draw").l) { case draw :: Nil =>
        draw.fullName shouldBe "rust2cpgtest::Drawable::draw"
        draw.astParentType shouldBe NodeTypes.TYPE_DECL
        draw.astParentFullName shouldBe "rust2cpgtest::Drawable"
      }
    }

    "preserve associated constants as typed locals" in {
      cpg.typeDecl.nameExact("Drawable").astChildren.isLocal.nameExact("KIND").typeFullName.l shouldBe List("u8")
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact("trait Drawable {\n type Error;\n const KIND: u8;\n fn draw(&self) -> Result<(), Self::Error>;\n}")
        .l shouldBe empty
    }
  }

  "an inherent impl block" should {
    val cpg = code("""
        |struct Point { x: i32 }
        |
        |impl Point {
        | const ORIGIN_X: i32 = 0;
        | type Coord = i32;
        | fn new(x: i32) -> Point { Point { x } }
        |}
        |""".stripMargin)

    "parent associated types and methods by the implemented type" in {
      inside(cpg.method.nameExact("new").l) { case newMethod :: Nil =>
        newMethod.fullName shouldBe "rust2cpgtest::Point::new"
        newMethod.astParentType shouldBe NodeTypes.TYPE_DECL
        newMethod.astParentFullName shouldBe "rust2cpgtest::Point"
      }

      inside(cpg.typeDecl.nameExact("Coord").l) { case coord :: Nil =>
        coord.fullName shouldBe "rust2cpgtest::Point::Coord"
        coord.aliasTypeFullName shouldBe Option("i32")
        coord.astParentType shouldBe NodeTypes.TYPE_DECL
        coord.astParentFullName shouldBe "rust2cpgtest::Point"
      }
    }

    "preserve associated constants as typed locals" in {
      cpg.local.nameExact("ORIGIN_X").typeFullName.l shouldBe List("i32")
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact(
          "impl Point {\n const ORIGIN_X: i32 = 0;\n type Coord = i32;\n fn new(x: i32) -> Point { Point { x } }\n}"
        )
        .l shouldBe empty
    }
  }

  "a trait impl block" should {
    val cpg = code("""
        |trait Drawable {
        | fn draw(&self);
        |}
        |
        |struct Point { x: i32 }
        |
        |impl Drawable for Point {
        | fn draw(&self) {}
        |}
        |""".stripMargin)

    "keep trait declarations and trait implementations distinct" in {
      cpg.method.fullNameExact("rust2cpgtest::Drawable::draw").size shouldBe 1

      inside(cpg.method.fullNameExact("rust2cpgtest::Point::draw").l) { case draw :: Nil =>
        draw.astParentType shouldBe NodeTypes.TYPE_DECL
        draw.astParentFullName shouldBe "rust2cpgtest::Point"
      }
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact("impl Drawable for Point {\n fn draw(&self) {}\n}")
        .l shouldBe empty
    }
  }

  "an extern block" should {
    val cpg = code("""
        |unsafe extern "C" {
        | fn strlen(s: *const u8) -> usize;
        | fn printf(format: *const u8, ...);
        | fn takes_type_only(i32);
        | static errno: i32;
        | type FILE;
        |}
        |""".stripMargin)

    "lower extern functions as methods" in {
      inside(cpg.method.nameExact("strlen").l) { case strlen :: Nil =>
        strlen.fullName shouldBe "rust2cpgtest::strlen"
        strlen.methodReturn.typeFullName shouldBe "usize"
      }

      inside(cpg.method.nameExact("strlen").parameter.l) { case param :: Nil =>
        param.name shouldBe "s"
        param.typeFullName shouldBe "*const u8"
      }
    }

    "lower extern type-only parameters" in {
      inside(cpg.method.nameExact("takes_type_only").parameter.l) { case param :: Nil =>
        param.name shouldBe "<param>1"
        param.code shouldBe "i32"
        param.typeFullName shouldBe "i32"
        param.index shouldBe 1
        param.isVariadic shouldBe false
      }
    }

    "lower extern variadic parameters" in {
      inside(cpg.method.nameExact("printf").parameter.sortBy(_.index).l) { case format :: ellipsis :: Nil =>
        format.name shouldBe "format"
        format.code shouldBe "format: *const u8"
        format.typeFullName shouldBe "*const u8"
        format.index shouldBe 1
        format.isVariadic shouldBe false

        ellipsis.name shouldBe "<param>2"
        ellipsis.code shouldBe "<param>2..."
        ellipsis.typeFullName shouldBe Defines.Any
        ellipsis.index shouldBe 2
        ellipsis.isVariadic shouldBe true
      }
    }

    "lower extern statics and types" in {
      cpg.local.nameExact("errno").typeFullName.l shouldBe List("i32")

      inside(cpg.typeDecl.nameExact("FILE").l) { case file :: Nil =>
        file.fullName shouldBe "rust2cpgtest::FILE"
        file.aliasTypeFullName shouldBe empty
      }
    }

    "not create an unknown node" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact(
          "unsafe extern \"C\" {\n fn strlen(s: *const u8) -> usize;\n fn printf(format: *const u8, ...);\n fn takes_type_only(i32);\n static errno: i32;\n type FILE;\n}"
        )
        .l shouldBe empty
    }
  }
}
