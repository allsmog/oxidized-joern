package io.joern.console

import io.shiftleft.semanticcpg.utils.FileUtil.*
import io.shiftleft.codepropertygraph.generated.Languages
import io.shiftleft.semanticcpg.utils.FileUtil

import java.nio.file.{Path, Paths, Files}
import scala.collection.mutable
import scala.util.Try

package object cpgcreation {

  /** For a given language, return CPG generator script Note, this doesn't check if the generator is available, that is
    * done in the ImportCode class.
    */
  def cpgGeneratorForLanguage(
    language: String,
    config: FrontendConfig,
    rootPath: Path,
    args: List[String]
  ): Option[CpgGenerator] = {
    lazy val conf = config.withArgs(args)
    // Languages with cpg-rs (Rust) parity — C/C++, Go, Java source, JS/TS,
    // Python source, Ruby, Rust — are handled by cpg-rs; their Scala
    // frontends and generators (including the external js2cpg/py shims)
    // have been removed.
    language match {
      case Languages.CSHARP    => Some(CSharpCpgGenerator(conf, rootPath))
      case Languages.CSHARPSRC => Some(CSharpSrcCpgGenerator(conf, rootPath))
      case Languages.LLVM      => Some(LlvmCpgGenerator(conf, rootPath))
      case Languages.JAVA      => Some(JavaCpgGenerator(conf, rootPath))
      case Languages.PHP      => Some(PhpCpgGenerator(conf, rootPath))
      case Languages.GHIDRA   => Some(GhidraCpgGenerator(conf, rootPath))
      case Languages.KOTLIN   => Some(KotlinCpgGenerator(conf, rootPath))
      case Languages.SWIFTSRC => Some(SwiftSrcCpgGenerator(conf, rootPath))
      case Languages.ABAP     => Some(AbapSrcCpgGenerator(conf, rootPath))
      case _                  => None
    }
  }

  /** Heuristically determines language by inspecting file/dir at path.
    */
  def guessLanguage(path: String): Option[String] = {
    val file = Paths.get(path)
    if (Files.isDirectory(file)) {
      guessMajorityLanguageInDir(file)
    } else {
      guessLanguageForRegularFile(file)
    }
  }

  /** Guess the main language for an entire directory (e.g. a whole project), based on a group count of all individual
    * files. Rationale: many projects contain files from different languages, but most often one language is standing
    * out in numbers.
    */
  private def guessMajorityLanguageInDir(directory: Path): Option[String] = {
    assert(Files.isDirectory(directory), s"$directory must be a directory, but wasn't")
    val groupCount = mutable.Map.empty[String, Int].withDefaultValue(0)

    for {
      file <- directory.walk().filterNot(_ == directory)
      if Files.isRegularFile(file)
      guessedLanguage <- guessLanguageForRegularFile(file)
    } {
      val oldValue = groupCount(guessedLanguage)
      groupCount.update(guessedLanguage, oldValue + 1)
    }

    groupCount.toSeq.sortBy(_._2).lastOption.map(_._1)
  }

  private def isJavaBinary(filename: String): Boolean =
    Seq(".jar", ".war", ".ear", ".apk").exists(filename.endsWith)

  private def isCsharpFile(filename: String): Boolean =
    Seq(".csproj", ".cs").exists(filename.endsWith)

  private def isLlvmFile(filename: String): Boolean =
    Seq(".bc", ".ll").exists(filename.endsWith)

  // Extensions of cpg-rs-covered languages (go/js/ts/py/rb/rs/c/cpp/java-src)
  // deliberately guess to None here: no Scala generator exists for them.
  private def guessLanguageForRegularFile(file: Path): Option[String] = {
    file.fileName.toLowerCase match {
      case fileName if isJavaBinary(fileName)      => Some(Languages.JAVA)
      case fileName if isCsharpFile(fileName)      => Some(Languages.CSHARPSRC)
      case fileName if fileName.endsWith(".class") => Some(Languages.JAVA)
      case fileName if fileName.endsWith(".kt")    => Some(Languages.KOTLIN)
      case fileName if fileName.endsWith(".php")   => Some(Languages.PHP)
      case fileName if fileName.endsWith(".swift") => Some(Languages.SWIFTSRC)
      case fileName if isLlvmFile(fileName)        => Some(Languages.LLVM)
      case fileName if fileName.endsWith(".abap")  => Some(Languages.ABAP)
      case _                                       => None
    }
  }

  def withFileInTmpFile(inputPath: String)(f: Path => Try[String]): Try[String] = {
    FileUtil.usingTemporaryDirectory("cpgcreation") { dir =>
      Paths.get(inputPath).copyToDirectory(dir)
      f(dir)
    }
  }

}
