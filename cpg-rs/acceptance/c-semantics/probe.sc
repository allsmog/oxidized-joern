import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.semanticcpg.language.*
import java.nio.charset.StandardCharsets
import java.util.Base64

def encoded(values: List[String]): String =
  Base64.getEncoder.encodeToString(values.sorted.mkString("\u001f").getBytes(StandardCharsets.UTF_8))

def emit(id: String, values: List[String]): Unit =
  println(s"CSEM\t$id\t${encoded(values)}")

@main def exec(cpgPath: String) = {
  val cpg = CpgLoader.load(cpgPath)
  try {
    emit("selected-branch", cpg.method.nameExact("selected", "dead").name.l)
    emit("macro-call", cpg.call.nameExact("SCALE").code.l)
    emit("macro-origin", cpg.method.nameExact("SCALE").fullName.l)
    emit("expanded-multiply", cpg.call.nameExact("<operator>.multiplication").code.l)
  } finally cpg.close()
}
