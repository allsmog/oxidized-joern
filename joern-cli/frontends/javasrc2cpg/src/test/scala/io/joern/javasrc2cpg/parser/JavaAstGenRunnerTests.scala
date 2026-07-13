package io.joern.javasrc2cpg.parser

import io.joern.javasrc2cpg.Config
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path}

class JavaAstGenRunnerTests extends AnyWordSpec with Matchers {

  private def writeFile(file: Path, content: String): Unit = {
    file.createWithParentsIfNotExists(createParents = true)
    Files.writeString(file, content)
  }

  "JavaAstGenRunner" should {

    "parse Java source files into JSON documents" in {
      FileUtil.usingTemporaryDirectory("javaastgenTestInput") { inputDir =>
        writeFile(
          inputDir / "demo" / "Sample.java",
          """package demo;
            |class Sample {
            |  int value() { return 1; }
            |}
            |""".stripMargin
        )
        writeFile(inputDir / "notes.txt", "ignored")

        FileUtil.usingTemporaryDirectory("javaastgenTestOut") { outputDir =>
          val config = Config().withInputPath(inputDir.toString).withOutputPath(outputDir.toString)
          val result = new JavaAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(outputDir / "demo" / "Sample.java.json")
            .map(_.toString)

          val document = JavaAstJsonParser.parseFile(outputDir / "demo" / "Sample.java.json")
          document.relativeName shouldBe "demo/Sample.java"
          document.ast.kind shouldBe "program"
          document.ast.descendants.map(_.kind) should contain("class_declaration")
        }
      }
    }

    "respect ignored files regex configuration" in {
      FileUtil.usingTemporaryDirectory("javaastgenTestInput") { inputDir =>
        writeFile(inputDir / "Keep.java", "class Keep {}\n")
        writeFile(inputDir / "Skip.java", "class Skip {}\n")

        FileUtil.usingTemporaryDirectory("javaastgenTestOut") { outputDir =>
          val config = Config()
            .withInputPath(inputDir.toString)
            .withOutputPath(outputDir.toString)
            .withIgnoredFilesRegex(".*Skip.*")
          val result = new JavaAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(outputDir / "Keep.java.json").map(_.toString)
        }
      }
    }
  }
}
