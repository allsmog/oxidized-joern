package io.joern.csharpsrc2cpg.querying.ast

import io.shiftleft.semanticcpg.language.*
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture

class ImportTests extends CSharpCode2CpgFixture {

  "top-level using statements" should {

    val cpg = code("""
        |using System;
        |using System.Text;
        |
        |namespace HelloWorld
        |{
        |  class Program
        |  {
        |    static void Main(string[] args)
        |    {
        |      Console.WriteLine("Hey!");
        |    }
        |  }
        |
        |}
        |""".stripMargin)

    "create the respective import node for a simple base-level namespace" in {
      inside(cpg.imports.l) { case sysImport :: _ :: Nil =>
        sysImport.importedAs shouldBe Option("System")
        sysImport.importedEntity shouldBe Option("System")
      }
    }

    "create the respective import node for a fully-qualified namespace" in {
      inside(cpg.imports.l) { case _ :: textImport :: Nil =>
        textImport.importedAs shouldBe Option("Text")
        textImport.importedEntity shouldBe Option("System.Text")
      }
    }

    "allow for the type of `Console` to be known" in {
      inside(cpg.identifier.nameExact("Console").l) { case textImport :: Nil =>
        textImport.typeFullName shouldBe "System.Console"
      }
    }

  }

  "alias and static using statements" should {

    val cpg = code("""
        |using Alias = System.String;
        |using static System.Math;
        |
        |class Program
        |{
        |  static void Main(string[] args)
        |  {
        |    Alias text = "Hey!";
        |    var value = Abs(-1);
        |  }
        |}
        |""".stripMargin)

    "create import nodes with source-faithful aliases" in {
      inside(cpg.imports.l) { case aliasImport :: staticImport :: Nil =>
        aliasImport.code shouldBe "using Alias = System.String;"
        aliasImport.importedEntity shouldBe Option("System.String")
        aliasImport.importedAs shouldBe Option("Alias")

        staticImport.code shouldBe "using static System.Math;"
        staticImport.importedEntity shouldBe Option("System.Math")
        staticImport.importedAs shouldBe Option("Math")
      }
    }

    "resolve aliased type names" in {
      inside(cpg.local.nameExact("text").l) { case text :: Nil =>
        text.typeFullName shouldBe "System.String"
      }
    }

  }

}
