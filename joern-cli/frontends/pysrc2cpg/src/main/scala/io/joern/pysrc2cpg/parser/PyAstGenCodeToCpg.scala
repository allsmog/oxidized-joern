package io.joern.pysrc2cpg.parser

import io.joern.pysrc2cpg.PythonVersion.PythonV2AndV3
import io.joern.pysrc2cpg.{NodeBuilder, NodeToCode, PythonAstVisitor}
import io.joern.x2cpg.ValidationMode
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder}
import io.shiftleft.passes.ForkJoinParallelCpgPass
import org.slf4j.LoggerFactory

import java.nio.file.{Path, Paths}
import scala.util.Try

class PyAstGenCodeToCpg(
  cpg: Cpg,
  jsonFiles: Iterable[String],
  schemaValidationMode: ValidationMode,
  enableFileContent: Boolean,
  inputRoot: Path
) extends ForkJoinParallelCpgPass[String](cpg) {
  import PyAstGenCodeToCpg.logger

  override def generateParts(): Array[String] = jsonFiles.toArray

  override def runOnPart(diffGraph: DiffGraphBuilder, jsonFile: String): Unit = {
    val jsonPath = Paths.get(jsonFile)
    try {
      val parsed     = PyAstJsonParser.parseFile(jsonPath, inputRoot)
      val nodeToCode = new NodeToCode(parsed.source)
      val astVisitor = new PythonAstVisitor(parsed.relFileName, nodeToCode, PythonV2AndV3, enableFileContent)(
        schemaValidationMode
      )
      astVisitor.convert(parsed.module)
      diffGraph.absorb(astVisitor.createAst())
    } catch {
      case exception: Throwable =>
        handleParsingError(jsonPath, exception, diffGraph)
    }
  }

  private def handleParsingError(jsonPath: Path, exception: Throwable, diffGraph: DiffGraphBuilder): Unit = {
    val relFileName = PyAstJsonParser
      .sourcePath(jsonPath)
      .flatMap(path =>
        Try(inputRoot.toAbsolutePath.normalize().relativize(path.toAbsolutePath.normalize()).toString).toOption
      )
      .getOrElse(jsonPath.getFileName.toString.stripSuffix(".json"))
    new NodeBuilder(diffGraph).fileNode(relFileName, None)
    logger.warn(s"Failed to convert pyastgen JSON file $jsonPath", exception)
  }
}

object PyAstGenCodeToCpg {
  private val logger = LoggerFactory.getLogger(getClass)
}
