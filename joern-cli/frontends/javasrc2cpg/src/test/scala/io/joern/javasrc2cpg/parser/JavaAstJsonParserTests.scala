package io.joern.javasrc2cpg.parser

import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

class JavaAstJsonParserTests extends AnyWordSpec with Matchers {

  "JavaAstJsonParser" should {

    "parse the javaastgen JSON envelope" in {
      val document = JavaAstJsonParser.parseString {
        """{
          |  "fullName": "/tmp/Foo.java",
          |  "relativeName": "Foo.java",
          |  "ast": {
          |    "kind": "program",
          |    "fieldName": null,
          |    "named": true,
          |    "missing": false,
          |    "extra": false,
          |    "hasError": false,
          |    "startByte": 0,
          |    "endByte": 12,
          |    "start": { "line": 1, "column": 1 },
          |    "end": { "line": 1, "column": 13 },
          |    "code": "class Foo {}",
          |    "children": [
          |      {
          |        "kind": "class_declaration",
          |        "fieldName": null,
          |        "named": true,
          |        "missing": false,
          |        "extra": false,
          |        "hasError": false,
          |        "startByte": 0,
          |        "endByte": 12,
          |        "start": { "line": 1, "column": 1 },
          |        "end": { "line": 1, "column": 13 },
          |        "code": "class Foo {}",
          |        "children": []
          |      }
          |    ]
          |  }
          |}""".stripMargin
      }

      document.fullName shouldBe "/tmp/Foo.java"
      document.relativeName shouldBe "Foo.java"
      document.ast.kind shouldBe "program"
      document.ast.descendants.map(_.kind) should contain("class_declaration")
      document.ast.start.line shouldBe 1
      document.ast.end.column shouldBe 13
    }
  }
}
