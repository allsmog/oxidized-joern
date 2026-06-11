package io.joern.pysrc2cpg.passes

import io.joern.x2cpg.frontendspecific.pysrc2cpg.PythonTypeStubs
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

class PythonTypeStubsTests extends AnyWordSpec with Matchers {

  "PythonTypeStubs" should {
    "resolve return types for security-relevant stdlib calls" in {
      PythonTypeStubs.returnTypeFor("subprocess.run") shouldBe Some("subprocess.CompletedProcess")
      PythonTypeStubs.returnTypeFor("subprocess.check_output") shouldBe Some("__builtin.bytes")
      PythonTypeStubs.returnTypeFor("os.system") shouldBe Some("__builtin.int")
      PythonTypeStubs.returnTypeFor("pickle.loads") shouldBe Some("__builtin.object")
      PythonTypeStubs.returnTypeFor("yaml.safe_load") shouldBe Some("__builtin.object")
      PythonTypeStubs.returnTypeFor("base64.b64decode") shouldBe Some("__builtin.bytes")
      PythonTypeStubs.returnTypeFor("re.compile") shouldBe Some("re.Pattern")
      PythonTypeStubs.returnTypeFor("sqlite3.connect") shouldBe Some("sqlite3.Connection")
      PythonTypeStubs.returnTypeFor("urllib.parse.urlparse") shouldBe Some("urllib.parse.ParseResult")
    }

    "resolve return types for common third-party HTTP calls" in {
      PythonTypeStubs.returnTypeFor("requests.get") shouldBe Some("requests.Response")
      PythonTypeStubs.returnTypeFor("requests.post") shouldBe Some("requests.Response")
      PythonTypeStubs.returnTypeFor("requests.Session") shouldBe Some("requests.Session")
    }

    "still resolve the original builtins" in {
      PythonTypeStubs.returnTypeFor("len") shouldBe Some("__builtin.int")
      PythonTypeStubs.returnTypeFor("json.loads") shouldBe Some("__builtin.dict")
    }

    "return None for unknown names" in {
      PythonTypeStubs.returnTypeFor("definitely.not.a.real.function") shouldBe None
    }
  }
}
