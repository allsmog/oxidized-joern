package io.joern.gosrc2cpg.astcreation

import io.joern.gosrc2cpg.parser.ParserAst.*
import io.joern.gosrc2cpg.parser.{ParserKeys, ParserNodeInfo}
import io.joern.x2cpg.{Ast, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.{NewCall, NewFieldIdentifier}
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, Operators}

import scala.util.{Success, Try}

trait AstForPrimitivesCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  protected def astForPrimitive(primitive: ParserNodeInfo): Seq[Ast] = {
    primitive.node match {
      case BasicLit     => Seq(astForLiteral(primitive))
      case CompositeLit => astForCompositeLiteral(primitive)
      case Ident        => Seq(astForIdentifier(primitive))
      case FuncLit      => astForFuncLiteral(primitive)
      case _            => Seq(Ast())
    }
  }

  private def astForCompositeLiteralHavingTypeKey(
    typeNode: ParserNodeInfo,
    compositeLiteralNode: ParserNodeInfo
  ): Seq[Ast] = {
    typeNode.node match {
      case ArrayType =>
        astForArrayInitializerWithElements(compositeLiteralNode, astForArrayInitializer(compositeLiteralNode))
      // Handling structure initialisation by creating a call node and arguments
      case Ident =>
        astForConstructorCall(compositeLiteralNode)
      // Handling structure initialisation(alias present) by creating a call node and arguments
      case SelectorExpr =>
        astForConstructorCall(compositeLiteralNode)
      case _ =>
        Seq.empty
    }
  }

  private def astForArrayInitializerWithElements(
    compositeLiteralNode: ParserNodeInfo,
    arrayInitCallNode: NewCall
  ): Seq[Ast] = {
    val elementsAsts = Try(compositeLiteralNode.json(ParserKeys.Elts)) match {
      case Success(value) if !value.isNull => value.arr.flatMap(e => astForNode(createParserNodeInfo(e))).toSeq
      case _                               => Seq.empty
    }
    Seq(callAst(arrayInitCallNode, elementsAsts))
  }

  private def astForCompositeLiteral(compositeLiteralNodeInfo: ParserNodeInfo): Seq[Ast] = {
    Try(createParserNodeInfo(compositeLiteralNodeInfo.json(ParserKeys.Type))) match {
      case Success(typeNode) =>
        astForCompositeLiteralHavingTypeKey(typeNode, compositeLiteralNodeInfo)
      case _ =>
        // Elided-type / nested composite literal (e.g. the inner {"1","2"} of [][]string{...}):
        // wrap the elements in an arrayInitializer call instead of flattening them away.
        val elements = Try(compositeLiteralNodeInfo.json(ParserKeys.Elts)) match {
          case Success(compositeElements) if !compositeElements.isNull => compositeElements.arr.toSeq
          case _                                                       => Seq.empty
        }
        val elementsAsts = elements.flatMap(e => astForNode(createParserNodeInfo(e)))
        // Keyed elements (e.g. the inner {phone: "..."} of []Phone{...}) belong to an elided
        // struct or map literal, not an array; keep flattening those.
        val isKeyedLiteral =
          elements.exists(e => Try(e(ParserKeys.NodeType).str).toOption.contains("ast.KeyValueExpr"))
        if (isKeyedLiteral) {
          elementsAsts
        } else {
          // The Type key is absent here, so we cannot use processTypeInfo. Derive the type
          // as "[]" + element type, mirroring the outer dimension's element type.
          val elemType = elementsAsts.headOption
            .map(elementAst => getTypeFullNameFromAstNode(Seq(elementAst)))
            .getOrElse(Defines.anyTypeName)
          val arrayInitCallNode = callNode(
            compositeLiteralNodeInfo,
            compositeLiteralNodeInfo.code,
            Operators.arrayInitializer,
            Operators.arrayInitializer,
            DispatchTypes.STATIC_DISPATCH,
            Option(Defines.empty),
            Option(s"[]$elemType")
          )
          Seq(callAst(arrayInitCallNode, elementsAsts))
        }
    }
  }

  private def astForLiteral(stringLiteral: ParserNodeInfo): Ast = {
    // TODO May need to revisit this
    val typ = getTypeOfToken(stringLiteral)
    Ast(literalNode(stringLiteral, stringLiteral.code, typ))
  }

  private def astForIdentifier(ident: ParserNodeInfo): Ast = {
    val identifierName = ident.json(ParserKeys.Name).str
    if (identifierName != "_") {
      val variableOption = scope.lookupVariable(identifierName)
      variableOption match {
        case Some((variable, variableTypeName)) =>
          val node = identifierNode(ident, identifierName, ident.code, variableTypeName)
          Ast(node).withRefEdge(node, variable)
        case _ =>
          // If its not local node then check if its global member variable of package TypeDecl
          goGlobal.getStructTypeMemberType(fullyQualifiedPackage, identifierName) match {
            case Some(fieldTypeFullName) => astForPackageGlobalFieldAccess(fieldTypeFullName, identifierName, ident)
            case _                       =>
              // TODO: something is wrong here. Refer to SwitchTests -> "be correct for switch case 4"
              Ast(identifierNode(ident, identifierName, ident.json(ParserKeys.Name).str, Defines.anyTypeName))
          }
      }
    } else {
      Ast()
    }
  }

  protected def astForPackageGlobalFieldAccess(
    fieldTypeFullName: String,
    identifierName: String,
    ident: ParserNodeInfo
  ): Ast = {
    val identifierAsts = Seq(Ast(identifierNode(ident, declaredPackageName, ident.code, fullyQualifiedPackage)))
    callAst(
      operatorCallNode(ident, ident.code, Operators.fieldAccess, Some(fieldTypeFullName)),
      identifierAsts ++: Seq(
        Ast(
          NewFieldIdentifier()
            .canonicalName(identifierName)
            .lineNumber(line(ident))
            .columnNumber(column(ident))
            .code(identifierName)
        )
      )
    )
  }

  protected def getTypeOfToken(basicLit: ParserNodeInfo): String = {
    Try(basicLit.json(ParserKeys.Kind).str match {
      case "INT"    => "int"
      case "FLOAT"  => "float64"
      case "IMAG"   => "complex128"
      case "CHAR"   => "int32"
      case "STRING" => "string"
      case _        => Defines.anyTypeName
    }).toOption.getOrElse(Defines.anyTypeName)
  }

  protected def astForBooleanLiteral(rhsParserNode: ParserNodeInfo): Seq[Ast] = {
    rhsParserNode.node match {
      case Ident
          // NOTE: This is very corner case where for boolean literals true and false.
          // We don't get node of type BasicLit as is the case with other literals. Hence we have to handle it here
          if (rhsParserNode.json(ParserKeys.Name).str == "true" || rhsParserNode
            .json(ParserKeys.Name)
            .str == "false") =>
        Seq(Ast(literalNode(rhsParserNode, rhsParserNode.code, Defines.Bool)))
      case _ =>
        astForNode(rhsParserNode)
    }
  }

  private def astForArrayInitializer(primitive: ParserNodeInfo): NewCall = {
    val (typeFullName, _, _, _) = processTypeInfo(primitive)
    callNode(
      primitive,
      primitive.code,
      Operators.arrayInitializer,
      Operators.arrayInitializer,
      DispatchTypes.STATIC_DISPATCH,
      Option(Defines.empty),
      Option(typeFullName)
    )
  }
}
