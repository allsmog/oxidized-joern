package io.joern.pysrc2cpg

import io.joern.pysrc2cpg.parser.{PyAstGenCodeToCpg, PyAstGenRunner, PyAstJsonParser}
import io.joern.x2cpg.ValidationMode
import io.joern.x2cpg.frontendspecific.pysrc2cpg.Constants
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder}
import io.shiftleft.codepropertygraph.generated.Languages
import flatgraph.DiffGraphApplier
import io.shiftleft.codepropertygraph.generated.DiffGraphBuilder
import io.shiftleft.semanticcpg.utils.FileUtil
import org.slf4j.LoggerFactory

import java.nio.file.{Path, Paths}

object Py2Cpg {
  private val logger = LoggerFactory.getLogger(getClass)

  case class InputPair(content: String, relFileName: String)
  type InputProvider = () => InputPair
}

/** Entry point for general cpg generation from python code.
  *
  * @param inputProviders
  *   Set of functions which provide InputPairs. The functions must be safe to call from different threads.
  * @param outputCpg
  *   Empty target cpg which will be populated.
  * @param inputPath
  *   The project root.
  * @param requirementsTxt
  *   The configured name of the requirements txt file.
  * @param schemaValidationMode
  *   The boolean switch for enabling or disabling early schema checking during AST creation.
  */
class Py2Cpg(
  inputProviders: Iterable[Py2Cpg.InputProvider],
  outputCpg: Cpg,
  config: Py2CpgOnFileSystemConfig,
  inputFiles: Iterable[Path] = Nil
) {
  import Py2Cpg.logger

  private val diffGraph   = Cpg.newDiffGraphBuilder
  private val nodeBuilder = new NodeBuilder(diffGraph)
  private val edgeBuilder = new EdgeBuilder(diffGraph)

  def buildCpg(): Unit = {
    nodeBuilder
      .metaNode(Languages.PYTHONSRC, version = classOf[Py2Cpg].getPackage.getImplementationVersion)
      .root(config.inputPath + java.io.File.separator)
    val globalNamespaceBlock =
      nodeBuilder.namespaceBlockNode(Constants.GLOBAL_NAMESPACE, Constants.GLOBAL_NAMESPACE, "N/A")
    nodeBuilder.typeNode(Constants.ANY, Constants.ANY)
    val anyTypeDecl =
      nodeBuilder.typeDeclNode(Constants.ANY, Constants.ANY, "N/A", Nil, LineAndColumn(1, 1, 1, 1, 1, 1))
    edgeBuilder.astEdge(anyTypeDecl, globalNamespaceBlock, 0)
    DiffGraphApplier.applyDiff(outputCpg.graph, diffGraph)
    config.parserBackend match {
      case PythonParserBackend.JavaCc =>
        new CodeToCpg(outputCpg, inputProviders, config.schemaValidation, !config.disableFileContent).createAndApply()
      case PythonParserBackend.Oxidized =>
        buildCpgWithPyAstGen()
    }
    new ConfigFileCreationPass(outputCpg, config.requirementsTxt, config).createAndApply()
    new DependenciesFromRequirementsTxtPass(outputCpg).createAndApply()
  }

  private def buildCpgWithPyAstGen(): Unit = {
    FileUtil.usingTemporaryDirectory("pysrc2cpgOut") { tmpDir =>
      val astGenResult = new PyAstGenRunner(config).execute(tmpDir)

      val allowedFiles = inputFiles.map(normalizedPath).toSet
      val parsedFiles =
        if (allowedFiles.isEmpty) {
          astGenResult.parsedFiles
        } else {
          astGenResult.parsedFiles.filter { jsonFile =>
            PyAstJsonParser
              .sourcePath(Paths.get(jsonFile))
              .exists(path => allowedFiles.contains(normalizedPath(path)))
          }
        }

      new PyAstGenCodeToCpg(
        outputCpg,
        parsedFiles,
        config.schemaValidation,
        !config.disableFileContent,
        Paths.get(config.inputPath)
      ).createAndApply()

      val fallbackInputProviders = inputProvidersFor(astGenResult.skippedFiles.toSet).toSeq
      if (fallbackInputProviders.nonEmpty) {
        logger.info(s"Falling back to JavaCC parser for ${fallbackInputProviders.size} Python file(s)")
        new CodeToCpg(outputCpg, fallbackInputProviders, config.schemaValidation, !config.disableFileContent)
          .createAndApply()
      }
    }
  }

  private def normalizedPath(path: Path): Path = path.toAbsolutePath.normalize()

  private def inputProvidersFor(relFileNames: Set[String]): Iterable[Py2Cpg.InputProvider] = {
    inputProviders.flatMap { inputProvider =>
      val inputPair = inputProvider()
      Option.when(relFileNames.contains(inputPair.relFileName))(() => inputPair)
    }
  }
}
