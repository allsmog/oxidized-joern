package io.joern.csharpsrc2cpg.astcreation

import io.joern.csharpsrc2cpg.datastructures.FieldDecl
import io.joern.csharpsrc2cpg.parser.DotNetJsonAst.*
import io.joern.csharpsrc2cpg.parser.{DotNetJsonAst, DotNetNodeInfo, ParserKeys}
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.{DeclarationNew, NewCall, NewFieldIdentifier, NewLocal}
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, Operators}

import scala.util.Try

trait AstForPrimitivesCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  protected def astForIdentifier(ident: DotNetNodeInfo, typeFullName: String = Defines.Any): Ast = {
    val identifierName = nameFromNode(ident)
    if (identifierName != "_") {
      scope.lookupVariable(identifierName) match {
        case Some(variable: DeclarationNew) =>
          val node = identifierFromDecl(variable, Option(ident))
          Ast(node).withRefEdge(node, variable)
        case None =>
          scope.findFieldInScope(identifierName) match {
            // Check for implicit field reference
            case Some(field) if field.node.node != DotNetJsonAst.VariableDeclarator =>
              astForFieldIdentifier(typeFullName, identifierName, field)
            case Some(field) =>
              Ast(identifierNode(ident, identifierName, identifierName, field.typeFullName))
            case None =>
              // Check for static type reference
              scope.tryResolveTypeReference(identifierName) match {
                case Some(typeReference) if typeFullName == Defines.Any =>
                  Ast(identifierNode(ident, identifierName, ident.code, typeReference.name))
                case _ =>
                  Ast(identifierNode(ident, identifierName, ident.code, typeFullName))
              }
          }
      }
    } else {
      Ast()
    }
  }

  private def astForFieldIdentifier(baseTypeFullName: String, baseIdentifierName: String, field: FieldDecl) = {
    val fieldAccess =
      operatorCallNode(field.node, field.node.code, Operators.fieldAccess, Some(field.typeFullName))
    val identifierAst = Ast(identifierNode(field.node, baseIdentifierName, baseIdentifierName, baseTypeFullName))
    val fieldIdentifier = Ast(
      NewFieldIdentifier()
        .code(field.name)
        .canonicalName(field.name)
        .lineNumber(field.node.lineNumber)
        .columnNumber(field.node.columnNumber)
    )
    callAst(fieldAccess, Seq(identifierAst, fieldIdentifier))
  }

  protected def astForUsing(usingNode: DotNetNodeInfo): Ast = {
    val targetNode = createDotNetNodeInfo(usingNode.json(ParserKeys.Name))
    val namespace = nameFromNode(targetNode) match {
      case "<empty>" => targetNode.code
      case name      => name
    }
    val alias = Try(usingNode.json(ParserKeys.Alias)).toOption
      .collect { case alias: ujson.Obj => nameFromNode(createDotNetNodeInfo(alias)) }
      .getOrElse(namespace.split('.').last)
    val importNode = newImportNode(code(usingNode), namespace, alias, usingNode)

    if (Try(usingNode.json(ParserKeys.Static).bool).getOrElse(false)) {
      scope.addImportedTypeOrModule(namespace)
      scope.addImportedMember(namespace)
    } else if (Try(usingNode.json(ParserKeys.Alias)).toOption.exists(_.isInstanceOf[ujson.Obj])) {
      scope.addImportedAlias(alias, nodeTypeFullName(targetNode))
    } else {
      scope.addImportedNamespace(namespace)
      scope.addImportedTypeOrModule(namespace) // We cannot determine if the namespace refers to a type so we do both
    }
    Ast(importNode)
  }

}
