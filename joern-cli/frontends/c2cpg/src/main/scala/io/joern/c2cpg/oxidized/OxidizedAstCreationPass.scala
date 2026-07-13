package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.joern.x2cpg.SourceFiles
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder}
import io.shiftleft.passes.CpgPass

import scala.collection.mutable

final class OxidizedAstCreationPass(cpg: Cpg, config: Config) extends CpgPass(cpg) {

  private val usedTypes: mutable.Set[String] = mutable.Set.empty

  def typesSeen(): Set[String] = usedTypes.toSet

  override def run(dstGraph: DiffGraphBuilder): Unit = {
    OxidizedAstGenRunner.run(config).foreach { document =>
      val filename = SourceFiles.toRelativePath(document.path, config.inputPath)
      val creator  = new OxidizedAstCreator(filename, document, config)
      dstGraph.absorb(creator.createAst())
      usedTypes.addAll(creator.typesSeen())
    }
  }

}
