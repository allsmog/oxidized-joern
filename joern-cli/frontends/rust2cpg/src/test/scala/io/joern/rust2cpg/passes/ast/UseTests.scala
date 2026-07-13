package io.joern.rust2cpg.passes.ast

import io.joern.rust2cpg.testfixtures.Rust2CpgSuite
import io.shiftleft.codepropertygraph.generated.nodes.Unknown
import io.shiftleft.semanticcpg.language.*

class UseTests extends Rust2CpgSuite(noSysRoot = true) {

  "use declarations" should {
    val cpg = code("""
        |use std::fmt;
        |use std::{io, fs as filesystem, collections::*};
        |
        |fn main() {
        | use crate::util::{self, Thing as Renamed};
        |}
        |""".stripMargin)

    "create one import node per flattened use tree leaf" in {
      val importsByEntity = cpg.imports.l.map(importNode => importNode.importedEntity.get -> importNode).toMap

      importsByEntity.keySet shouldBe Set(
        "std::fmt",
        "std::io",
        "std::fs",
        "std::collections",
        "crate::util",
        "crate::util::Thing"
      )
    }

    "derive imported aliases from the leaf, rename, wildcard, and self forms" in {
      val importsByEntity = cpg.imports.l.map(importNode => importNode.importedEntity.get -> importNode).toMap

      importsByEntity("std::fmt").importedAs shouldBe Option("fmt")
      importsByEntity("std::io").importedAs shouldBe Option("io")
      importsByEntity("std::fs").importedAs shouldBe Option("filesystem")
      importsByEntity("std::collections").importedAs shouldBe Option("*")
      importsByEntity("crate::util").importedAs shouldBe Option("util")
      importsByEntity("crate::util::Thing").importedAs shouldBe Option("Renamed")
    }

    "mark wildcard imports" in {
      val wildcardFlagsByEntity =
        cpg.imports.l.map(importNode => importNode.importedEntity.get -> importNode.isWildcard).toMap

      wildcardFlagsByEntity("std::collections") shouldBe Option(true)
      wildcardFlagsByEntity.removed("std::collections").values.toSet shouldBe Set(Option(false))
    }

    "not create unknown nodes for handled use declarations" in {
      cpg.all
        .collectAll[Unknown]
        .codeExact(
          "use std::fmt;",
          "use std::{io, fs as filesystem, collections::*};",
          "use crate::util::{self, Thing as Renamed};"
        )
        .l shouldBe empty
    }
  }

  "extern crate declarations" should {
    val cpg = code("""
        |extern crate serde;
        |extern crate serde_json as json;
        |""".stripMargin)

    "create import nodes" in {
      val importsByEntity = cpg.imports.l.map(importNode => importNode.importedEntity.get -> importNode).toMap

      importsByEntity("serde").importedAs shouldBe Option("serde")
      importsByEntity("serde_json").importedAs shouldBe Option("json")
      importsByEntity.values.map(_.isExplicit).toSet shouldBe Set(Option(true))
      importsByEntity.values.map(_.isWildcard).toSet shouldBe Set(Option(false))
    }

    "not create unknown nodes for handled extern crate declarations" in {
      cpg.all.collectAll[Unknown].codeExact("extern crate serde;", "extern crate serde_json as json;").l shouldBe empty
    }
  }
}
