package io.joern.javasrc2cpg.parser

import io.joern.javasrc2cpg.Config
import io.joern.x2cpg.astgen.AstGenRunner
import io.joern.x2cpg.astgen.AstGenRunner.AstGenProgramMetaData
import io.shiftleft.semanticcpg.utils.ExternalCommand
import org.slf4j.LoggerFactory

import java.nio.file.{Path, Paths}
import scala.util.Try

object JavaAstGenRunner {

  private object astGenMetaData
      extends AstGenProgramMetaData(
        name = "javaastgen",
        configPrefix = "javasrc2cpg",
        binEnvVar = Option("JAVAASTGEN_BIN"),
        versionFlag = "-version"
      )
}

class JavaAstGenRunner(config: Config) extends AstGenRunner(JavaAstGenRunner.astGenMetaData, config) {

  private val logger = LoggerFactory.getLogger(getClass)

  override protected def runAstGenNative(in: String, out: Path, exclude: String, include: String): Try[Seq[String]] = {
    val excludeArgs = Option(exclude).filter(_.nonEmpty).toSeq.flatMap(regex => Seq("-exclude", regex))
    val args        = Seq(astGenCommand, "-out", out.toString) ++ excludeArgs ++ Seq(in)
    ExternalCommand.run(args).toTry
  }

  override protected def skippedFiles(in: Path, astGenOut: List[String]): List[String] = {
    astGenOut.flatMap {
      case line if line.startsWith("Converted AST for ") =>
        logger.debug(s"\t+ $line")
        None
      case line =>
        val fileName = line.takeWhile(!_.isWhitespace)
        logger.warn(s"\t- failed to parse '$fileName': ${line.stripPrefix(fileName).strip()}")
        Option(relativeSkippedFile(in, fileName))
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
