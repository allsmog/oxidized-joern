package io.joern.rust2cpg.astgen

import io.joern.rust2cpg.Config
import io.joern.x2cpg.astgen.AstGenRunner
import io.joern.x2cpg.astgen.AstGenRunner.AstGenProgramMetaData
import io.shiftleft.semanticcpg.utils.ExternalCommand
import org.slf4j.LoggerFactory

import java.nio.file.{Path, Paths}
import scala.util.Try

object RustAstGenRunner {
  private object astGenMetaData extends AstGenProgramMetaData(name = "rust_ast_gen", configPrefix = "rust2cpg")
}

class RustAstGenRunner(config: Config) extends AstGenRunner(RustAstGenRunner.astGenMetaData, config) {

  private val logger = LoggerFactory.getLogger(getClass)

  override def skippedFiles(in: Path, astGenOut: List[String]): List[String] = {
    astGenOut.flatMap {
      case line if line.startsWith("Converted AST for ") =>
        logger.debug(s"\t+ $line")
        None
      case line =>
        skippedFileName(line).map { fileName =>
          logger.warn(s"\t- failed to parse '$fileName': ${line.stripPrefix(fileName).strip()}")
          relativeSkippedFile(in, fileName)
        }
    }
  }

  override def runAstGenNative(in: String, out: Path, exclude: String, include: String): Try[Seq[String]] = {
    val baseArgs    = Seq(astGenCommand, "-i", in, "-o", out.toString)
    val excludeArgs = Option(exclude).filter(_.nonEmpty).toSeq.flatMap(regex => Seq("--exclude-regex", regex))
    val args = {
      val withoutSysroot = baseArgs ++ excludeArgs
      if (config.noSysRoot) withoutSysroot :+ "--no-sysroot" else withoutSysroot
    }
    ExternalCommand.run(args).toTry
  }

  private def skippedFileName(line: String): Option[String] = {
    val trimmed = line.trim
    if (trimmed.isEmpty) {
      None
    } else if (trimmed.startsWith("Skipped:")) {
      Option(trimmed.stripPrefix("Skipped:").trim).filter(_.nonEmpty)
    } else {
      Option(trimmed.takeWhile(!_.isWhitespace)).filter(_.nonEmpty)
    }
  }

  private def relativeSkippedFile(in: Path, fileName: String): String = {
    val path      = Paths.get(fileName)
    val inputPath = Try(in.toRealPath()).getOrElse(in.toAbsolutePath.normalize())
    val skippedPath =
      if (path.isAbsolute) Try(path.toRealPath()).getOrElse(path.normalize())
      else path
    if (skippedPath.isAbsolute && skippedPath.startsWith(inputPath)) {
      inputPath.relativize(skippedPath).toString
    } else {
      fileName
    }
  }
}
