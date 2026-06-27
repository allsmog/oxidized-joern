package io.joern.csharpsrc2cpg.astcreation

import io.joern.csharpsrc2cpg.{CSharpModifiers, Constants}
import io.joern.csharpsrc2cpg.astcreation.AstParseLevel.FULL_AST
import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes.DotNetTypeMap
import io.joern.csharpsrc2cpg.datastructures.*
import io.joern.csharpsrc2cpg.parser.DotNetJsonAst.*
import io.joern.csharpsrc2cpg.parser.{DotNetNodeInfo, ParserKeys}
import io.joern.csharpsrc2cpg.utils.Utils.{
  composeGetterName,
  composeMethodFullName,
  composeMethodLikeSignature,
  composeSetterName,
  withoutSignature
}
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.*
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.proto.cpg.Cpg.EvaluationStrategies

import scala.annotation.tailrec
import scala.collection.mutable
import scala.util.Try
import java.util.regex.Pattern

trait AstForDeclarationsCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  private val emittedNamespaceBlockFullNames = mutable.Set.empty[String]

  private def inheritedTypeFullNames(typeDecl: DotNetNodeInfo): Seq[String] =
    Try(typeDecl.json(ParserKeys.BaseList)).toOption match {
      case Some(baseList: ujson.Obj) =>
        baseList(ParserKeys.Types).arr.map { t =>
          nodeTypeFullName(createDotNetNodeInfo(t(ParserKeys.Type)))
        }.toSeq
      case _ => Seq.empty
    }

  protected def astForNamespaceDeclaration(namespace: DotNetNodeInfo): Seq[Ast] = {
    @tailrec
    def recurseNamespace(parts: List[String], prefix: List[String] = List.empty): Unit = {
      parts match {
        case head :: tail =>
          val currentFullName = prefix :+ head
          scope.pushNewScope(NamespaceScope(currentFullName.mkString(".")))
          recurseNamespace(tail, currentFullName)
        case Nil => // nothing
      }
    }

    val fullName = astFullName(namespace)

    val namespaceParts = fullName.split("[.]").toList
    recurseNamespace(namespaceParts)

    val implicitNamespaceAsts = namespaceParts
      .scanLeft(List.empty[String])(_ :+ _)
      .tail
      .dropRight(1)
      .flatMap { prefixParts =>
        val prefixFullName = prefixParts.mkString(".")
        Option.when(emittedNamespaceBlockFullNames.add(prefixFullName)) {
          val prefixName = prefixParts.last
          val namespaceBlock = NewNamespaceBlock()
            .name(prefixName)
            .code(s"namespace $prefixFullName")
            .lineNumber(line(namespace))
            .columnNumber(column(namespace))
            .filename(relativeFileName)
            .fullName(prefixFullName)
          Ast(namespaceBlock)
        }
      }

    val name = fullName.split('.').filterNot(_.isBlank).lastOption.getOrElse(fullName)
    val namespaceBlock = NewNamespaceBlock()
      .name(name)
      .code(code(namespace))
      .lineNumber(line(namespace))
      .columnNumber(columnEnd(namespace))
      .filename(relativeFileName)
      .fullName(fullName)
    emittedNamespaceBlockFullNames.add(fullName)
    val memberAsts = namespace.json(ParserKeys.Members).arr.flatMap(astForNode).toSeq
    namespaceParts.foreach(_ => scope.popScope())
    implicitNamespaceAsts :+ Ast(namespaceBlock).withChildren(memberAsts)
  }

  protected def astForClassDeclaration(classDecl: DotNetNodeInfo): Seq[Ast] = {
    val name                     = nameFromNode(classDecl)
    val fullName                 = astFullName(classDecl)
    val inheritsFromTypeFullName = inheritedTypeFullNames(classDecl)

    inheritsFromTypeFullName.foreach(scope.pushTypeToScope)

    val annotationAsts =
      Try(classDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    val typeDecl =
      typeDeclNode(
        classDecl,
        name,
        fullName,
        relativeFileName,
        code(classDecl),
        inherits = inheritsFromTypeFullName,
        genericSignature = genericSignatureForDeclaration(classDecl)
      )
    scope.pushNewScope(TypeScope(fullName))
    val (modifiers, members) =
      try {
        withInheritedTypeFullNames(inheritsFromTypeFullName) {
          val modifiers = astForModifiers(classDecl)
          val members = astForMembers(classDecl.json(ParserKeys.Members).arr.map(createDotNetNodeInfo).toSeq)
            ++ astForPrimaryConstructorDeclaration(classDecl, fullName)
            ++ addConstructorWithFieldInitializationsIfNeeded(classDecl, fullName)
            ++ addStaticConstructorWithFieldInitializationsIfNeeded(classDecl, fullName)
          (modifiers, members)
        }
      } finally {
        scope.popScope()
      }
    val typeDeclAst = Ast(typeDecl)
      .withChildren(modifiers)
      .withChildren(members)
      .withChildren(annotationAsts)
    Seq(typeDeclAst)
  }

  private def addConstructorWithFieldInitializationsIfNeeded(
    node: DotNetNodeInfo,
    typeDeclFullName: String
  ): Seq[Ast] = {
    val dynamicFields = scope.getFieldsInScope.filter(f => !f.isStatic && f.isInitialized)
    val hasExplicitCtor =
      scope.tryResolveTypeReference(typeDeclFullName).exists(_.methods.exists(_.name == Defines.ConstructorMethodName))
    val hasPrimaryCtor = primaryConstructorParameters(node).nonEmpty
    // We should only create the constructor when we are the FULL_AST parseLevel. Otherwise, hasExplicitCtor will
    // not be accurate.
    val shouldBuildCtor = dynamicFields.nonEmpty && !hasExplicitCtor && !hasPrimaryCtor && parseLevel == FULL_AST

    if (shouldBuildCtor) {
      val methodReturn = methodReturnNode(node, DotNetTypeMap(BuiltinTypes.Void))
      val signature    = composeMethodLikeSignature(methodReturn.typeFullName)
      val modifiers    = Seq(modifierNode(node, ModifierTypes.CONSTRUCTOR), modifierNode(node, ModifierTypes.INTERNAL))
      val name         = Defines.ConstructorMethodName
      val fullName     = composeMethodFullName(typeDeclFullName, name, signature)

      val body = {
        scope.pushNewScope(MethodScope(fullName))
        val fieldInitAssignmentAsts = astVariableDeclarationForInitializedFields(dynamicFields)
        scope.popScope()
        Ast(NewBlock().typeFullName(Defines.Any)).withChildren(fieldInitAssignmentAsts)
      }

      val methodNode_ = NewMethod()
        .name(name)
        .fullName(fullName)
        .signature(signature)
        .filename(relativeFileName)

      val parameterNodes = Seq(
        NewMethodParameterIn()
          .name(Constants.This)
          .code(Constants.This)
          .typeFullName(typeDeclFullName)
          .evaluationStrategy(EvaluationStrategies.BY_SHARING.name)
          .isVariadic(false)
          .index(0)
      )

      methodAst(methodNode_, parameterNodes.map(Ast(_)), body, methodReturn, modifiers) :: Nil
    } else {
      Seq.empty
    }
  }

  private def astForPrimaryConstructorDeclaration(typeDecl: DotNetNodeInfo, typeDeclFullName: String): Seq[Ast] = {
    val parameterNodes = primaryConstructorParameters(typeDecl)
    if (parameterNodes.isEmpty) {
      return Seq.empty
    }

    val params       = parameterNodes.zipWithIndex.map(astForParameter(_, _, None))
    val methodReturn = methodReturnNode(typeDecl, DotNetTypeMap(BuiltinTypes.Void))
    val signature    = composeMethodLikeSignature(DotNetTypeMap(BuiltinTypes.Void), params)
    val name         = Defines.ConstructorMethodName
    val fullName     = composeMethodFullName(typeDeclFullName, name, signature)
    val modifiers = modifiersForNode(typeDecl).filter { modifier =>
      Set(ModifierTypes.PUBLIC, ModifierTypes.PRIVATE, ModifierTypes.INTERNAL, ModifierTypes.PROTECTED).contains(
        modifier.modifierType
      )
    } :+ modifierNode(typeDecl, ModifierTypes.CONSTRUCTOR)

    scope.pushNewScope(MethodScope(fullName))
    val primaryBaseInitializerAsts = astForPrimaryConstructorBaseInitializers(typeDecl)
    val fieldInitializerAsts =
      astVariableDeclarationForInitializedFields(scope.getFieldsInScope.filter(f => !f.isStatic && f.isInitialized))
    val body =
      Ast(NewBlock().typeFullName(Defines.Any)).withChildren(primaryBaseInitializerAsts ++ fieldInitializerAsts)
    scope.popScope()

    val methodNode_ = methodNode(typeDecl, name, code(typeDecl), fullName, Option(signature), relativeFileName)
    methodAst(methodNode_, astForThisParameter(typeDecl) +: params, body, methodReturn, modifiers) :: Nil
  }

  private def primaryConstructorParameters(typeDecl: DotNetNodeInfo): Seq[DotNetNodeInfo] =
    Try {
      typeDecl.json(ParserKeys.ParameterList).obj(ParserKeys.Parameters).arr.map(createDotNetNodeInfo).toSeq
    }.getOrElse(Seq.empty)

  private def astForPrimaryConstructorBaseInitializers(typeDecl: DotNetNodeInfo): Seq[Ast] =
    Try(typeDecl.json(ParserKeys.BaseList)(ParserKeys.Types).arr).toOption.toSeq.flatten
      .map(createDotNetNodeInfo)
      .filter(_.node == PrimaryConstructorBaseType)
      .flatMap { baseType =>
        val argumentList  = createDotNetNodeInfo(baseType.json(ParserKeys.ArgumentList))
        val arguments     = astForArgumentList(argumentList)
        val argTypes      = arguments.map(getTypeFullNameFromAstNode)
        val returnType    = DotNetTypeMap(BuiltinTypes.Void)
        val signature     = composeMethodLikeSignature(returnType, argTypes)
        val ownerFullName = nodeTypeFullName(createDotNetNodeInfo(baseType.json(ParserKeys.Type)))
        val fullName      = composeMethodFullName(ownerFullName, Defines.ConstructorMethodName, signature)
        val call = callNode(
          baseType,
          code(baseType),
          Defines.ConstructorMethodName,
          fullName,
          DispatchTypes.STATIC_DISPATCH,
          Option(signature),
          Option(returnType)
        )
        Seq(callAst(call, arguments))
      }

  private def addStaticConstructorWithFieldInitializationsIfNeeded(
    node: DotNetNodeInfo,
    typeDeclFullname: String
  ): Seq[Ast] = {
    val staticFields = scope.getFieldsInScope.filter(f => f.isStatic && f.isInitialized)
    val hasExplicitCtor =
      scope.tryResolveTypeReference(typeDeclFullname).exists(_.methods.exists(_.name == Defines.StaticInitMethodName))
    val shouldBuildCtor = staticFields.nonEmpty && !hasExplicitCtor && parseLevel == FULL_AST

    if (shouldBuildCtor) {
      val methodReturn = methodReturnNode(node, DotNetTypeMap(BuiltinTypes.Void))
      val signature    = composeMethodLikeSignature(methodReturn.typeFullName)
      val modifiers = Seq(
        modifierNode(node, ModifierTypes.CONSTRUCTOR),
        modifierNode(node, ModifierTypes.INTERNAL),
        modifierNode(node, ModifierTypes.STATIC)
      )
      val name     = Defines.StaticInitMethodName
      val fullName = composeMethodFullName(typeDeclFullname, name, signature)

      val body = {
        scope.pushNewScope(MethodScope(fullName))
        val fieldInitAssignmentAsts = astVariableDeclarationForInitializedFields(staticFields)
        scope.popScope()
        Ast(NewBlock().typeFullName(Defines.Any)).withChildren(fieldInitAssignmentAsts)
      }

      val methodNode_ = NewMethod()
        .name(name)
        .fullName(fullName)
        .signature(signature)
        .filename(relativeFileName)

      methodAst(methodNode_, Nil, body, methodReturn, modifiers) :: Nil
    } else {
      Nil
    }
  }

  protected def astForRecordDeclaration(recordDecl: DotNetNodeInfo): Seq[Ast] = {
    val name                     = nameFromNode(recordDecl)
    val fullName                 = astFullName(recordDecl)
    val inheritsFromTypeFullName = inheritedTypeFullNames(recordDecl)
    inheritsFromTypeFullName.foreach(scope.pushTypeToScope)

    val typeDecl =
      typeDeclNode(
        recordDecl,
        name,
        fullName,
        relativeFileName,
        code(recordDecl),
        inherits = inheritsFromTypeFullName,
        genericSignature = genericSignatureForDeclaration(recordDecl)
      )
    scope.pushNewScope(TypeScope(fullName))

    // Covers the case where record type can be declared as `record Person(string Name);`
    // Here, Person should be a TypeDecl and Name should be a member instead of a parameter
    val (modifiers, members) =
      try {
        withInheritedTypeFullNames(inheritsFromTypeFullName) {
          val modifiers = astForModifiers(recordDecl)
          val membersFromParams = Try {
            recordDecl
              .json(ParserKeys.ParameterList)(ParserKeys.Parameters)
              .arr
              .map(createDotNetNodeInfo)
              .toSeq
          }.toOption
            .getOrElse(Seq.empty)
            .map { paramNode =>
              val name         = nameFromNode(paramNode)
              val typeFullName = nodeTypeFullName(paramNode)
              Ast(memberNode(paramNode, name, paramNode.code, typeFullName))
            }

          val members =
            astForMembers(recordDecl.json(ParserKeys.Members).arr.map(createDotNetNodeInfo).toSeq)
              ++ membersFromParams
              ++ astForPrimaryConstructorDeclaration(recordDecl, fullName)
              ++ addConstructorWithFieldInitializationsIfNeeded(recordDecl, fullName)
              ++ addStaticConstructorWithFieldInitializationsIfNeeded(recordDecl, fullName)
          (modifiers, members)
        }
      } finally {
        scope.popScope()
      }

    val annotationAsts =
      Try(recordDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    val typeDeclAst = Ast(typeDecl)
      .withChildren(modifiers)
      .withChildren(members)
      .withChildren(annotationAsts)
    Seq(typeDeclAst)
  }

  protected def astForDelegateDeclaration(delegateDecl: DotNetNodeInfo): Seq[Ast] = {
    val name     = nameFromNode(delegateDecl)
    val fullName = astFullName(delegateDecl)
    val typeDecl = typeDeclNode(
      delegateDecl,
      name,
      fullName,
      relativeFileName,
      code(delegateDecl),
      inherits = Seq("System.MulticastDelegate"),
      genericSignature = genericSignatureForDeclaration(delegateDecl)
    )
    scope.pushNewScope(TypeScope(fullName))
    val modifiers    = astForModifiers(delegateDecl)
    val invokeMethod = astForDelegateInvokeMethod(delegateDecl, fullName)
    val annotationAsts =
      Try(delegateDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)
    scope.popScope()

    val typeDeclAst = Ast(typeDecl)
      .withChildren(modifiers)
      .withChild(invokeMethod)
      .withChildren(annotationAsts)
    Seq(typeDeclAst)
  }

  private def astForDelegateInvokeMethod(delegateDecl: DotNetNodeInfo, delegateFullName: String): Ast = {
    val params = delegateDecl
      .json(ParserKeys.ParameterList)
      .obj(ParserKeys.Parameters)
      .arr
      .map(createDotNetNodeInfo)
      .zipWithIndex
      .map(astForParameter(_, _, None))
      .toSeq

    val returnTypeNode = createDotNetNodeInfo(delegateDecl.json(ParserKeys.ReturnType))
    val returnType     = nodeTypeFullName(returnTypeNode)
    val methodReturn   = methodReturnNode(returnTypeNode, returnType)
    val signature      = composeMethodLikeSignature(returnType, params)
    val name           = "Invoke"
    val fullName       = composeMethodFullName(delegateFullName, name, signature)
    val methodNode_ = methodNode(delegateDecl, name, code(delegateDecl), fullName, Option(signature), relativeFileName)
    val body        = Ast(blockNode(delegateDecl))
    val modifiers   = Seq(modifierNode(delegateDecl, ModifierTypes.PUBLIC))
    methodAst(methodNode_, astForThisParameter(delegateDecl) +: params, body, methodReturn, modifiers)
  }

  protected def astForEnumDeclaration(enumDecl: DotNetNodeInfo): Seq[Ast] = {
    val name     = nameFromNode(enumDecl)
    val fullName = astFullName(enumDecl)
    val aliasFor = Try(enumDecl.json(ParserKeys.BaseList)(ParserKeys.Types).arr.map(createDotNetNodeInfo).head).toOption
      .map(nodeTypeFullName)
      .getOrElse(DotNetTypeMap(BuiltinTypes.Int))

    val typeDecl = typeDeclNode(enumDecl, name, fullName, relativeFileName, code(enumDecl))
    scope.pushNewScope(EnumScope(fullName, aliasFor))
    val modifiers = astForModifiers(enumDecl)

    val annotationAsts =
      Try(enumDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    val memberDecls = enumDecl.json(ParserKeys.Members).arr.map(createDotNetNodeInfo).toSeq
    val members = astForMembers(memberDecls) ++ astForEnumStaticInitializer(enumDecl, fullName, aliasFor, memberDecls)
    scope.popScope()
    val typeDeclAst = Ast(typeDecl)
      .withChildren(modifiers)
      .withChildren(members)
      .withChildren(annotationAsts)
    Seq(typeDeclAst)
  }

  private def astForEnumStaticInitializer(
    enumDecl: DotNetNodeInfo,
    typeDeclFullName: String,
    enumTypeFullName: String,
    enumMemberDecls: Seq[DotNetNodeInfo]
  ): Seq[Ast] = {
    if (enumMemberDecls.isEmpty) {
      Nil
    } else {
      val returnType   = DotNetTypeMap(BuiltinTypes.Void)
      val methodReturn = methodReturnNode(enumDecl, returnType)
      val signature    = composeMethodLikeSignature(returnType)
      val modifiers = Seq(
        modifierNode(enumDecl, ModifierTypes.CONSTRUCTOR),
        modifierNode(enumDecl, ModifierTypes.INTERNAL),
        modifierNode(enumDecl, ModifierTypes.STATIC)
      )
      val name     = Defines.StaticInitMethodName
      val fullName = composeMethodFullName(typeDeclFullName, name, signature)
      val body = blockAst(
        blockNode(enumDecl),
        enumMemberDecls.zipWithIndex.map { case (memberDecl, ordinal) =>
          astForEnumMemberInitializer(memberDecl, enumTypeFullName, ordinal)
        }.toList
      )
      val methodNode_ = methodNode(enumDecl, name, name, fullName, Option(signature), relativeFileName)

      methodAst(methodNode_, Nil, body, methodReturn, modifiers) :: Nil
    }
  }

  private def astForEnumMemberInitializer(
    enumMemberDecl: DotNetNodeInfo,
    enumTypeFullName: String,
    ordinal: Int
  ): Ast = {
    val name = nameFromNode(enumMemberDecl)
    val explicitInitializer = Try(enumMemberDecl.json(ParserKeys.Initializer)).toOption
      .filterNot(_.isNull)
      .map(createDotNetNodeInfo)

    val rhs = explicitInitializer match {
      case Some(initializer) => astForEqualsValueClause(initializer)
      case None              => Seq(Ast(literalNode(enumMemberDecl, ordinal.toString, enumTypeFullName)))
    }
    val assignmentCode = explicitInitializer.map(_ => code(enumMemberDecl)).getOrElse(s"$name = $ordinal")
    val assignmentNode = callNode(
      enumMemberDecl,
      assignmentCode,
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(enumTypeFullName)
    )
    callAst(assignmentNode, Ast(identifierNode(enumMemberDecl, name, name, enumTypeFullName)) +: rhs)
  }

  /** Creates enum members. These are associated with integer types, and by default, are `int` types.
    * @see
    *   <a href="https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/enum">Enumeration
    *   Types</a>
    */
  protected def astForEnumMemberDeclaration(enumMemberDecl: DotNetNodeInfo): Seq[Ast] = {
    val name = nameFromNode(enumMemberDecl)
    val typeFullName = scope
      .peekScope()
      .collectFirst { case EnumScope(_, aliasFor) => aliasFor }
      .getOrElse(DotNetTypeMap(BuiltinTypes.Int))
    val member    = memberNode(enumMemberDecl, name, code(enumMemberDecl), typeFullName)
    val modifiers = astForModifiers(enumMemberDecl)

    val memberAst = Ast(member).withChildren(modifiers)
    Seq(memberAst)
  }

  protected def astForFieldDeclaration(fieldDecl: DotNetNodeInfo): Seq[Ast] = {
    val modifiers    = modifiersForNode(fieldDecl)
    val isStatic     = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val modifierAsts = modifiers.map(Ast(_))

    val declarationNode = createDotNetNodeInfo(fieldDecl.json(ParserKeys.Declaration))
    val declAsts        = astForVariableDeclaration(declarationNode, isStatic)

    val annotationAsts =
      Try(fieldDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    val memberNodes = declAsts
      .flatMap(_.nodes.collectFirst { case x: NewIdentifier => x })
      .map(x => memberNode(declarationNode, x.name, code(declarationNode), x.typeFullName))
    memberNodes.map(Ast(_).withChildren(annotationAsts).withChildren(modifierAsts))
  }

  protected def astForEventDeclaration(eventDecl: DotNetNodeInfo): Seq[Ast] = {
    val name         = explicitMemberName(eventDecl, nameFromNode(eventDecl))
    val typeFullName = nodeTypeFullName(eventDecl)
    val modifiers    = modifiersForNode(eventDecl)
    val isStatic     = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val modifierAsts = modifiers.map(Ast(_))
    val annotationAsts =
      Try(eventDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    scope.pushField(FieldDecl(name, typeFullName, isStatic, false, eventDecl))

    val memberAst = Ast(memberNode(eventDecl, name, code(eventDecl), typeFullName))
      .withChildren(annotationAsts)
      .withChildren(modifierAsts)

    val accessorList = createDotNetNodeInfo(eventDecl.json(ParserKeys.AccessorList))
    val accessors    = accessorList.json(ParserKeys.Accessors).arr.map(createDotNetNodeInfo)
    memberAst +: accessors.flatMap(astForEventAccessor(_, eventDecl)).toList
  }

  protected def astForLocalDeclarationStatement(localDecl: DotNetNodeInfo): Seq[Ast] = {
    astForVariableDeclaration(createDotNetNodeInfo(localDecl.json(ParserKeys.Declaration)))
  }

  protected def astForVariableDeclaration(varDecl: DotNetNodeInfo, isStatic: Boolean): Seq[Ast] = {
    val typeFullName = nodeTypeFullName(varDecl)

    varDecl
      .json(ParserKeys.Variables)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap { x =>
        val name    = nameFromNode(x)
        val hasInit = !x.json(ParserKeys.Initializer).isNull
        scope.pushField(FieldDecl(name, typeFullName, isStatic, hasInit, x))
        astForVariableDeclarator(x, typeFullName, shouldPushVariable = false)
      }
      .toSeq
  }

  protected def astForVariableDeclaration(varDecl: DotNetNodeInfo): Seq[Ast] = {
    val typeFullName = nodeTypeFullName(varDecl)
    varDecl
      .json(ParserKeys.Variables)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap(astForVariableDeclarator(_, typeFullName))
      .toSeq
  }

  protected def astForVariableDeclarator(
    varDecl: DotNetNodeInfo,
    typeFullName: String,
    shouldPushVariable: Boolean = true
  ): Seq[Ast] = {
    val designation = Try(varDecl.json(ParserKeys.Designation)).toOption
      .filterNot(_.isNull)
      .map(createDotNetNodeInfo)
    if (designation.nonEmpty) {
      return astForDeconstructionVariableDeclarator(varDecl, designation.get, shouldPushVariable)
    }

    // Create RHS AST first to propagate types
    val initializerJson = varDecl.json(ParserKeys.Initializer)
    val rhs             = if (!initializerJson.isNull) astForNode(createDotNetNodeInfo(initializerJson)) else Seq.empty
    val rhsTypeFullName =
      if (typeFullName == Defines.Any || typeFullName == "var") getTypeFullNameFromAstNode(rhs)
      else scope.tryResolveTypeReference(typeFullName).map(_.name).getOrElse(typeFullName)

    val name          = nameFromNode(varDecl)
    val identifierAst = astForIdentifier(varDecl, rhsTypeFullName)
    val _localNode    = localNode(varDecl, name, name, rhsTypeFullName)
    val localNodeAst  = Ast(_localNode)

    if (shouldPushVariable) {
      scope.addToScope(name, _localNode)
    }

    if (initializerJson.isNull) {
      val assignmentNode = callNode(
        varDecl,
        code(varDecl),
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        None
      )
      // Implicitly assigned to `null`
      Seq(
        callAst(assignmentNode, Seq(identifierAst, Ast(literalNode(varDecl, BuiltinTypes.Null, BuiltinTypes.Null)))),
        localNodeAst
      )
    } else {
      val assignmentNode = callNode(
        varDecl,
        code(varDecl),
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(rhsTypeFullName)
      )

      Seq(callAst(assignmentNode, identifierAst +: rhs), localNodeAst)
    }
  }

  private def astForDeconstructionVariableDeclarator(
    varDecl: DotNetNodeInfo,
    designation: DotNetNodeInfo,
    shouldPushVariable: Boolean
  ): Seq[Ast] = {
    val initializerJson = varDecl.json(ParserKeys.Initializer)
    if (initializerJson.isNull) {
      return Seq.empty
    }

    val rhsNode  = createDotNetNodeInfo(createDotNetNodeInfo(initializerJson).json(ParserKeys.Value))
    val bindings = deconstructionBindings(designation)
    bindings.flatMap { case (designationNode, path) =>
      val name         = nameFromNode(designationNode)
      val typeFullName = Defines.Any
      val local        = localNode(designationNode, name, name, typeFullName)
      if (shouldPushVariable) {
        scope.addToScope(name, local)
      }
      val assignmentNode = callNode(
        varDecl,
        s"$name = ${rhsNode.code}.${path.mkString(".")}",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(typeFullName)
      )
      val lhsNode = identifierNode(designationNode, name, name, typeFullName)
      val lhs     = Ast(lhsNode).withRefEdge(lhsNode, local)
      val rhs     = deconstructionAccessAst(varDecl, rhsNode, path, typeFullName)
      Seq(callAst(assignmentNode, Seq(lhs, rhs)), Ast(local))
    }
  }

  private def deconstructionBindings(
    designation: DotNetNodeInfo,
    path: List[String] = Nil
  ): Seq[(DotNetNodeInfo, List[String])] = {
    designation.node match {
      case SingleVariableDesignation => Seq((designation, path))
      case DiscardPattern            => Seq.empty
      case TuplePattern | ParenthesizedVariableDesignation =>
        designation
          .json(ParserKeys.Patterns)
          .arr
          .map(createDotNetNodeInfo)
          .zipWithIndex
          .flatMap { case (child, idx) =>
            deconstructionBindings(child, path :+ s"Item${idx + 1}")
          }
          .toSeq
      case _ => Seq.empty
    }
  }

  private def deconstructionAccessAst(
    origin: DotNetNodeInfo,
    rhsNode: DotNetNodeInfo,
    path: List[String],
    typeFullName: String
  ): Ast = {
    path
      .foldLeft((astForExpression(rhsNode).headOption.getOrElse(Ast()), rhsNode.code)) {
        case ((baseAst, baseCode), memberName) =>
          val accessCode = s"$baseCode.$memberName"
          (fieldAccessAst(origin, origin, baseAst, accessCode, memberName, typeFullName), accessCode)
      }
      ._1
  }

  protected def astForConstructorDeclaration(constructorDecl: DotNetNodeInfo): Seq[Ast] = {
    val params = constructorDecl
      .json(ParserKeys.ParameterList)
      .obj(ParserKeys.Parameters)
      .arr
      .map(createDotNetNodeInfo)
      .zipWithIndex
      .map(astForParameter(_, _, None))
      .toSeq
    // TODO: Decide on proper return type for constructors. No `ReturnType` key in C# JSON for constructors so just
    //  defaulted to void (same as java) for now
    val methodReturn     = methodReturnNode(constructorDecl, DotNetTypeMap(BuiltinTypes.Void))
    val signature        = composeMethodLikeSignature(DotNetTypeMap(BuiltinTypes.Void), params)
    val typeDeclFullName = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace);

    val modifiers = (modifiersForNode(constructorDecl) :+ modifierNode(constructorDecl, ModifierTypes.CONSTRUCTOR))
      .filter(_.modifierType != ModifierTypes.INTERNAL)

    val isStaticConstructor = modifiers.exists(_.modifierType == ModifierTypes.STATIC)

    val (name, fullName) =
      if (isStaticConstructor)
        (Defines.StaticInitMethodName, composeMethodFullName(typeDeclFullName, Defines.StaticInitMethodName, signature))
      else
        (
          Defines.ConstructorMethodName,
          composeMethodFullName(typeDeclFullName, Defines.ConstructorMethodName, signature)
        )

    scope.pushNewScope(MethodScope(fullName))

    // 1. Do we have fields? Then we need to initialize them explicitly
    val (staticFields, dynamicFields) = scope.getFieldsInScope.partition(_.isStatic)

    val fieldInitializerAsts = if (isStaticConstructor && staticFields.nonEmpty) {
      // 2. If this has a static modifier, then we create a prefixAst list of the static field initializers
      astVariableDeclarationForInitializedFields(staticFields)
    } else if (dynamicFields.nonEmpty) {
      // 3. If this does not have a static modifier, then we create a prefixAst list of the dynamic field initializers
      astVariableDeclarationForInitializedFields(dynamicFields)
    } else {
      Seq.empty
    }

    val initializerAsts = Try(constructorDecl.json(ParserKeys.Initializer)).toOption
      .filterNot(_.isNull)
      .map(createDotNetNodeInfo)
      .map(astForConstructorInitializer)
      .getOrElse(Seq.empty)
    val prefixAsts = initializerAsts ++ fieldInitializerAsts

    val body = astForBlock(createDotNetNodeInfo(constructorDecl.json(ParserKeys.Body)), prefixAsts = prefixAsts.toList)

    scope.popScope()

    val methodNode_ =
      methodNode(constructorDecl, name, code(constructorDecl), fullName, Option(signature), relativeFileName)

    val thisNode =
      if (!isStaticConstructor) astForThisParameter(constructorDecl)
      else Ast()
    Seq(methodAst(methodNode_, thisNode +: params, body, methodReturn, modifiers))
  }

  protected def astForConstructorInitializer(initializer: DotNetNodeInfo): Seq[Ast] = {
    val argumentList = createDotNetNodeInfo(initializer.json(ParserKeys.ArgumentList))
    val arguments    = astForArgumentList(argumentList)
    val argTypes     = arguments.map(getTypeFullNameFromAstNode)
    val returnType   = DotNetTypeMap(BuiltinTypes.Void)
    val signature    = composeMethodLikeSignature(returnType, argTypes)
    val ownerFullName = initializer.node match {
      case ThisConstructorInitializer => scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
      case _                          => Defines.UnresolvedNamespace
    }
    val fullName = composeMethodFullName(ownerFullName, Defines.ConstructorMethodName, signature)
    val call = callNode(
      initializer,
      code(initializer),
      Defines.ConstructorMethodName,
      fullName,
      DispatchTypes.STATIC_DISPATCH,
      Option(signature),
      Option(returnType)
    )
    Seq(callAst(call, arguments))
  }

  protected def astForMethodDeclaration(
    methodDecl: DotNetNodeInfo,
    extraModifiers: List[NewModifier] = Nil
  ): Seq[Ast] = {
    val localName = nameFromNode(methodDecl)
    val name      = explicitMemberName(methodDecl, localName)
    val params = methodDecl
      .json(ParserKeys.ParameterList)
      .obj(ParserKeys.Parameters)
      .arr
      .map(createDotNetNodeInfo)
      .zipWithIndex
      .map(astForParameter(_, _, None))
      .toSeq

    val annotationAsts =
      Try(methodDecl.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)

    val methodReturnAstNode   = createDotNetNodeInfo(methodDecl.json(ParserKeys.ReturnType))
    val methodReturn          = methodReturnNode(methodReturnAstNode, nodeTypeFullName(methodReturnAstNode))
    val signature             = composeMethodLikeSignature(methodReturn.typeFullName, params)
    val methodNameForFullName = explicitMemberName(methodDecl, genericMethodName(methodDecl, localName))
    val fullNameBase = scope.surroundingScopeFullName match {
      case Some(fullName) => s"${withoutSignature(fullName)}.$methodNameForFullName"
      case _              => methodNameForFullName
    }
    val fullName = s"$fullNameBase:$signature"
    val methodNode_ = methodNode(
      methodDecl,
      name,
      code(methodDecl),
      fullName,
      Option(signature),
      relativeFileName,
      genericSignature = genericSignatureForDeclaration(methodDecl)
    )
    scope.pushNewScope(MethodScope(fullName))

    // In the case of interfaces, the method body may not be present
    val jsonBody = methodDecl.json(ParserKeys.Body)
    val body =
      if (!jsonBody.isNull && parseLevel == AstParseLevel.FULL_AST) astForBlock(createDotNetNodeInfo(jsonBody))
      else Ast(blockNode(methodDecl)) // Creates an empty block
    scope.popScope()
    val modifiers = modifiersForNode(methodDecl) ++ extraModifiers
    val thisNode =
      if (!modifiers.exists(_.modifierType == ModifierTypes.STATIC)) astForThisParameter(methodDecl)
      else Ast()
    Seq(methodAstWithAnnotations(methodNode_, thisNode +: params, body, methodReturn, modifiers, annotationAsts))
  }

  private def genericMethodName(methodDecl: DotNetNodeInfo, name: String): String = {
    val typeParameterPattern = s"\\b${Pattern.quote(name)}\\s*(<[^>]+>)".r
    typeParameterPattern
      .findFirstMatchIn(code(methodDecl))
      .map(m => s"$name${m.group(1).replaceAll("\\s+", "")}")
      .getOrElse(name)
  }

  private def genericSignatureForDeclaration(decl: DotNetNodeInfo): Option[String] = {
    val typeParameterList =
      Try(decl.json(ParserKeys.TypeParameterList)).toOption
        .collect { case typeParameterList: ujson.Obj => createDotNetNodeInfo(typeParameterList).code }
        .map(_.replaceAll("\\s+", ""))
        .filter(_.nonEmpty)

    val constraints =
      Try(decl.json(ParserKeys.ConstraintClauses).arr.map(createDotNetNodeInfo).map(_.code.replaceAll("\\s+", " ")))
        .getOrElse(Seq.empty)

    val signature = (typeParameterList.toSeq ++ constraints).mkString(" ").trim
    Option.when(signature.nonEmpty)(signature)
  }

  private def explicitInterfaceName(decl: DotNetNodeInfo): Option[String] = {
    Try(decl.json(ParserKeys.ExplicitInterfaceSpecifier)).toOption
      .filterNot(_.isNull)
      .map(createDotNetNodeInfo)
      .map(nameFromNode)
  }

  private def explicitMemberName(decl: DotNetNodeInfo, localName: String): String = {
    explicitInterfaceName(decl).map(prefix => s"$prefix.$localName").getOrElse(localName)
  }

  private def astForParameter(paramNode: DotNetNodeInfo, idx: Int, paramTypeHint: Option[String] = None): Ast = {
    val name         = nameFromNode(paramNode)
    val modifiers    = explicitModifiersForNode(paramNode)
    val modifierType = modifiers.map(_.modifierType)
    val isVariadic   = modifierType.contains(CSharpModifiers.PARAMS)
    val typeFullName = paramTypeHint.getOrElse(nodeTypeFullName(paramNode))
    val evaluationStrategy =
      if (modifierType.exists(Set(CSharpModifiers.REF, CSharpModifiers.OUT, CSharpModifiers.IN))) {
        EvaluationStrategies.BY_REFERENCE.name
      } else {
        EvaluationStrategies.BY_SHARING.name
      }
    val param =
      parameterInNode(paramNode, name, code(paramNode), idx + 1, isVariadic, evaluationStrategy, Option(typeFullName))
    val annotationAsts =
      Try(paramNode.json(ParserKeys.AttributeLists))
        .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
        .getOrElse(Seq.empty)
    scope.addToScope(name, param)
    Ast(param).withChildren(annotationAsts).withChildren(modifiers.map(Ast(_)))
  }

  private def astForThisParameter(methodDecl: DotNetNodeInfo): Ast = {
    val name         = Constants.This
    val typeFullName = scope.surroundingTypeDeclFullName.getOrElse(Defines.Any)
    val param = parameterInNode(methodDecl, name, name, 0, false, EvaluationStrategies.BY_SHARING.name, typeFullName)
    Ast(param)
  }

  protected def astForThisReceiver(invocationExpr: DotNetNodeInfo, typeFullName: Option[String] = None): Ast = {
    val name = Constants.This
    val param = identifierNode(
      invocationExpr,
      name,
      name,
      typeFullName.orElse(scope.surroundingTypeDeclFullName).getOrElse(Defines.Any)
    )
    Ast(param)
  }

  protected def astForBaseReceiver(invocationExpr: DotNetNodeInfo): Ast = {
    val name  = Constants.Base
    val param = identifierNode(invocationExpr, name, name, currentBaseTypeFullName.getOrElse(Defines.Any))
    Ast(param)
  }

  protected def astForBlock(
    body: DotNetNodeInfo,
    code: Option[String] = None,
    prefixAsts: List[Ast] = List.empty
  ): Ast = {
    val block = blockNode(body)
    code.foreach(block.code(_))
    scope.pushNewScope(BlockScope)
    val statements = Try(body.json(ParserKeys.Statements)).toOption match {
      case Some(value: ujson.Arr) => astsForBlockStatements(value.arr.map(createDotNetNodeInfo).toList)
      case _                      => List.empty
    }
    val _blockAst = blockAst(block, prefixAsts ++ statements)
    scope.popScope()
    _blockAst
  }

  private def astsForBlockStatements(statements: List[DotNetNodeInfo]): List[Ast] = statements match {
    case Nil => List.empty
    case statement :: rest if isUsingLocalDeclaration(statement) =>
      astForUsingDeclarationStatement(statement, rest)
    case statement :: rest =>
      astForNode(statement).toList ++ astsForBlockStatements(rest)
  }

  private def isUsingLocalDeclaration(statement: DotNetNodeInfo): Boolean = {
    statement.node == LocalDeclarationStatement &&
    (Try(statement.json(ParserKeys.Using).bool).getOrElse(false) ||
      statement.code.trim.startsWith("using ") ||
      statement.code.trim.startsWith("await using "))
  }

  private def astForUsingDeclarationStatement(usingDecl: DotNetNodeInfo, rest: List[DotNetNodeInfo]): List[Ast] = {
    val declAst    = astForLocalDeclarationStatement(usingDecl)
    val bodyAsts   = astsForBlockStatements(rest)
    val tryNode    = controlStructureNode(usingDecl, ControlStructureTypes.TRY, usingDecl.code)
    val tryBodyAst = blockAst(blockNode(usingDecl, "try", Defines.Any), bodyAsts)
    val finallyAst = finallyAstForUsingDisposals(usingDecl, declAst, isAwaitUsingDeclaration(usingDecl))

    (declAst :+ tryCatchAst(tryNode, tryBodyAst, Seq.empty, finallyAst)).toList
  }

  private def isAwaitUsingDeclaration(usingDecl: DotNetNodeInfo): Boolean =
    Try(usingDecl.json(ParserKeys.Await).bool).getOrElse(false) || usingDecl.code.trim.startsWith("await using ")

  /** Parses the modifier array and handles implicit defaults.
    * @see
    *   https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/classes-and-structs/access-modifiers
    */
  private def astForModifiers(declaration: DotNetNodeInfo): Seq[Ast] = {
    modifiersForNode(declaration).map(Ast(_))
  }

  private def modifiersForNode(node: DotNetNodeInfo): Seq[NewModifier] = {
    val explicitModifiers = explicitModifiersForNode(node)
    val accessModifiers = explicitModifiers.map(_.modifierType) intersect List(
      ModifierTypes.PUBLIC,
      ModifierTypes.PRIVATE,
      ModifierTypes.INTERNAL,
      ModifierTypes.PROTECTED,
      CSharpModifiers.CONST,
      CSharpModifiers.FILE
    )
    val implicitAccessModifier = accessModifiers match {
      // Internal is default for top-level definitions
      case Nil if scope.isTopLevel => modifierNode(node, ModifierTypes.INTERNAL) :: Nil
      // Private is default for nested definitions
      case Nil => modifierNode(node, ModifierTypes.PRIVATE) :: Nil
      case _   => Nil
    }

    implicitAccessModifier ++ explicitModifiers
  }

  private def explicitModifiersForNode(node: DotNetNodeInfo): Seq[NewModifier] =
    node.json(ParserKeys.Modifiers).arr.flatMap(readModifier(node, _)).toList

  private def readModifier(node: DotNetNodeInfo, modifier: ujson.Value): Option[NewModifier] = {
    Option {
      modifier(ParserKeys.Value).str match {
        case "public"    => modifierNode(node, ModifierTypes.PUBLIC)
        case "private"   => modifierNode(node, ModifierTypes.PRIVATE)
        case "internal"  => modifierNode(node, ModifierTypes.INTERNAL)
        case "static"    => modifierNode(node, ModifierTypes.STATIC)
        case "readonly"  => modifierNode(node, ModifierTypes.READONLY)
        case "virtual"   => modifierNode(node, ModifierTypes.VIRTUAL)
        case "sealed"    => modifierNode(node, ModifierTypes.FINAL)
        case "abstract"  => modifierNode(node, ModifierTypes.ABSTRACT)
        case "protected" => modifierNode(node, ModifierTypes.PROTECTED)
        case "extern"    => modifierNode(node, ModifierTypes.NATIVE)
        case "const"     => modifierNode(node, CSharpModifiers.CONST)
        case "async"     => modifierNode(node, CSharpModifiers.ASYNC)
        case "override"  => modifierNode(node, CSharpModifiers.OVERRIDE)
        case "file"      => modifierNode(node, CSharpModifiers.FILE)
        case "in"        => modifierNode(node, CSharpModifiers.IN)
        case "new"       => modifierNode(node, CSharpModifiers.NEW)
        case "out"       => modifierNode(node, CSharpModifiers.OUT)
        case "params"    => modifierNode(node, CSharpModifiers.PARAMS)
        case "partial"   => modifierNode(node, CSharpModifiers.PARTIAL)
        case "ref"       => modifierNode(node, CSharpModifiers.REF)
        case "required"  => modifierNode(node, CSharpModifiers.REQUIRED)
        case "scoped"    => modifierNode(node, CSharpModifiers.SCOPED)
        case "struct"    => modifierNode(node, CSharpModifiers.STRUCT)
        case "this"      => modifierNode(node, CSharpModifiers.THIS)
        case "unsafe"    => modifierNode(node, CSharpModifiers.UNSAFE)
        case "volatile"  => modifierNode(node, CSharpModifiers.VOLATILE)
        case x =>
          logger.warn(s"Unhandled modifier name '$x'")
          null
      }
    }
  }

  protected def astVariableDeclarationForInitializedFields(fieldDecls: Seq[FieldDecl]): Seq[Ast] = {
    fieldDecls.filter(_.isInitialized).flatMap { case FieldDecl(name, typeFullName, _, isInitialized, node) =>
      astForVariableDeclarator(node, nodeTypeFullName(node), shouldPushVariable = false)
    }
  }

  protected def astForPropertyDeclaration(propertyDecl: DotNetNodeInfo): Seq[Ast] = {
    val name         = explicitMemberName(propertyDecl, nameFromNode(propertyDecl))
    val typeFullName = nodeTypeFullName(propertyDecl)
    val typeCode = Try(propertyDecl.json(ParserKeys.Type))
      .map(typeJson => code(createDotNetNodeInfo(typeJson)))
      .getOrElse(typeFullName)
    val modifierCode = Try {
      propertyDecl
        .json(ParserKeys.Modifiers)
        .arr
        .map(_(ParserKeys.Value).str)
        .mkString(" ")
    }.getOrElse("")
    val memberCode = Seq(modifierCode, typeCode, name).filter(_.nonEmpty).mkString(" ")

    val modifierAsts = modifiersForNode(propertyDecl).map(Ast(_))
    val memberAst    = Ast(memberNode(propertyDecl, name, memberCode, typeFullName)).withChildren(modifierAsts)

    val accessorList = createDotNetNodeInfo(propertyDecl.json(ParserKeys.AccessorList))
    val accessors    = accessorList.json(ParserKeys.Accessors).arr.map(createDotNetNodeInfo)
    memberAst +: accessors.flatMap(astForPropertyAccessor(_, propertyDecl)).toList
  }

  protected def astForIndexerDeclaration(indexerDecl: DotNetNodeInfo): Seq[Ast] = {
    val name         = explicitMemberName(indexerDecl, nameFromNode(indexerDecl))
    val typeFullName = nodeTypeFullName(indexerDecl)
    val modifierAsts = modifiersForNode(indexerDecl).map(Ast(_))
    val memberAst    = Ast(memberNode(indexerDecl, name, code(indexerDecl), typeFullName)).withChildren(modifierAsts)

    val accessorList = createDotNetNodeInfo(indexerDecl.json(ParserKeys.AccessorList))
    val accessors    = accessorList.json(ParserKeys.Accessors).arr.map(createDotNetNodeInfo)
    memberAst +: accessors.flatMap(astForIndexerAccessor(_, indexerDecl)).toList
  }

  private def astForIndexerAccessor(accessorDecl: DotNetNodeInfo, indexerDecl: DotNetNodeInfo): Seq[Ast] = {
    accessorDecl.node match {
      case GetAccessorDeclaration => astForIndexerGetAccessorDeclaration(accessorDecl, indexerDecl)
      case SetAccessorDeclaration => astForIndexerSetAccessorDeclaration(accessorDecl, indexerDecl)
      case _ =>
        logger.warn(s"Unhandled indexer accessor '${accessorDecl.node}'")
        Nil
    }
  }

  private def astForIndexerGetAccessorDeclaration(
    accessorDecl: DotNetNodeInfo,
    indexerDecl: DotNetNodeInfo
  ): Seq[Ast] = {
    val name         = composeGetterName(explicitMemberName(indexerDecl, nameFromNode(indexerDecl)))
    val modifiers    = modifiersForNode(indexerDecl)
    val returnType   = nodeTypeFullName(indexerDecl)
    val baseType     = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
    val indexParams  = astForIndexerParameters(indexerDecl)
    val isStatic     = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val parameters   = Option.unless(isStatic)(astForThisParameter(indexerDecl)).toList ++ indexParams
    val signature    = composeMethodLikeSignature(returnType, parameters)
    val fullName     = composeMethodFullName(baseType, name, signature)
    val body         = astForOptionalAccessorBody(accessorDecl)
    val methodReturn = methodReturnNode(accessorDecl, returnType)
    val methodNode_  = methodNode(accessorDecl, name, fullName, signature, relativeFileName)

    methodAst(methodNode_, parameters, body, methodReturn, modifiers) :: Nil
  }

  private def astForIndexerSetAccessorDeclaration(
    accessorDecl: DotNetNodeInfo,
    indexerDecl: DotNetNodeInfo
  ): Seq[Ast] = {
    val name        = composeSetterName(explicitMemberName(indexerDecl, nameFromNode(indexerDecl)))
    val modifiers   = modifiersForNode(indexerDecl)
    val returnType  = BuiltinTypes.Void
    val valueType   = nodeTypeFullName(indexerDecl)
    val baseType    = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
    val indexParams = astForIndexerParameters(indexerDecl)
    val isStatic    = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val valueParam = Ast(
      NewMethodParameterIn()
        .typeFullName(valueType)
        .name("value")
        .code("value")
        .index(indexParams.size + 1)
        .evaluationStrategy(EvaluationStrategies.BY_SHARING.name)
        .isVariadic(false)
    )
    val parameters   = Option.unless(isStatic)(astForThisParameter(indexerDecl)).toList ++ indexParams :+ valueParam
    val signature    = composeMethodLikeSignature(returnType, parameters)
    val fullName     = composeMethodFullName(baseType, name, signature)
    val body         = astForOptionalAccessorBody(accessorDecl)
    val methodReturn = methodReturnNode(accessorDecl, returnType)
    val methodNode_  = methodNode(accessorDecl, name, fullName, signature, relativeFileName)

    methodAst(methodNode_, parameters, body, methodReturn, modifiers) :: Nil
  }

  private def astForIndexerParameters(indexerDecl: DotNetNodeInfo): Seq[Ast] = {
    indexerDecl
      .json(ParserKeys.ParameterList)
      .obj(ParserKeys.Parameters)
      .arr
      .map(createDotNetNodeInfo)
      .zipWithIndex
      .map(astForParameter(_, _, None))
      .toSeq
  }

  private def astForOptionalAccessorBody(accessorDecl: DotNetNodeInfo): Ast = {
    Try(accessorDecl.json(ParserKeys.Body)).toOption
      .collect { case body: ujson.Obj => astForBlock(createDotNetNodeInfo(body)) }
      .getOrElse(Ast(blockNode(accessorDecl)))
  }

  private def astForEventAccessor(accessorDecl: DotNetNodeInfo, eventDecl: DotNetNodeInfo): Seq[Ast] = {
    accessorDecl.node match {
      case AddAccessorDeclaration    => astForEventAccessorDeclaration(accessorDecl, eventDecl, "add")
      case RemoveAccessorDeclaration => astForEventAccessorDeclaration(accessorDecl, eventDecl, "remove")
      case _ =>
        logger.warn(s"Unhandled event accessor '${accessorDecl.node}'")
        Nil
    }
  }

  private def astForEventAccessorDeclaration(
    accessorDecl: DotNetNodeInfo,
    eventDecl: DotNetNodeInfo,
    prefix: String
  ): Seq[Ast] = {
    val eventName  = explicitMemberName(eventDecl, nameFromNode(eventDecl))
    val name       = s"${prefix}_$eventName"
    val modifiers  = modifiersForNode(eventDecl)
    val returnType = DotNetTypeMap(BuiltinTypes.Void)
    val valueType  = nodeTypeFullName(eventDecl)
    val baseType   = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
    val isStatic   = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val valueParam = Ast(
      NewMethodParameterIn()
        .typeFullName(valueType)
        .name("value")
        .code("value")
        .index(1)
        .evaluationStrategy(EvaluationStrategies.BY_SHARING.name)
        .isVariadic(false)
    )
    val parameters   = Option.unless(isStatic)(astForThisParameter(eventDecl)).toList :+ valueParam
    val signature    = composeMethodLikeSignature(returnType, parameters)
    val fullName     = composeMethodFullName(baseType, name, signature)
    val body         = astForOptionalAccessorBody(accessorDecl)
    val methodReturn = methodReturnNode(accessorDecl, returnType)
    val methodNode_  = methodNode(accessorDecl, name, fullName, signature, relativeFileName)

    methodAst(methodNode_, parameters, body, methodReturn, modifiers) :: Nil
  }

  private def astForPropertyAccessor(accessorDecl: DotNetNodeInfo, propertyDecl: DotNetNodeInfo): Seq[Ast] = {
    accessorDecl.node match {
      case GetAccessorDeclaration => astForGetAccessorDeclaration(accessorDecl, propertyDecl)
      case SetAccessorDeclaration => astForSetAccessorDeclaration(accessorDecl, propertyDecl)
      case _ =>
        logger.warn(s"Unhandled property accessor '${accessorDecl.node}'")
        Nil
    }
  }

  private def astForSetAccessorDeclaration(accessorDecl: DotNetNodeInfo, propertyDecl: DotNetNodeInfo): Seq[Ast] = {
    val name         = composeSetterName(explicitMemberName(propertyDecl, nameFromNode(propertyDecl)))
    val modifiers    = modifiersForNode(propertyDecl)
    val returnType   = BuiltinTypes.Void
    val valueType    = nodeTypeFullName(propertyDecl)
    val baseType     = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
    val isStatic     = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val valueParam   = Ast(NewMethodParameterIn().typeFullName(valueType).name("value").index(1))
    val parameters   = Option.unless(isStatic)(astForThisParameter(propertyDecl)).toList :+ valueParam
    val signature    = composeMethodLikeSignature(returnType, parameters)
    val fullName     = composeMethodFullName(baseType, name, signature)
    val body         = astForOptionalAccessorBody(accessorDecl)
    val methodReturn = methodReturnNode(accessorDecl, returnType)
    val methodNode_  = methodNode(accessorDecl, name, fullName, signature, relativeFileName)

    methodAst(methodNode_, parameters, body, methodReturn, modifiers) :: Nil
  }

  private def astForGetAccessorDeclaration(accessorDecl: DotNetNodeInfo, propertyDecl: DotNetNodeInfo): Seq[Ast] = {
    val name         = composeGetterName(explicitMemberName(propertyDecl, nameFromNode(propertyDecl)))
    val modifiers    = modifiersForNode(propertyDecl)
    val returnType   = nodeTypeFullName(propertyDecl)
    val baseType     = scope.surroundingTypeDeclFullName.getOrElse(Defines.UnresolvedNamespace)
    val isStatic     = modifiers.exists(_.modifierType == ModifierTypes.STATIC)
    val parameters   = if (isStatic) Nil else astForThisParameter(propertyDecl) :: Nil
    val signature    = composeMethodLikeSignature(returnType, parameters)
    val fullName     = composeMethodFullName(baseType, name, signature)
    val body         = astForOptionalAccessorBody(accessorDecl)
    val methodReturn = methodReturnNode(accessorDecl, returnType)
    val methodNode_  = methodNode(accessorDecl, name, fullName, signature, relativeFileName)

    methodAst(methodNode_, parameters, body, methodReturn, modifiers) :: Nil
  }

  /** Creates an AST for a simple `x => { ... }` style lambda expression
    *
    * @param lambdaExpression
    *   the expression.
    * @param paramTypeHint
    *   a type that could hint at what the parameter type may be.
    */
  protected def astForSimpleLambdaExpression(
    lambdaExpression: DotNetNodeInfo,
    paramTypeHint: Option[String] = None
  ): Seq[Ast] = {
    // Create method declaration
    val name = nextClosureName()
    val fullName = {
      val baseType  = withoutSignature(scope.surroundingScopeFullName.getOrElse(Defines.UnresolvedNamespace))
      val signature = Defines.UnresolvedSignature
      composeMethodFullName(baseType, name, signature)
    }
    // Set parameter type if necessary, which may require the type hint
    val paramType = paramTypeHint.flatMap(AstCreatorHelper.elementTypesFromCollectionType).headOption
    val paramAsts = Try(lambdaExpression.json(ParserKeys.Parameter)).toOption match {
      case Some(parameterObj: ujson.Obj) =>
        Seq(astForParameter(createDotNetNodeInfo(parameterObj), 0, paramType))
      case _ =>
        lambdaExpression
          .json(ParserKeys.ParameterList)
          .obj(ParserKeys.Parameters)
          .arr
          .map(createDotNetNodeInfo)
          .zipWithIndex
          .map(astForParameter(_, _, paramType))
          .toSeq
    }

    scope.pushNewScope(MethodScope(fullName))
    // Handle lambda body
    val bodyJson = createDotNetNodeInfo(lambdaExpression.json(ParserKeys.Body))
    val block    = blockNode(bodyJson)
    scope.pushNewScope(BlockScope)
    val body =
      if (this.parseLevel == AstParseLevel.SIGNATURES) Seq.empty else astForNode(bodyJson)
    val blockAst_ = blockAst(block, body.toList)
    scope.popScope()
    scope.popScope()
    val method = methodNode(
      lambdaExpression,
      name,
      code(lambdaExpression),
      fullName,
      None,
      relativeFileName,
      Option(NodeTypes.METHOD),
      scope.surroundingScopeFullName
    )
    val modifiers = astForModifiers(lambdaExpression).flatMap(_.nodes).collect { case x: NewModifier => x }
    val lambdaReturnType = body.lastOption
      .getOrElse(Ast())
      .nodes
      .filter {
        case x: NewCall => !x.name.startsWith("<operator")
        case _          => true
      }
      .map(Ast.apply)
      .map(getTypeFullNameFromAstNode)
      .headOption
      .getOrElse(Defines.Any)
    val methodReturn = methodReturnNode(lambdaExpression, lambdaReturnType)
    Ast.storeInDiffGraph(methodAst(method, paramAsts, blockAst_, methodReturn, modifiers), diffGraph)
    // Create type decl
    val lambdaTypeDecl = typeDeclNode(lambdaExpression, name, fullName, relativeFileName, code(lambdaExpression))
    scope.surroundingScopeFullName.foreach { fn =>
      lambdaTypeDecl.astParentFullName(fn).astParentType(NodeTypes.METHOD)
    }
    Ast.storeInDiffGraph(Ast(lambdaTypeDecl), diffGraph)
    // Create method ref
    val methodRef = methodRefNode(lambdaExpression, code(lambdaExpression), fullName, lambdaReturnType)
    Ast(methodRef) :: Nil
  }

  def astForAnonymousObjectCreationExpression(anonObjExpr: DotNetNodeInfo): Seq[Ast] = {
    val typeDeclName     = nextAnonymousTypeName()
    val typeDeclFullName = s"${withoutSignature(scope.surroundingScopeFullName.getOrElse(Defines.Any))}.${typeDeclName}"

    val _typeDeclNode = typeDeclNode(
      anonObjExpr,
      typeDeclName,
      typeDeclFullName,
      relativeFileName,
      code(anonObjExpr),
      astParentType = NodeTypes.METHOD,
      astParentFullName = scope.surroundingScopeFullName.getOrElse(Defines.Any)
    )

    scope.pushNewScope(TypeScope(typeDeclFullName))

    val memberAsts = anonObjExpr
      .json(ParserKeys.Initializers)
      .arr
      .map(createDotNetNodeInfo)
      .map(astForAnonymousObjectMemberDeclarator)
      .toSeq

    scope.popScope()
    Ast.storeInDiffGraph(Ast(_typeDeclNode).withChildren(memberAsts), diffGraph)

    val _typeRefNode = typeRefNode(anonObjExpr, code(anonObjExpr), typeDeclFullName)
    Ast(_typeRefNode) :: Nil
  }

  private def astForAnonymousObjectMemberDeclarator(memberDeclarator: DotNetNodeInfo): Ast = {
    val rhsNode         = createDotNetNodeInfo(memberDeclarator.json(ParserKeys.Expression))
    val rhsAst          = astForNode(rhsNode)
    val rhsTypeFullName = getTypeFullNameFromAstNode(rhsAst)

    val lhsNode = Try(
      createDotNetNodeInfo(memberDeclarator.json(ParserKeys.NameEquals)(ParserKeys.Name))
    ).toOption match {
      case Some(lhs) => Option(lhs)
      case None      => None
    }

    val lhsAst = lhsNode match {
      case Some(node) => astForNode(node)
      case _          => Seq.empty[Ast]
    }

    val name = lhsNode match {
      case Some(node) => nameFromNode(node)
      case _          => nameFromNode(rhsNode)
    }

    val memberType = rhsTypeFullName match {
      case Defines.Any => getTypeFullNameFromAstNode(lhsAst)
      case otherType   => otherType
    }

    val _memberNode = memberNode(memberDeclarator, name, code(memberDeclarator), memberType)

    Ast(_memberNode)
  }

}
