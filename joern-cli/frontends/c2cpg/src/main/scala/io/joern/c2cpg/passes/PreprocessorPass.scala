package io.joern.c2cpg.passes

import io.joern.c2cpg.C2Cpg.DefaultIgnoredFolders
import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.CdtParser
import io.joern.c2cpg.parser.FileDefaults
import io.joern.c2cpg.parser.HeaderFileFinder
import io.joern.c2cpg.parser.JSONCompilationDatabaseParser
import io.joern.c2cpg.parser.JSONCompilationDatabaseParser.CompilationDatabase
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.parser.TranslationUnitParseInput
import io.joern.x2cpg.SourceFiles
import org.eclipse.cdt.core.dom.ast.{
  IASTPreprocessorIfStatement,
  IASTPreprocessorIfdefStatement,
  IASTPreprocessorStatement
}
import org.slf4j.LoggerFactory

import java.nio.file.Paths

class PreprocessorPass(config: Config) {

  private val logger = LoggerFactory.getLogger(classOf[PreprocessorPass])

  private val compilationDatabase: Option[CompilationDatabase] =
    config.compilationDatabaseFilename.flatMap(JSONCompilationDatabaseParser.parse)

  private val headerFileFinder = new HeaderFileFinder(config)
  private val parser           = new CdtParser(config, headerFileFinder, compilationDatabase)
  private val accumulator      = AstCreationPass.Accumulator()

  def run(): Iterable[String] = {
    if (config.parserBackend != ParserBackend.Cdt) {
      throw new UnsupportedOperationException(
        "The oxidized c2cpg parser backend does not support --print-ifdef-only yet. Use --parser-backend cdt."
      )
    }
    val sourceFiles = if (config.compilationDatabaseFilename.isEmpty) {
      sourceFilesFromDirectory()
    } else {
      sourceFilesFromCompilationDatabase(config.compilationDatabaseFilename.get)
    }
    sourceFiles
      .flatMap { file =>
        val path = Paths.get(file).toAbsolutePath
        CdtParser.languageMappingForSourceFile(path, Map.empty, config)
      }
      .flatMap(runOnPart)
  }

  private def sourceFilesFromDirectory(): Iterable[String] = {
    SourceFiles
      .determine(
        config.inputPath,
        FileDefaults.SourceFileExtensions,
        ignoredDefaultRegex = Option(DefaultIgnoredFolders),
        ignoredFilesRegex = Option(config.ignoredFilesRegex),
        ignoredFilesPath = Option(config.ignoredFiles)
      )
  }

  private def sourceFilesFromCompilationDatabase(compilationDatabaseFile: String): Iterable[String] = {
    compilationDatabase match {
      case Some(db) =>
        SourceFiles
          .filterFiles(
            db.commands.map(_.compiledFile()).toList,
            config.inputPath,
            ignoredDefaultRegex = Option(DefaultIgnoredFolders),
            ignoredFilesRegex = Option(config.ignoredFilesRegex),
            ignoredFilesPath = Option(config.ignoredFiles)
          )
      case None =>
        logger.warn(s"'$compilationDatabaseFile' contains no source files. CPG will be empty.")
        Iterable.empty
    }
  }

  private def preprocessorStatement2String(stmt: IASTPreprocessorStatement): Option[String] = {
    stmt match {
      case s: IASTPreprocessorIfStatement =>
        Option(s"${s.getCondition.mkString}${if (s.taken()) "=true" else ""}")
      case s: IASTPreprocessorIfdefStatement =>
        Option(s"${s.getCondition.mkString}${if (s.taken()) "=true" else ""}")
      case _ => None
    }
  }

  private def runOnPart(input: TranslationUnitParseInput): Iterable[String] = {
    parser.preprocessorStatements(input.path, input.language, accumulator).flatMap(preprocessorStatement2String).toSet
  }

}
