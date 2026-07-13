package io.joern.gosrc2cpg.astcreation

import io.joern.gosrc2cpg.datastructures.PackageMemberAst
import io.joern.gosrc2cpg.parser.ParserAst.*
import io.joern.gosrc2cpg.parser.{ParserKeys, ParserNodeInfo}
import io.joern.x2cpg
import io.joern.x2cpg.{Ast, ValidationMode}
import io.joern.x2cpg.utils.AstPropertiesUtil.RootProperties
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, NodeTypes, Operators}
import ujson.Value

import scala.util.{Success, Try}

trait AstForGenDeclarationCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>
  def astForGenDecl(genDecl: ParserNodeInfo, globalStatements: Boolean = false): Seq[Ast] = {
    Try(
      genDecl
        .json(ParserKeys.Specs)
        .arr
    ) match {
      case Success(specArr) =>
        specArr
          .map(createParserNodeInfo)
          .flatMap { genDeclNode =>
            genDeclNode.node match {
              case ImportSpec                     => astForImport(genDeclNode)
              case TypeSpec                       => astForTypeSpec(genDeclNode)
              case ValueSpec if !globalStatements => astForValueSpec(genDeclNode, globalStatements = globalStatements)
              case _                              => Seq[Ast]()
            }
          }
          .toSeq
      case _ =>
        Seq.empty
    }
  }

  private def astForImport(nodeInfo: ParserNodeInfo): Seq[Ast] = {
    val basicLit                     = createParserNodeInfo(nodeInfo.json(ParserKeys.Path))
    val (importedEntity, importedAs) = processImports(nodeInfo.json)
    val importedAsReplacement        = if (importedEntity.equals(importedAs)) "" else s"$importedAs "
    // This may be better way to add code for import node
    Seq(Ast(newImportNode(s"import $importedAsReplacement$importedEntity", importedEntity, importedAs, basicLit)))
  }

  protected def astForValueSpec(valueSpec: ParserNodeInfo, globalStatements: Boolean = false): Seq[Ast] = {
    val typeFullName = Try(valueSpec.json(ParserKeys.Type)) match {
      case Success(typeJson) =>
        val (typeFullName, _, _, _) = processTypeInfo(createParserNodeInfo(typeJson))
        Some(typeFullName)
      case _ => None
    }

    Try(valueSpec.json(ParserKeys.Values).arr.toList) match {
      case Success(_) =>
        val lhsParserNodes = valueSpec.json(ParserKeys.Names).arr.toList.map(createParserNodeInfo)
        val rhsParserNodes = valueSpec.json(ParserKeys.Values).arr.toList.map(createParserNodeInfo)
        val (assCallAsts, localAsts) =
          astForAssignmentCallNodes(lhsParserNodes, rhsParserNodes, typeFullName, valueSpec.code, globalStatements)
        if (globalStatements) Seq.empty else localAsts ++: assCallAsts
      case _ =>
        valueSpec
          .json(ParserKeys.Names)
          .arr
          .flatMap { parserNode =>
            val localParserNode = createParserNodeInfo(parserNode)
            if (globalStatements) {
              val variableName = localParserNode.json(ParserKeys.Name).str
              if (goGlobal.checkForDependencyFlags(variableName)) {
                // While processing the dependencies code ignoring package level global variables starting with lower case letter
                // as these variables are only accessible within package. So those will not be referred from main source code.
                goGlobal.recordStructTypeMemberTypeInfo(
                  fullyQualifiedPackage,
                  variableName,
                  typeFullName.getOrElse(Defines.anyTypeName)
                )
                astForGlobalVarAndConstants(typeFullName.getOrElse(Defines.anyTypeName), localParserNode)
              }
              Seq.empty
            } else {
              Seq(astForLocalNode(localParserNode, typeFullName)) ++: astForNode(localParserNode)
            }
          }
          .toSeq
    }
  }

  protected def astForAssignmentCallNode(
    lhsParserNode: ParserNodeInfo,
    rhsParserNode: ParserNodeInfo,
    typeFullName: Option[String],
    code: String,
    globalStatements: Boolean = false
  ): (Ast, Ast) = {
    val rhsAst = astForBooleanLiteral(rhsParserNode)
    val rhsTypeFullName = typeFullName
      .orElse(makeCallResultType(rhsParserNode))
      .getOrElse(getTypeFullNameFromAstNode(rhsAst))
    if (globalStatements) {
      val variableName = lhsParserNode.json(ParserKeys.Name).str
      if (goGlobal.checkForDependencyFlags(variableName)) {
        goGlobal.recordStructTypeMemberTypeInfo(fullyQualifiedPackage, variableName, rhsTypeFullName)
        astForGlobalVarAndConstants(rhsTypeFullName, lhsParserNode, Some(rhsAst))
      }
      (Ast(), Ast())
    } else {
      val localAst  = astForLocalNode(lhsParserNode, Some(rhsTypeFullName))
      val lhsAst    = astForNode(lhsParserNode)
      val arguments = lhsAst ++: rhsAst
      val cNode = callNode(
        rhsParserNode,
        code,
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(rhsTypeFullName)
      )
      (callAst(cNode, arguments), localAst)
    }
  }

  protected def astForAssignmentCallNodes(
    lhsParserNodes: Seq[ParserNodeInfo],
    rhsParserNodes: Seq[ParserNodeInfo],
    typeFullName: Option[String],
    code: String,
    globalStatements: Boolean = false
  ): (Seq[Ast], Seq[Ast]) = {
    if (!globalStatements && lhsParserNodes.size > 1 && rhsParserNodes.size == 1) {
      astForTupleUnpackingAssignment(lhsParserNodes, rhsParserNodes.head, typeFullName, code)
    } else {
      lhsParserNodes
        .zip(rhsParserNodes)
        .map { case (lhsParserNode, rhsParserNode) =>
          astForAssignmentCallNode(lhsParserNode, rhsParserNode, typeFullName, code, globalStatements)
        }
        .unzip
    }
  }

  private var tupleTempCounter = 0

  private def nextTupleTempName(): String = {
    val name = s"<tuple-return>$tupleTempCounter"
    tupleTempCounter += 1
    name
  }

  private def astForTupleUnpackingAssignment(
    lhsParserNodes: Seq[ParserNodeInfo],
    rhsParserNode: ParserNodeInfo,
    typeFullName: Option[String],
    code: String
  ): (Seq[Ast], Seq[Ast]) = {
    val rhsAst = astForBooleanLiteral(rhsParserNode)
    val rhsTypeFullName = typeFullName
      .orElse(makeCallResultType(rhsParserNode))
      .getOrElse(getTypeFullNameFromAstNode(rhsAst))

    val tempName  = nextTupleTempName()
    val tempLocal = localNode(rhsParserNode, tempName, tempName, rhsTypeFullName)
    scope.addToScope(tempName, (tempLocal, rhsTypeFullName))

    val tempAssignmentLhs    = identifierNode(rhsParserNode, tempName, tempName, rhsTypeFullName)
    val tempAssignmentLhsAst = Ast(tempAssignmentLhs).withRefEdge(tempAssignmentLhs, tempLocal)
    val tempAssignmentNode = callNode(
      rhsParserNode,
      s"$tempName = ${rhsParserNode.code}",
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(rhsTypeFullName)
    )
    val tempAssignmentAst = callAst(tempAssignmentNode, tempAssignmentLhsAst +: rhsAst)

    val (assignmentAsts, localAsts) = lhsParserNodes
      .filterNot(isBlankIdentifier)
      .map { lhsParserNode =>
        val localAst   = astForLocalNode(lhsParserNode, Some(rhsTypeFullName))
        val lhsAst     = astForNode(lhsParserNode)
        val tempUse    = identifierNode(lhsParserNode, tempName, tempName, rhsTypeFullName)
        val tempUseAst = Ast(tempUse).withRefEdge(tempUse, tempLocal)
        val assignmentNode = callNode(
          lhsParserNode,
          s"${lhsParserNode.code} = $tempName",
          Operators.assignment,
          Operators.assignment,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Some(rhsTypeFullName)
        )
        (callAst(assignmentNode, lhsAst ++ Seq(tempUseAst)), localAst)
      }
      .unzip

    (tempAssignmentAst +: assignmentAsts, Ast(tempLocal) +: localAsts)
  }

  private def isBlankIdentifier(parserNodeInfo: ParserNodeInfo): Boolean =
    parserNodeInfo.node == Ident && parserNodeInfo.json(ParserKeys.Name).str == "_"

  private def makeCallResultType(rhsParserNode: ParserNodeInfo): Option[String] = {
    Option
      .when(rhsParserNode.node == CallExpr) {
        val funNode = createParserNodeInfo(rhsParserNode.json(ParserKeys.Fun))
        val firstArg = rhsParserNode
          .json(ParserKeys.Args)
          .arrOpt
          .flatMap(_.headOption)
          .map(createParserNodeInfo)

        Option
          .when(funNode.node == Ident && funNode.json(ParserKeys.Name).str == "make") {
            firstArg.collect {
              case arg if arg.node == MapType  => Defines.map
              case arg if arg.node == ChanType => Defines.chan
            }
          }
          .flatten
      }
      .flatten
  }

  private def astForGlobalVarAndConstants(
    typeFullName: String,
    lhsParserNode: ParserNodeInfo,
    rhsAst: Option[Seq[Ast]] = None
  ): Unit = {
    val name = lhsParserNode.json(ParserKeys.Name).str
    val memberAst = Ast(
      memberNode(lhsParserNode, name, lhsParserNode.code, typeFullName)
        .astParentType(NodeTypes.TYPE_DECL)
        .astParentFullName(fullyQualifiedPackage)
    )
    Ast.storeInDiffGraph(memberAst, diffGraph)
    rhsAst match {
      case Some(rhsSeqAst) if !goGlobal.processingDependencies =>
        // Add this AST to be processed in PackageCtorCreationPass only for main source code. Ignore it while processing dependency code.
        // Only in case rhs ast is present then the respective variable or constant will be added as part
        // of package level initializer/constructor statement
        val lhsAst    = astForPackageGlobalFieldAccess(typeFullName, name, lhsParserNode)
        val arguments = Seq(lhsAst) ++: rhsSeqAst
        val assignmentCode =
          s"var ${lhsParserNode.code} = ${rhsSeqAst.headOption.flatMap(_.rootCode).getOrElse(Defines.empty)}"
        val cNode = callNode(
          lhsParserNode,
          assignmentCode,
          Operators.assignment,
          Operators.assignment,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Some(typeFullName)
        )
        goGlobal.recordPkgLevelVarAndConstantAst(
          fullyQualifiedPackage,
          PackageMemberAst(callAst(cNode, arguments), relPathFileName)
        )
      case _ =>
    }
  }

  protected def astForLocalNode(localParserNode: ParserNodeInfo, typeFullName: Option[String]): Ast = {
    val name = localParserNode.json(ParserKeys.Name).str
    if (name != "_") {
      val typeFullNameStr = typeFullName.getOrElse(Defines.anyTypeName)
      val node            = localNode(localParserNode, name, localParserNode.code, typeFullNameStr)
      scope.addToScope(name, (node, typeFullNameStr))
      Ast(node)
    } else {
      Ast()
    }
  }
}
