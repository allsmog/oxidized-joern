package io.joern.console

import io.shiftleft.codepropertygraph.generated.Languages
import io.joern.console.cpgcreation.{LlvmCpgGenerator, guessLanguage}
import io.shiftleft.semanticcpg.utils.FileUtil.*
import io.shiftleft.semanticcpg.utils.FileUtil
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path, Paths}

class LanguageHelperTests extends AnyWordSpec with Matchers {

  "LanguageHelper.guessLanguage" should {

    "guess `Java` for .jars/wars/ears" in {
      guessLanguage("foo.jar") shouldBe Some(Languages.JAVA)
      guessLanguage("foo.war") shouldBe Some(Languages.JAVA)
      guessLanguage("foo.ear") shouldBe Some(Languages.JAVA)
    }

    "guess `C#` for .csproj" in {
      guessLanguage("foo.csproj") shouldBe Some(Languages.CSHARPSRC)
    }

    "guess `Swift` for a directory containing `.swift`" in {
      FileUtil.usingTemporaryDirectory("oculartests") { tmpDir =>
        val subdir = Files.createDirectory(tmpDir / "subdir")
        (subdir / "main.swift").createWithParentsIfNotExists()
        guessLanguage(tmpDir.toString) shouldBe Some(Languages.SWIFTSRC)
      }
    }

    "guess `C` for a directory containing .ll (LLVM) file" in {
      FileUtil.usingTemporaryDirectory("oculartests") { tmpDir =>
        val subdir = Files.createDirectory(tmpDir / "subdir")
        (subdir / "foobar.ll").createWithParentsIfNotExists()
        guessLanguage(tmpDir.toString) shouldBe Some(Languages.LLVM)
      }
    }

    "guess the language with the largest number of files" in {
      FileUtil.usingTemporaryDirectory("oculartests") { tmpDir =>
        val subdir = Files.createDirectory(tmpDir / "subdir")
        (subdir / "source.kt").createWithParentsIfNotExists()
        (subdir / "one.php").createWithParentsIfNotExists()
        (subdir / "two.php").createWithParentsIfNotExists()
        guessLanguage(tmpDir.toString) shouldBe Some(Languages.PHP)
      }
    }

    "guess nothing for cpg-rs-covered languages (go/js/py/rb/rs/c/java-src)" in {
      FileUtil.usingTemporaryDirectory("oculartests") { tmpDir =>
        val subdir = Files.createDirectory(tmpDir / "subdir")
        (subdir / "source.c").createWithParentsIfNotExists()
        (subdir / "source.java").createWithParentsIfNotExists()
        (subdir / "source.py").createWithParentsIfNotExists()
        (subdir / "source.js").createWithParentsIfNotExists()
        (subdir / "package.json").createWithParentsIfNotExists()
        (subdir / "main.go").createWithParentsIfNotExists()
        (subdir / "main.rs").createWithParentsIfNotExists()
        guessLanguage(tmpDir.toString) shouldBe None
      }
    }

    "not find anything for an empty directory" in {
      FileUtil.usingTemporaryDirectory("oculartests") { tmpDir =>
        guessLanguage(tmpDir.toString) shouldBe None
      }
    }

  }

  "LanguageHelper.cpgGeneratorForLanguage" should {

    "select LLVM frontend for directories containing ll files" in {
      val frontend =
        io.joern.console.cpgcreation.cpgGeneratorForLanguage(Languages.LLVM, FrontendConfig(), Paths.get("."), Nil)
      frontend.get.isInstanceOf[LlvmCpgGenerator] shouldBe true
    }
  }

}
