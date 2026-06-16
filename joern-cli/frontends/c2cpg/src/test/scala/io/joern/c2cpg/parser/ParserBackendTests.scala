package io.joern.c2cpg.parser

import io.joern.c2cpg.Config
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.semanticcpg.language.*

import java.nio.file.Path

class ParserBackendTests extends C2CpgSuite {

  "The c2cpg parser backend selector" should {

    "default to the CDT backend" in {
      Config().parserBackend shouldBe ParserBackend.Cdt

      val cpg = code("int main() { return 0; }")
      cpg.method.nameExact("main").size shouldBe 1
    }

    "fail clearly if the oxidized backend is requested through the CDT translation unit provider" in {
      val config   = Config(parserBackend = ParserBackend.Oxidized)
      val provider = TranslationUnitProvider.forConfig(config, new HeaderFileFinder(config), None)

      val error = intercept[UnsupportedOperationException] {
        provider.languageMappingForSourceFile(Path.of("main.c"), Map.empty)
      }
      error.getMessage should include("does not expose Eclipse CDT translation units")
      error.getMessage should include("--parser-backend oxidized")
      error.getMessage should include("--parser-backend cdt")
    }

  }

}
