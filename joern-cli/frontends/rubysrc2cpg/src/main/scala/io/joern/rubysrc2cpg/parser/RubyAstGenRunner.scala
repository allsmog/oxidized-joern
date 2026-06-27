package io.joern.rubysrc2cpg.parser

import io.joern.rubysrc2cpg.Config
import io.joern.rubysrc2cpg.parser.RubyAstGenRunner.{JRubyEnvironment, astGenMetaData}
import io.joern.x2cpg.SourceFiles
import io.joern.x2cpg.astgen.AstGenRunner
import io.joern.x2cpg.astgen.AstGenRunner.{AstGenProgramMetaData, AstGenRunnerResult, DefaultAstGenRunnerResult}
import io.shiftleft.semanticcpg.utils.ExternalCommand
import org.slf4j.LoggerFactory

import java.nio.file.{Path, Paths}
import scala.collection.mutable
import scala.util.{Failure, Success, Try}

class RubyAstGenRunner(config: Config, sharedJRubyEnv: Option[JRubyEnvironment] = None)
    extends AstGenRunner(RubyAstGenRunner.astGenMetaData, config)
    with AutoCloseable {

  private val logger = LoggerFactory.getLogger(getClass)

  override def close(): Unit = ()

  override def fileFilter(file: String, out: Path): Boolean = {
    file.stripSuffix(".json").replace(out.toString, config.inputPath) match {
      case filePath if isIgnoredByUserConfig(filePath)   => false
      case filePath if isIgnoredByDefaultRegex(filePath) => false
      case _                                             => true
    }
  }

  private def isIgnoredByDefaultRegex(filePath: String): Boolean = {
    config.defaultIgnoredFilesRegex.exists(_.matches(filePath))
  }

  override def skippedFiles(in: Path, astGenOut: List[String]): List[String] = {
    val diagnosticMap = mutable.LinkedHashMap.empty[String, Seq[String]]

    def addReason(reason: String, lastFile: Option[String] = None): Unit = {
      lastFile.orElse(diagnosticMap.lastOption.map(_._1)).foreach { key =>
        diagnosticMap.updateWith(key) {
          case Some(x) => Option(x :+ reason)
          case None    => Option(reason :: Nil)
        }
      }
    }

    astGenOut.map(_.strip()).foreach {
      case s"[WARN] $reason - $fileName"  => addReason(reason, Option(fileName))
      case s"[ERR] '$fileName' - $reason" => addReason(reason, Option(fileName))
      case s"[ERR] Failed to parse $fileName: $reason" =>
        addReason(s"Failed to parse: $reason", Option(fileName))
      case s"[INFO] Processed: $fileName -> $_" => diagnosticMap.put(fileName, Nil)
      case s"[INFO] Excluding: $fileName"       => addReason("Skipped", Option(fileName))
      case _                                    => // ignore
    }

    diagnosticMap.flatMap {
      case (filename, Nil) =>
        logger.debug(s"Successfully parsed '$filename'")
        None
      case (filename, "Skipped" :: Nil) =>
        logger.debug(s"Skipped '$filename' due to file filter")
        Option(filename)
      case (filename, diagnostics) =>
        logger.warn(
          s"Parsed '$filename' with the following diagnostics:\n${diagnostics.map(x => s" - $x").mkString("\n")}"
        )
        Option(filename)
    }.toList
  }

  override def runAstGenNative(in: String, out: Path, exclude: String, include: String): Try[Seq[String]] = {
    val excludeCommand = if (exclude.isEmpty) Seq.empty else Seq("-e", exclude)
    ExternalCommand.run(Seq(astGenCommand) ++ excludeCommand ++ Seq(in, out.toString)).toTry
  }

  override def execute(out: Path): AstGenRunnerResult = {
    execute(out, config)
  }

  def execute(out: Path, specifiedConfig: Config): AstGenRunnerResult = {
    val in = Paths.get(specifiedConfig.inputPath)
    logger.info(s"Running ${astGenMetaData.name} on '${specifiedConfig.inputPath}'")

    val combineIgnoreRegex =
      if (
        specifiedConfig.ignoredFilesRegex
          .toString()
          .isEmpty && specifiedConfig.defaultIgnoredFilesRegex.toString.nonEmpty
      ) {
        specifiedConfig.defaultIgnoredFilesRegex.mkString("|")
      } else if (
        specifiedConfig.ignoredFilesRegex
          .toString()
          .nonEmpty && specifiedConfig.defaultIgnoredFilesRegex.toString.isEmpty
      ) {
        specifiedConfig.ignoredFilesRegex.toString()
      } else if (
        specifiedConfig.ignoredFilesRegex.toString().nonEmpty && specifiedConfig.defaultIgnoredFilesRegex
          .toString()
          .nonEmpty
      ) {
        s"((${specifiedConfig.ignoredFilesRegex.toString()})|(${specifiedConfig.defaultIgnoredFilesRegex.mkString("|")}))"
      } else {
        ""
      }

    runAstGenNative(specifiedConfig.inputPath, out, combineIgnoreRegex, "") match {
      case Success(result) =>
        val srcFiles = SourceFiles.determine(
          out.toString(),
          Set(".json"),
          ignoredDefaultRegex = Option(specifiedConfig.defaultIgnoredFilesRegex),
          ignoredFilesRegex = Option(specifiedConfig.ignoredFilesRegex),
          ignoredFilesPath = Option(specifiedConfig.ignoredFiles)
        )
        val parsed  = filterFiles(srcFiles, out)
        val skipped = skippedFiles(in, result.toList)
        DefaultAstGenRunnerResult(parsed, skipped)
      case Failure(f) =>
        logger.error(s"\t- running ${astGenMetaData.name} failed!", f)
        DefaultAstGenRunnerResult()
    }
  }

}

object RubyAstGenRunner {

  private object astGenMetaData
      extends AstGenProgramMetaData(
        name = "rubyastgen",
        configPrefix = "rubysrc2cpg",
        binEnvVar = Option("RUBYASTGEN_BIN"),
        versionFlag = "--version",
        versionConfigKey = Option("rubysrc2cpg.rubyastgen_version")
      )

  final class JRubyEnvironment extends AutoCloseable {
    override def close(): Unit = ()
  }

  object JRubyEnvironment {
    def apply(): JRubyEnvironment = new JRubyEnvironment()
  }

}
