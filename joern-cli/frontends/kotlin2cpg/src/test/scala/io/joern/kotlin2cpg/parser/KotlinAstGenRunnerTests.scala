package io.joern.kotlin2cpg.parser

import io.joern.kotlin2cpg.Config
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path}

class KotlinAstGenRunnerTests extends AnyWordSpec with Matchers {

  "KotlinAstGenRunner" should {

    "parse Kotlin source files into JSON documents" in {
      FileUtil.usingTemporaryDirectory("kotlinastgenTestInput") { inputDir =>
        writeFile(
          inputDir / "demo" / "Sample.kt",
          """package demo
            |class Sample {
            |  fun value(): Int = 1
            |}
            |""".stripMargin
        )
        writeFile(inputDir / "notes.txt", "ignored")

        FileUtil.usingTemporaryDirectory("kotlinastgenTestOut") { outputDir =>
          val config = Config().withInputPath(inputDir.toString).withOutputPath(outputDir.toString)
          val result = new KotlinAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(outputDir / "demo" / "Sample.kt.json")
            .map(_.toString)

          val document = KotlinAstJsonParser.parseFile(outputDir / "demo" / "Sample.kt.json")
          document.relativeName shouldBe "demo/Sample.kt"
          document.ast.kind shouldBe "source_file"
          document.ast.descendants.map(_.kind) should contain allOf ("class_declaration", "function_declaration")
        }
      }
    }

    "respect ignored files regex configuration" in {
      FileUtil.usingTemporaryDirectory("kotlinastgenTestInput") { inputDir =>
        writeFile(inputDir / "Keep.kt", "class Keep\n")
        writeFile(inputDir / "Skip.kt", "class Skip\n")

        FileUtil.usingTemporaryDirectory("kotlinastgenTestOut") { outputDir =>
          val config = Config()
            .withInputPath(inputDir.toString)
            .withOutputPath(outputDir.toString)
            .withIgnoredFilesRegex(".*Skip.*")
          val result = new KotlinAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(outputDir / "Keep.kt.json").map(_.toString)
        }
      }
    }
  }

  private def writeFile(path: Path, content: String): Unit = {
    path.createWithParentsIfNotExists(createParents = true)
    Files.writeString(path, content)
  }
}
