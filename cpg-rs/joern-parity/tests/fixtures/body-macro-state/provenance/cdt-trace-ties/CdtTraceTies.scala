import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.{CdtParser,HeaderFileFinder}
import io.joern.c2cpg.passes.AstCreationPass
import org.eclipse.cdt.core.dom.ast.{IASTNode,IASTMacroExpansionLocation}
import org.eclipse.cdt.core.dom.ast.gnu.c.GCCLanguage
import java.nio.file.Paths

def esc(s: String): String = s.replace("\n", "\\n")
object CdtTraceTies { def main(args: Array[String]): Unit = {
 val inputPath = args(0)
 val extra = os.Path("/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-aggregates/.local/ninth-body-state-review/input")
 val paths = os.list(os.Path(inputPath)).filter(os.isDir(_)).toList ++ List("branch_condition_entry_state","header_after_body_undef","recovery_after_redefinition").map(extra / _)
 paths.foreach { dir =>
  val config = Config().withInputPath(dir.toString)
  val parser = new CdtParser(config,new HeaderFileFinder(config),None)
  val tu=parser.parse(Paths.get((dir / "main.c").toString),GCCLanguage.getDefault,AstCreationPass.Accumulator()).get
  println("CASE|"+dir.last)
  tu.getNodeLocations.toList.zipWithIndex.collect { case (e: IASTMacroExpansionLocation,i) => (e,i) }.foreach { case(e,i) =>
   println("RAWPAIR|"+i+"|"+e.asFileLocation.getNodeOffset+"|"+e.asFileLocation.getFileName+"|"+esc(e.getExpansion.getMacroDefinition.getRawSignature))
  }
  tu.getNodeLocations.toList.collect { case e: IASTMacroExpansionLocation => e }.sortBy(_.asFileLocation.getNodeOffset).foreach { e =>
   val d=e.getExpansion.getMacroDefinition
   println("PAIR|"+e.asFileLocation.getNodeOffset+"|"+e.asFileLocation.getFileName+"|"+esc(d.getRawSignature))
  }
  def walk(n: IASTNode, depth: Int): Unit = {
   val loc=Option(n.getFileLocation)
   val ex=n.getNodeLocations.toList.collect { case e: IASTMacroExpansionLocation => e }
   if(ex.nonEmpty) println("NODE|"+depth+"|"+n.getClass.getSimpleName+"|"+loc.map(_.getNodeOffset).getOrElse(-1)+"|"+loc.map(_.getFileName).getOrElse("")+"|"+esc(n.getRawSignature)+"|"+ex.map(e=>esc(e.getExpansion.getMacroDefinition.getRawSignature)).mkString(";;"))
   n.getChildren.foreach(c=>walk(c,depth+1))
  }
  tu.getChildren.foreach(c=>walk(c,0))
 }
}

}
