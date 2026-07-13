package io.joern.pysrc2cpg.parser

import io.joern.pysrc2cpg.Py2CpgOnFileSystemConfig
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path}

class PyAstGenRunnerTests extends AnyWordSpec with Matchers {

  private def writeFile(file: Path, content: String): Unit = {
    file.createWithParentsIfNotExists(createParents = true)
    Files.writeString(file, content)
  }

  "PyAstGenRunner" should {

    "parse Python source files into JSON documents" in {
      FileUtil.usingTemporaryDirectory("pyastgenTestInput") { inputDir =>
        writeFile(inputDir / "main.py", "def main():\n    return 1\n")
        writeFile(inputDir / "pkg" / "service.py", "class Service:\n    pass\n")
        writeFile(inputDir / "notes.txt", "ignored")

        FileUtil.usingTemporaryDirectory("pyastgenTestOut") { outputDir =>
          val config = Py2CpgOnFileSystemConfig().withInputPath(inputDir.toString).withOutputPath(outputDir.toString)
          val result = new PyAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(
            outputDir / "main.py.json",
            outputDir / "pkg" / "service.py.json"
          ).map(_.toString)
          Files.readString(outputDir / "main.py.json") should include("oxidized-pyastgen")
        }
      }
    }

    "respect ignored files regex configuration" in {
      FileUtil.usingTemporaryDirectory("pyastgenTestInput") { inputDir =>
        writeFile(inputDir / "keep.py", "x = 1\n")
        writeFile(inputDir / "skip.py", "x = 2\n")

        FileUtil.usingTemporaryDirectory("pyastgenTestOut") { outputDir =>
          val config = Py2CpgOnFileSystemConfig()
            .withInputPath(inputDir.toString)
            .withOutputPath(outputDir.toString)
            .withIgnoredFilesRegex(".*skip.*")
          val result = new PyAstGenRunner(config).execute(outputDir)
          result.parsedFiles.sorted should contain theSameElementsAs Seq(outputDir / "keep.py.json").map(_.toString)
        }
      }
    }
  }
}
