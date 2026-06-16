package io.joern.c2cpg.parser

import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.CdtParser.HeaderFileParserLanguage
import io.joern.c2cpg.parser.JSONCompilationDatabaseParser.CompilationDatabase
import io.joern.c2cpg.passes.AstCreationPass
import org.eclipse.cdt.core.dom.ast.IASTTranslationUnit

import java.nio.file.Path

enum TranslationUnitLanguage {
  case C, Cpp
}

case class TranslationUnitParseInput(path: Path, language: TranslationUnitLanguage)

case class ParsedTranslationUnit(path: Path, cdtAst: IASTTranslationUnit)

trait TranslationUnitProvider {

  def languageMappingForSourceFile(
    path: Path,
    headerIncludes: Map[String, HeaderFileParserLanguage]
  ): Seq[TranslationUnitParseInput]

  def parse(input: TranslationUnitParseInput, accumulator: AstCreationPass.Accumulator): Option[ParsedTranslationUnit]

}

object TranslationUnitProvider {

  def forConfig(
    config: Config,
    headerFileFinder: HeaderFileFinder,
    compilationDatabase: Option[CompilationDatabase]
  ): TranslationUnitProvider = {
    config.parserBackend match {
      case ParserBackend.Cdt      => CdtTranslationUnitProvider(config, headerFileFinder, compilationDatabase)
      case ParserBackend.Oxidized => OxidizedTranslationUnitProvider()
    }
  }

}

final case class CdtTranslationUnitProvider(
  config: Config,
  headerFileFinder: HeaderFileFinder,
  compilationDatabase: Option[CompilationDatabase]
) extends TranslationUnitProvider {

  private val parser = new CdtParser(config, headerFileFinder, compilationDatabase)

  override def languageMappingForSourceFile(
    path: Path,
    headerIncludes: Map[String, HeaderFileParserLanguage]
  ): Seq[TranslationUnitParseInput] = {
    CdtParser.languageMappingForSourceFile(path, headerIncludes, config)
  }

  override def parse(
    input: TranslationUnitParseInput,
    accumulator: AstCreationPass.Accumulator
  ): Option[ParsedTranslationUnit] = {
    parser.parse(input.path, input.language, accumulator).map { translationUnit =>
      ParsedTranslationUnit(input.path, translationUnit)
    }
  }

}

final case class OxidizedTranslationUnitProvider() extends TranslationUnitProvider {

  override def languageMappingForSourceFile(
    path: Path,
    headerIncludes: Map[String, HeaderFileParserLanguage]
  ): Seq[TranslationUnitParseInput] = {
    throw unsupported
  }

  override def parse(
    input: TranslationUnitParseInput,
    accumulator: AstCreationPass.Accumulator
  ): Option[ParsedTranslationUnit] = {
    throw unsupported
  }

  private def unsupported: UnsupportedOperationException = {
    new UnsupportedOperationException(
      "The oxidized c2cpg parser backend does not expose Eclipse CDT translation units. " +
        "Use --parser-backend oxidized through C2Cpg.createCpg to generate CPGs from Rust JSON, " +
        "or use --parser-backend cdt for CDT translation units."
    )
  }

}
