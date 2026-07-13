package io.joern.javasrc2cpg.oxidized

import io.joern.javasrc2cpg.Config
import io.joern.javasrc2cpg.parser.{JavaAstGenRunner, JavaAstJsonParser}
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder}
import io.shiftleft.passes.CpgPass
import io.shiftleft.semanticcpg.utils.FileUtil

import scala.collection.mutable

final class OxidizedAstCreationPass(cpg: Cpg, config: Config) extends CpgPass(cpg) {

  private val seenTypes: mutable.Set[String] = mutable.Set.empty

  def usedTypes(): Set[String] = seenTypes.toSet

  override def run(dstGraph: DiffGraphBuilder): Unit = {
    FileUtil.usingTemporaryDirectory("javaastgenOut") { tmpDir =>
      val result = new JavaAstGenRunner(config).execute(tmpDir)
      result.parsedFiles.foreach { jsonFile =>
        val document = JavaAstJsonParser.parseFile(java.nio.file.Paths.get(jsonFile))
        val creator  = new OxidizedAstCreator(document, config)
        dstGraph.absorb(creator.createAst())
        seenTypes.addAll(creator.usedTypes())
      }
    }
  }
}
