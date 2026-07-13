package io.joern.gosrc2cpg.astcreation

import io.joern.gosrc2cpg.datastructures.MethodCacheMetaData
import io.joern.gosrc2cpg.parser.ParserAst.*
import io.joern.gosrc2cpg.parser.{ParserKeys, ParserNodeInfo}
import io.joern.x2cpg.{Ast, ValidationMode}
import io.shiftleft.codepropertygraph.generated.{NodeTypes, PropertyNames}
import io.shiftleft.codepropertygraph.generated.nodes.NewMethod
import ujson.Value

import scala.util.Try

trait CommonCacheBuilder(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  protected def identifyAndRecordPackagesWithDifferentName(): Unit = {
    // record the package to full namespace mapping only when declared package name is not matching with containing folder name
    if (declaredPackageName != fullyQualifiedPackage.split("/").last)
      goGlobal.recordAliasToNamespaceMapping(declaredPackageName, fullyQualifiedPackage)
  }

  protected def processPackageLevelGlobalVariablesAndConstants(json: Value): Unit = {
    json(ParserKeys.Decls).arrOpt
      .getOrElse(List())
      .map(createParserNodeInfo)
      .foreach { decl =>
        decl.node match {
          case GenDecl =>
            decl
              .json(ParserKeys.Specs)
              .arrOpt
              .getOrElse(List())
              .map(createParserNodeInfo)
              .foreach { spec =>
                spec.node match {
                  case ValueSpec => astForValueSpec(spec, true)
                  case _         =>
                  // Only process ValueSpec
                }
              }
          case _ =>
          // Only process GenDecl
        }
      }
  }

  protected def processFuncLiteral(funcLit: Value): Unit = {
    val LambdaFunctionMetaData(signature, _, _, _, _) = generateLambdaSignature(
      createParserNodeInfo(funcLit(ParserKeys.Type))
    )
    goGlobal.recordForThisLamdbdaSignature(signature)
  }

  protected def processTypeSepc(typeSepc: ParserNodeInfo): (String, String, Seq[Ast]) = {
    val name = typeSepc.json(ParserKeys.Name)(ParserKeys.Name).str
    if (goGlobal.checkForDependencyFlags(name)) {
      // Ignoring recording the Type details when we are processing dependencies code with Type name starting with lower case letter
      // As the Types starting with lower case letters will only be accessible within that package. Which means
      // these Types are not going to get referred from main source code.
      val fullName = typeSpecFullName(name)
      val typeNode = createParserNodeInfo(typeSepc.json(ParserKeys.Type))
      val ast = typeNode.node match {
        case InterfaceType => astForInterfaceType(typeNode, fullName)
        // astForStructType() function will record the member types
        case StructType => astForStructType(typeNode, fullName)
        // Process lambda function types to record lambda function signature mapped to TypeFullName
        case FuncType => processFuncType(typeNode, fullName)
        case _        => Seq.empty
      }
      (name, fullName, ast)
    } else
      ("", "", Seq.empty)
  }

  protected def typeSpecAstParent: (String, String) = {
    methodAstParentStack.headOption
      .map {
        case parent: NewMethod if parent.fullName.startsWith(s"$relPathFileName:") =>
          (NodeTypes.TYPE_DECL, fullyQualifiedPackage)
        case parent =>
          (parent.label, parent.properties(PropertyNames.FullName).toString)
      }
      .getOrElse((NodeTypes.TYPE_DECL, fullyQualifiedPackage))
  }

  private def typeSpecFullName(name: String): String = {
    val (_, parentFullName) = typeSpecAstParent
    s"$parentFullName${Defines.dot}$name"
  }

  protected def processFuncDecl(funcDeclVal: Value): MethodMetadata = {
    val name = funcDeclVal(ParserKeys.Name).obj(ParserKeys.Name).str
    if (goGlobal.checkForDependencyFlags(name)) {
      // Ignoring recording the method details when we are processing dependencies code with functions name starting with lower case letter
      // As the functions starting with lower case letters will only be accessible within that package. Which means
      // these methods / functions are not going to get referred from main source code.
      val receiverInfo = getReceiverInfo(Try(funcDeclVal(ParserKeys.Recv)))
      val (methodFullname, recordNamespace) = receiverInfo match {
        case Some(_, typeFullName, _, _) =>
          (s"$typeFullName.$name", typeFullName)
        case _ =>
          (s"$fullyQualifiedPackage.$name", fullyQualifiedPackage)
      }
      val genericTypeMethodMap = processTypeParams(funcDeclVal(ParserKeys.Type))
      val returnTypes          = getReturnType(funcDeclVal(ParserKeys.Type), genericTypeMethodMap).map(_._1)
      val returnTypeStr        = returnTypes.headOption.getOrElse(Defines.voidTypeName)
      val params               = funcDeclVal(ParserKeys.Type)(ParserKeys.Params)(ParserKeys.List)
      val signature =
        s"$methodFullname(${parameterSignature(params, genericTypeMethodMap)})${returnTypeSignature(returnTypes)}"
      goGlobal.recordMethodMetadata(recordNamespace, name, MethodCacheMetaData(returnTypeStr, signature))
      MethodMetadata(name, methodFullname, signature, params, receiverInfo, genericTypeMethodMap)
    } else
      MethodMetadata()
  }

  private def returnTypeSignature(returnTypes: Seq[String]): String = {
    returnTypes match {
      case Seq()                            => ""
      case Seq(Defines.voidTypeName)        => ""
      case Seq(singleReturnType)            => singleReturnType
      case multipleReturnTypes: Seq[String] => multipleReturnTypes.mkString("(", ",", ")")
    }
  }

  protected def processImports(importDecl: Value): (String, String) = {
    val importedEntity = importDecl(ParserKeys.Path).obj(ParserKeys.Value).str.replaceAll("\"", "")
    val importedAsOption =
      Try(importDecl(ParserKeys.Name).obj(ParserKeys.Name).str).toOption
    importedAsOption match {
      case Some(importedAs) =>
        // As these alias could be different for each file. Hence we maintain the cache at file level.
        aliasToNameSpaceMapping.put(importedAs, importedEntity)
        (importedEntity, importedAs)
      case _ =>
        val derivedImportedAs = importedEntity.split("/").last
        aliasToNameSpaceMapping.put(derivedImportedAs, importedEntity)
        (importedEntity, derivedImportedAs)
    }
  }
}

case class MethodMetadata(
  name: String = "",
  methodFullname: String = "",
  signature: String = "",
  params: Value = Value("{}"),
  receiverInfo: Option[(String, String, String, ParserNodeInfo)] = None,
  genericTypeMethodMap: Map[String, List[String]] = Map()
)
