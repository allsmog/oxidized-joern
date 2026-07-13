package io.joern.abap2cpg.parser

import io.joern.abap2cpg.parser.AbapIntermediateAst.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.Files

class AbapJsonParserTests extends AnyWordSpec with Matchers {

  "AbapJsonParser" should {

    "parse compound EDITOR-CALL tokens with the report argument" in {
      val json =
        """{
          |  "file": "test.abap",
          |  "objectType": "PROG",
          |  "statements": [
          |    {
          |      "type": "Form",
          |      "tokens": [{"str": "FORM"}, {"str": "run"}, {"str": "."}],
          |      "start": {"row": 1, "col": 1},
          |      "end": {"row": 1, "col": 9}
          |    },
          |    {
          |      "type": "EditorCall",
          |      "tokens": [{"str": "EDITOR-CALL"}, {"str": "FOR"}, {"str": "REPORT"}, {"str": "lv_prog"}, {"str": "."}],
          |      "start": {"row": 2, "col": 1},
          |      "end": {"row": 2, "col": 31}
          |    },
          |    {
          |      "type": "EndForm",
          |      "tokens": [{"str": "ENDFORM"}, {"str": "."}],
          |      "start": {"row": 3, "col": 1},
          |      "end": {"row": 3, "col": 8}
          |    }
          |  ]
          |}""".stripMargin
      val path = Files.createTempFile("abap-json-parser-", ".json")
      try {
        Files.writeString(path, json)
        val program = AbapJsonParser().parseFile(path).get
        val call = program.methods.head.body.get.statements.collectFirst { case call: CallExpr =>
          call
        }.get

        call.targetName shouldBe "EDITOR_CALL"
        call.arguments.map(_.name) shouldBe Seq(Some("REPORT"))
        call.arguments.head.value match {
          case IdentifierExpr(name, _) => name shouldBe "lv_prog"
          case other                   => fail(s"expected report identifier, got $other")
        }
      } finally {
        Files.deleteIfExists(path)
      }
    }

    "parse reference-style split CLASS-METHODS tokens as static method signatures" in {
      val json =
        """{
          |  "file": "test.abap",
          |  "objectType": "CLAS",
          |  "statements": [
          |    {
          |      "type": "ClassDefinition",
          |      "tokens": [{"str": "CLASS"}, {"str": "z_test"}, {"str": "DEFINITION"}, {"str": "."}],
          |      "start": {"row": 1, "col": 1},
          |      "end": {"row": 1, "col": 25}
          |    },
          |    {
          |      "type": "MethodDef",
          |      "tokens": [{"str": "CLASS"}, {"str": "-"}, {"str": "METHODS"}, {"str": "run"}, {"str": "."}],
          |      "start": {"row": 2, "col": 3},
          |      "end": {"row": 2, "col": 21}
          |    },
          |    {
          |      "type": "EndClass",
          |      "tokens": [{"str": "ENDCLASS"}, {"str": "."}],
          |      "start": {"row": 3, "col": 1},
          |      "end": {"row": 3, "col": 10}
          |    }
          |  ]
          |}""".stripMargin
      val path = Files.createTempFile("abap-json-parser-", ".json")
      try {
        Files.writeString(path, json)
        val program = AbapJsonParser().parseFile(path).get

        program.classes.head.methods.head.name shouldBe "run"
        program.classes.head.methods.head.isStatic shouldBe true
      } finally {
        Files.deleteIfExists(path)
      }
    }

    "parse reference-style MOVE statements as assignments" in {
      val json =
        """{
          |  "file": "test.abap",
          |  "objectType": "PROG",
          |  "statements": [
          |    {
          |      "type": "Form",
          |      "tokens": [{"str": "FORM"}, {"str": "run"}, {"str": "."}],
          |      "start": {"row": 1, "col": 1},
          |      "end": {"row": 1, "col": 10}
          |    },
          |    {
          |      "type": "Move",
          |      "tokens": [{"str": "MOVE"}, {"str": "2"}, {"str": "TO"}, {"str": "lv_y"}, {"str": "."}],
          |      "start": {"row": 2, "col": 1},
          |      "end": {"row": 2, "col": 16}
          |    },
          |    {
          |      "type": "EndForm",
          |      "tokens": [{"str": "ENDFORM"}, {"str": "."}],
          |      "start": {"row": 3, "col": 1},
          |      "end": {"row": 3, "col": 9}
          |    }
          |  ]
          |}""".stripMargin
      val path = Files.createTempFile("abap-json-parser-", ".json")
      try {
        Files.writeString(path, json)
        val program = AbapJsonParser().parseFile(path).get
        val assignment = program.methods.head.body.get.statements.collectFirst { case assignment: AssignmentStmt =>
          assignment
        }.get

        assignment.target shouldBe IdentifierExpr("lv_y", TextSpan(Some(Position(2, 1)), Some(Position(2, 16)), "MOVE 2 TO lv_y ."))
        assignment.value shouldBe LiteralExpr("2", "NUMBER", TextSpan(Some(Position(2, 1)), Some(Position(2, 16)), "MOVE 2 TO lv_y ."))
      } finally {
        Files.deleteIfExists(path)
      }
    }
  }
}
