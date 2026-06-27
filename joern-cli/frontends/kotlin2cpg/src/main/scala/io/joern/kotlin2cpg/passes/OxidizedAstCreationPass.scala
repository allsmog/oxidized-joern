package io.joern.kotlin2cpg.passes

import io.joern.kotlin2cpg.Config
import io.joern.kotlin2cpg.oxidized.OxidizedAstCreator
import io.joern.kotlin2cpg.parser.KotlinAstJsonParser
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder}
import io.shiftleft.passes.CpgPass

import java.nio.file.Paths
import scala.collection.mutable

class OxidizedAstCreationPass(cpg: Cpg, jsonFiles: List[String], config: Config) extends CpgPass(cpg) {

  private val usedTypeNames: mutable.Set[String] = mutable.Set.empty

  def usedTypes(): Set[String] = usedTypeNames.toSet

  override def run(dstGraph: DiffGraphBuilder): Unit = {
    jsonFiles.foreach { jsonFile =>
      val document = KotlinAstJsonParser.parseFile(Paths.get(jsonFile))
      val creator  = new OxidizedAstCreator(document, config)
      dstGraph.absorb(creator.createAst())
      usedTypeNames.addAll(creator.usedTypes())
    }
  }
}
