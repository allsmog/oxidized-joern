package io.joern.javasrc2cpg.oxidized

import io.joern.javasrc2cpg.Config
import io.joern.javasrc2cpg.parser.JavaAstJsonParser.{JavaAstDocument, JavaAstNode}
import io.joern.x2cpg.AstNodeBuilder.closureBindingNode
import io.joern.x2cpg.{Ast, AstCreatorBase, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DiffGraphBuilder,
  DispatchTypes,
  EvaluationStrategies,
  ModifierTypes,
  NodeTypes,
  Operators
}
import io.shiftleft.semanticcpg.language.types.structure.NamespaceTraversal

import scala.collection.mutable

final class OxidizedAstCreator(document: JavaAstDocument, config: Config)
    extends AstCreatorBase[JavaAstNode, OxidizedAstCreator](document.relativeName)(config.schemaValidation) {

  private implicit val validationMode: ValidationMode = config.schemaValidation

  private val usedTypeNames: mutable.Set[String]           = mutable.Set(Defines.Any, "void", "java.lang.Object")
  private val memberTypeNames: mutable.Map[String, String] = mutable.Map.empty
  private val memberTypeNamesByType: mutable.Map[(String, String), String]                   = mutable.Map.empty
  private val methodSignatureInfos: mutable.Map[String, MethodSignatureInfo]                 = mutable.Map.empty
  private val methodSignatureInfosByType: mutable.Map[(String, String), MethodSignatureInfo] = mutable.Map.empty
  private val inheritedTypeNamesByType: mutable.Map[String, Seq[String]]                     = mutable.Map.empty
  private val functionalInterfaceInfos: mutable.Map[String, FunctionalInterfaceInfo]         = mutable.Map.empty
  private val recordParameterInfosByType: mutable.Map[String, List[RecordParameterInfo]]     = mutable.Map.empty
  private val localRecordCaptureInfosByType: mutable.Map[String, Seq[CaptureInfo]]           = mutable.Map.empty
  private val anonymousOuterClassTypes: mutable.Set[String]                                  = mutable.Set.empty
  private val anonymousTypeCounters: mutable.Map[(String, String), Int]                      = mutable.Map.empty
  private val enumAnonymousTypes: mutable.Map[(Int, Int), String]                            = mutable.Map.empty
  private val importAliases: mutable.Map[String, String]                                     = mutable.Map.empty
  private val wildcardImports: mutable.Set[String]                                           = mutable.Set.empty
  private val moduleImports: mutable.Set[String]                                             = mutable.Set.empty
  private var localScopes: List[mutable.Map[String, String]]                                 = Nil
  private var localRefScopes: List[mutable.Map[String, NewLocal]]                            = Nil
  private var localDeclarationScopes: List[mutable.Map[String, List[NewLocal]]]              = Nil
  private var parameterRefScopes: List[mutable.Map[String, NewNode]]                         = Nil
  private var currentTypeFullNames: List[String]                                             = Nil
  private var currentTypeInherits: List[Seq[String]]                                         = Nil
  private var currentMethodFullNames: List[String]                                           = Nil
  private var currentMethodReturnTypes: List[String]                                         = Nil
  private var expectedExpressionTypes: List[Option[String]]                                  = Nil
  private var anonymousOwnerBases: List[String]                                              = Nil
  private var currentPackageName: Option[String]                                             = None
  private var pendingLambdaAsts: List[Ast]                                                   = Nil
  private var pendingAnonymousTypeAsts: List[Ast]                                            = Nil
  private var pendingPatternLocals: List[PatternLocalInfo]                                   = Nil
  private var lambdaCounter: Int                                                             = 0
  private var objectCreationTempCounter: Int                                                 = 0

  def usedTypes(): Set[String] = usedTypeNames.toSet

  override def createAst(): DiffGraphBuilder = {
    val fileNode = NewFile()
      .name(document.relativeName)
      .order(0)
    Option.when(!config.disableFileContent)(document.ast.code).foreach(fileNode.content)
    val ast = astForProgram(document.ast)
    storeInDiffGraph(ast)
    diffGraph.addNode(fileNode)
    diffGraph
  }

  protected def line(node: JavaAstNode): Option[Int]      = Some(node.start.line)
  protected def column(node: JavaAstNode): Option[Int]    = Some(node.start.column)
  protected def lineEnd(node: JavaAstNode): Option[Int]   = Some(node.end.line)
  protected def columnEnd(node: JavaAstNode): Option[Int] = Some(node.end.column)
  protected def code(node: JavaAstNode): String           = node.code

  override protected def offset(node: JavaAstNode): Option[(Int, Int)] =
    Some((node.startByte.toInt, node.endByte.toInt))

  private def storeInDiffGraph(ast: Ast): Unit = {
    Ast.storeInDiffGraph(ast, diffGraph)
  }

  private def astForProgram(root: JavaAstNode): Ast = {
    val packageName    = packageNameFor(root)
    val namespaceBlock = namespaceBlockFor(root, packageName)
    currentPackageName = packageName
    val importAsts = root.children
      .filter(_.kind == "import_declaration")
      .map(astForImportDeclaration)
    registerTopLevelTypeAliases(root, packageName)
    val typeAsts = root.children
      .filter(node => TypeDeclarationKinds.contains(node.kind))
      .map(astForTypeDeclaration(_, packageName))
    Ast(namespaceBlock).withChildren(typeAsts).withChildren(importAsts)
  }

  private def namespaceBlockFor(root: JavaAstNode, packageName: Option[String]): NewNamespaceBlock = {
    packageName match {
      case Some(name) =>
        NewNamespaceBlock()
          .name(name)
          .fullName(s"${document.relativeName}:$name")
          .filename(document.relativeName)
          .lineNumber(line(root))
          .columnNumber(column(root))
      case None =>
        globalNamespaceBlock()
    }
  }

  private def astForImportDeclaration(node: JavaAstNode): Ast = {
    val importCode = node.code.stripSuffix(";").trim
    val isModule   = importCode.stripPrefix("import").trim.startsWith("module ")
    val importedEntity = namedChildren(node)
      .find(child => child.kind == "scoped_identifier" || child.kind == "identifier")
      .map(_.code)
      .getOrElse(node.code.stripPrefix("import").stripSuffix(";").trim)
      .stripPrefix("static ")
      .stripPrefix("module ")
      .trim
    val isWildcard = node.children.exists(_.kind == "asterisk")
    val isStatic   = node.children.exists(_.kind == "static")
    val importedAs =
      if (isWildcard) "*"
      else importedEntity.split('.').lastOption.getOrElse(importedEntity)
    val importNode = NewImport()
      .importedAs(importedAs)
      .importedEntity(importedEntity)
      .code(node.code.stripSuffix(";"))
      .isWildcard(isWildcard)
      .isModuleImport(isModule)

    if (isModule) {
      moduleImports += importedEntity
      registerType(importedEntity)
    } else if (isWildcard) {
      wildcardImports += importedEntity
    } else {
      registerType(importedEntity)
      if (!isStatic) {
        importAliases.update(importedAs, importedEntity)
      }
    }
    Ast(importNode)
  }

  private def registerTopLevelTypeAliases(root: JavaAstNode, packageName: Option[String]): Unit = {
    root.children
      .filter(node => TypeDeclarationKinds.contains(node.kind))
      .foreach { typeDecl =>
        childByField(typeDecl, "name").foreach { nameNode =>
          val fullName = packageName.filter(_.nonEmpty).map(pkg => s"$pkg.${nameNode.code}").getOrElse(nameNode.code)
          importAliases.update(nameNode.code, fullName)
        }
      }
  }

  private def inheritsForTypeDeclaration(node: JavaAstNode): Seq[String] = {
    val inherits = node.kind match {
      case "class_declaration" =>
        val superclass = childByField(node, "superclass").toSeq.flatMap(inheritedTypeNames)
        val interfaces = childByField(node, "interfaces").toSeq.flatMap(inheritedTypeNames)
        val baseTypes  = if (superclass.nonEmpty) superclass else Seq("java.lang.Object")
        baseTypes ++ interfaces
      case "interface_declaration" =>
        val interfaces = namedChildren(node).filter(_.kind == "extends_interfaces").flatMap(inheritedTypeNames)
        if (interfaces.nonEmpty) interfaces else Seq("java.lang.Object")
      case "enum_declaration" =>
        Seq("java.lang.Enum")
      case "record_declaration" =>
        Seq("java.lang.Record")
      case _ =>
        Seq.empty
    }
    inherits.distinct
  }

  private def inheritedTypeNames(node: JavaAstNode): Seq[String] = {
    namedChildren(node).flatMap {
      case typeNode if InheritedTypeKinds.contains(typeNode.kind) => Seq(typeName(typeNode))
      case wrapper                                                => inheritedTypeNames(wrapper)
    }
  }

  private def typeBodyChildren(node: JavaAstNode): List[JavaAstNode] = {
    childByField(node, "body")
      .map { body =>
        body.children.flatMap {
          case enumDeclarations if enumDeclarations.kind == "enum_body_declarations" => enumDeclarations.children
          case child                                                                 => Seq(child)
        }
      }
      .getOrElse(Nil)
  }

  private def astForTypeDeclaration(
    node: JavaAstNode,
    packageName: Option[String],
    fullNameOverride: Option[String] = None,
    codeOverride: Option[String] = None,
    astParentTypeOverride: Option[String] = None,
    astParentFullNameOverride: Option[String] = None,
    localCaptureInfos: Seq[CaptureInfo] = Nil,
    nameOverride: Option[String] = None,
    inheritsOverride: Option[Seq[String]] = None,
    bodyChildrenOverride: Option[Seq[JavaAstNode]] = None,
    forceDefaultConstructor: Boolean = false,
    defaultConstructorParameters: Seq[SyntheticConstructorParameterInfo] = Nil
  ): Ast = {
    val name     = nameOverride.orElse(childByField(node, "name").map(_.code)).getOrElse("<anonymous>")
    val fullName = fullNameOverride.getOrElse(packageName.filter(_.nonEmpty).map(pkg => s"$pkg.$name").getOrElse(name))
    registerType(fullName)
    importAliases.update(name, fullName)

    val inherits = inheritsOverride.getOrElse(inheritsForTypeDeclaration(node))
    inherits.foreach(registerType)
    inheritedTypeNamesByType.update(fullName, inherits)

    val typeDecl = typeDeclNode(
      node,
      name,
      fullName,
      document.relativeName,
      codeOverride.getOrElse(declarationHeader(node)),
      astParentTypeOverride.getOrElse(NodeTypes.NAMESPACE_BLOCK),
      astParentFullNameOverride
        .getOrElse(
          packageName.map(pkg => s"${document.relativeName}:$pkg").getOrElse(NamespaceTraversal.globalNamespaceName)
        ),
      inherits
    )

    val savedPendingLambdaAsts        = pendingLambdaAsts
    val savedPendingAnonymousTypeAsts = pendingAnonymousTypeAsts
    val savedMethodInfos              = methodSignatureInfos.toMap
    pendingLambdaAsts = Nil
    pendingAnonymousTypeAsts = Nil
    currentTypeFullNames = fullName :: currentTypeFullNames
    currentTypeInherits = inherits :: currentTypeInherits
    try {
      val bodyChildren   = bodyChildrenOverride.getOrElse(typeBodyChildren(node))
      val typeParameters = typeParameterNames(node).toSet
      if (node.kind == "interface_declaration") {
        registerFunctionalInterface(node, fullName, name, bodyChildren)
      }
      bodyChildren.collect {
        case method if method.kind == "method_declaration" =>
          registerMethodDeclaration(method, fullName, typeParameters)
      }
      val recordTypeParameters = typeParameters
      val recordParams         = recordParameterInfos(node)
      if (node.kind == "record_declaration") {
        recordParameterInfosByType.update(fullName, recordParams)
      }
      bodyChildren.collect {
        case ctor if ctor.kind == "constructor_declaration" =>
          registerConstructorDeclaration(ctor, fullName, recordTypeParameters)
        case ctor if node.kind == "record_declaration" && ctor.kind == "compact_constructor_declaration" =>
          registerConstructorInfo(fullName, recordParams.map(_.typeFullName))
      }
      if (
        node.kind == "record_declaration" && !hasCanonicalRecordConstructor(
          bodyChildren,
          recordParams,
          recordTypeParameters
        )
      ) {
        registerConstructorInfo(fullName, recordParams.map(_.typeFullName))
      }
      if (
        shouldCreateDefaultConstructor(node, bodyChildren) || (forceDefaultConstructor && !hasConstructor(bodyChildren))
      ) {
        registerConstructorInfo(fullName, defaultConstructorParameters.map(_.typeFullName))
      }
      val explicitMethodNames = bodyChildren.collect {
        case method if method.kind == "method_declaration" => childByField(method, "name").map(_.code).getOrElse("")
      }.toSet
      val recordMemberAsts  = recordParams.map(astForRecordParameterMember(_, fullName))
      val captureMemberAsts = localCaptureInfos.map(astForCaptureMember(_, fullName))
      val recordAccessorAsts = recordParams
        .filterNot(param => explicitMethodNames.contains(param.name))
        .map(astForRecordParameterAccessor(_, fullName))
      val fieldDeclarations = bodyChildren.filter(_.kind == "field_declaration")
      val memberAsts        = fieldDeclarations.flatMap(astsForFieldDeclaration(_, fullName))
      val enumConstantAsts = bodyChildren.collect {
        case enumConstant if enumConstant.kind == "enum_constant" => astForEnumConstant(enumConstant, fullName)
      }
      val staticInitializerAst = astForStaticInitializerMethod(node, fullName, fieldDeclarations, bodyChildren)
      val constructorAsts = bodyChildren.collect {
        case ctor if ctor.kind == "constructor_declaration" =>
          astForConstructorDeclaration(ctor, fullName, fieldDeclarations, recordTypeParameters, localCaptureInfos)
      }
      val recordCompactConstructorAsts = bodyChildren.collect {
        case ctor if node.kind == "record_declaration" && ctor.kind == "compact_constructor_declaration" =>
          astForRecordCompactConstructor(ctor, fullName, recordParams, localCaptureInfos)
      }
      val recordCanonicalConstructorAst =
        Option.when(
          node.kind == "record_declaration" && !hasCanonicalRecordConstructor(
            bodyChildren,
            recordParams,
            recordTypeParameters
          )
        ) {
          astForRecordCanonicalConstructor(node, fullName, recordParams, localCaptureInfos)
        }
      val defaultConstructorAst = Option.when(
        shouldCreateDefaultConstructor(node, bodyChildren) || (forceDefaultConstructor && !hasConstructor(bodyChildren))
      ) {
        astForDefaultConstructor(
          node,
          fullName,
          fieldDeclarations,
          localCaptureInfos,
          defaultConstructorParameters,
          inherits.headOption
        )
      }
      val methodAsts = bodyChildren.collect {
        case method if method.kind == "method_declaration" =>
          astForMethodDeclaration(method, fullName, isStatic(method), typeParameters)
        case nested if TypeDeclarationKinds.contains(nested.kind) => astForTypeDeclaration(nested, Some(fullName))
      }
      val lambdaAsts        = pendingLambdaAsts.reverse
      val anonymousTypeAsts = pendingAnonymousTypeAsts.reverse
      val annotationAsts    = annotationAstsFor(node)
      val modifiers = modifierTypes(node, isInterface = node.kind == "interface_declaration").map(modifierNode(node, _))
      val declaredMethodAsts = recordCanonicalConstructorAst.toSeq ++
        recordCompactConstructorAsts ++
        defaultConstructorAst.toSeq ++
        constructorAsts ++
        recordAccessorAsts ++
        methodAsts ++
        staticInitializerAst.toSeq
      val bindingAsts = bindingAstsForMethods(declaredMethodAsts)

      val typeDeclAst = Ast(typeDecl)
        .withChildren(recordMemberAsts)
        .withChildren(captureMemberAsts)
        .withChildren(enumConstantAsts)
        .withChildren(memberAsts)
        .withChildren(recordCanonicalConstructorAst.toSeq)
        .withChildren(recordCompactConstructorAsts)
        .withChildren(defaultConstructorAst.toSeq)
        .withChildren(constructorAsts)
        .withChildren(recordAccessorAsts)
        .withChildren(methodAsts)
        .withChildren(staticInitializerAst.toSeq)
        .withChildren(lambdaAsts)
        .withChildren(anonymousTypeAsts)
        .withChildren(annotationAsts)
        .withChildren(modifiers.map(Ast(_)))
      typeDeclAstWithBindings(typeDeclAst, typeDecl, bindingAsts)
    } finally {
      currentTypeFullNames = currentTypeFullNames.tail
      currentTypeInherits = currentTypeInherits.tail
      pendingLambdaAsts = savedPendingLambdaAsts
      pendingAnonymousTypeAsts = savedPendingAnonymousTypeAsts
      methodSignatureInfos.clear()
      methodSignatureInfos ++= savedMethodInfos
    }
  }

  private def astForEnumConstant(node: JavaAstNode, ownerFullName: String): Ast = {
    val name            = childByField(node, "name").map(_.code).getOrElse(node.code.takeWhile(_ != '('))
    val constructedType = enumConstantConstructedType(node, ownerFullName)
    registerMember(ownerFullName, name, constructedType)
    Ast(memberNode(node, name, node.code, constructedType))
  }

  private def astsForFieldDeclaration(node: JavaAstNode, ownerFullName: String): Seq[Ast] = {
    val typeNode = childByField(node, "type")
    val baseTyp  = typeNode.map(typeName).getOrElse(Defines.Any)
    val typeCode = typeNode.map(_.code).getOrElse(baseTyp)
    variableDeclarators(node).map { declarator =>
      val dimensions = declaratorDimensionsCode(declarator)
      val typ        = registerType(s"$baseTyp$dimensions")
      val codeType   = s"$typeCode$dimensions"
      val name       = declaratorName(declarator)
      registerMember(ownerFullName, name, typ)
      val member      = memberNode(declarator, name, s"$codeType $name", typ)
      val annotations = annotationAstsFor(node)
      val modifiers   = modifierTypes(node, isInterface = false).map(modifierNode(node, _))
      Ast(member).withChildren(annotations).withChildren(modifiers.map(Ast(_)))
    }
  }

  private def variableDeclarators(node: JavaAstNode): Seq[JavaAstNode] =
    node.children.filter(_.kind == "variable_declarator")

  private def declaratorName(node: JavaAstNode): String =
    childByField(node, "name").map(_.code).getOrElse(node.code.takeWhile(ch => ch != '=' && ch != '[').trim)

  private def declaratorDimensionsCode(node: JavaAstNode): String =
    childByField(node, "dimensions").map(_.code.replaceAll("\\s+", "")).getOrElse("")

  private def recordParameterInfos(node: JavaAstNode): List[RecordParameterInfo] = {
    if (node.kind != "record_declaration") {
      Nil
    } else {
      val typeParameters = typeParameterNames(node).toSet
      childByField(node, "parameters").toList
        .flatMap(_.children)
        .filter(_.kind == "formal_parameter")
        .map { parameter =>
          val typeNode     = childByField(parameter, "type")
          val rawType      = typeNode.map(typeName).getOrElse(Defines.Any)
          val typeFullName = registerType(eraseTypeParameters(rawType, typeParameters))
          val typeCode     = typeNode.map(_.code).getOrElse(typeFullName)
          val name         = childByField(parameter, "name").map(_.code).getOrElse("value")
          RecordParameterInfo(parameter, name, typeFullName, typeCode)
        }
    }
  }

  private def astForRecordParameterMember(parameter: RecordParameterInfo, ownerFullName: String): Ast = {
    registerMember(ownerFullName, parameter.name, parameter.typeFullName)
    val member =
      memberNode(parameter.node, parameter.name, s"${parameter.typeCode} ${parameter.name}", parameter.typeFullName)
    val modifiers = Seq(ModifierTypes.PRIVATE, ModifierTypes.FINAL).map(modifierNode(parameter.node, _))
    Ast(member).withChildren(annotationAstsFor(parameter.node)).withChildren(modifiers.map(Ast(_)))
  }

  private def astForCaptureMember(capture: CaptureInfo, ownerFullName: String): Ast = {
    registerMember(ownerFullName, capture.name, capture.typeFullName)
    val member =
      memberNode(capture.node, capture.name, s"${capture.typeFullName} ${capture.name}", capture.typeFullName)
    val modifiers = Seq(ModifierTypes.PRIVATE, ModifierTypes.FINAL).map(modifierNode(capture.node, _))
    Ast(member).withChildren(modifiers.map(Ast(_)))
  }

  private def astForRecordParameterAccessor(parameter: RecordParameterInfo, ownerFullName: String): Ast = {
    val signature = composeSignature(parameter.typeFullName, Nil)
    val fullName  = s"$ownerFullName.${parameter.name}:$signature"
    val info      = MethodSignatureInfo(Nil, parameter.typeFullName, fullName, signature, isStatic = false)
    methodSignatureInfos.update(parameter.name, info)
    methodSignatureInfosByType.update((ownerFullName, parameter.name), info)
    val method = methodNode(
      parameter.node,
      parameter.name,
      s"public ${parameter.typeCode} ${parameter.name}()",
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val thisParam = thisParameter(parameter.node, ownerFullName)
    val bodyAst = withReturnType(parameter.typeFullName) {
      withScope(Seq("this" -> ownerFullName)) {
        blockAst(
          blockNode(parameter.node, "", Defines.Any),
          List(recordParameterReturnAst(parameter, thisParam, ownerFullName))
        )
      }
    }
    val methodReturn = methodReturnNode(parameter.node, parameter.typeFullName)
    val modifiers    = Seq(ModifierTypes.PUBLIC, ModifierTypes.VIRTUAL).map(modifierNode(parameter.node, _))
    methodAst(method, Seq(Ast(thisParam)), bodyAst, methodReturn, modifiers)
  }

  private def recordParameterReturnAst(
    parameter: RecordParameterInfo,
    thisParam: NewMethodParameterIn,
    ownerFullName: String
  ): Ast = {
    val fieldAccess =
      recordFieldAccessAst(parameter.node, parameter.name, parameter.typeFullName, ownerFullName, Some(thisParam))
    returnAst(returnNode(parameter.node, s"return this.${parameter.name}"), Seq(fieldAccess))
  }

  private def astForMethodDeclaration(
    node: JavaAstNode,
    ownerFullName: String,
    staticMethod: Boolean,
    enclosingTypeParameters: Set[String] = Set.empty
  ): Ast = {
    val name                 = childByField(node, "name").map(_.code).getOrElse("<anonymous>")
    val erasedTypeParameters = enclosingTypeParameters ++ typeParameterNames(node)
    val returnType = childByField(node, "type")
      .map(typeNode => eraseTypeParameters(typeName(typeNode), erasedTypeParameters))
      .getOrElse("void")
    registerType(returnType)
    val parameters = parameterNodes(node, erasedTypeParameters)
    val signature  = composeSignature(returnType, parameters.map(_._2))
    val fullName   = s"$ownerFullName.$name:$signature"
    val method = methodNode(
      node,
      name,
      declarationHeader(node),
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val methodScopeBindings = parameters.map { case (_, typ, name) => name -> typ } ++
      Option.unless(staticMethod)("this" -> ownerFullName)
    val thisParam = Option.unless(staticMethod)(thisParameter(node, ownerFullName))
    val parameterAsts = parameters.zipWithIndex.map { case ((param, typ, _), index) =>
      parameterAst(param, index + 1, typ)
    }
    val parameterRefBindings = parameterBindings(thisParam.toSeq.map(Ast(_)) ++ parameterAsts)
    val bodyAst = withMethodFullName(fullName) {
      withReturnType(returnType) {
        withScope(methodScopeBindings) {
          withParameterRefs(parameterRefBindings) {
            childByField(node, "body").map(astForBlock(_)).getOrElse(emptyBlockAst(node))
          }
        }
      }
    }
    val methodReturn = methodReturnNode(childByField(node, "type").getOrElse(node), returnType)
    val paramAsts    = thisParam.map(Ast(_)).toSeq ++ parameterAsts
    val modifiers    = methodModifiers(node, staticMethod, constructor = false)
    val annotations  = annotationAstsFor(node)
    methodAstWithAnnotations(method, paramAsts, bodyAst, methodReturn, modifiers, annotations)
  }

  private def astForConstructorDeclaration(
    node: JavaAstNode,
    ownerFullName: String,
    instanceFieldDeclarations: Seq[JavaAstNode] = Nil,
    erasedTypeParameters: Set[String] = Set.empty,
    captureInfos: Seq[CaptureInfo] = Nil
  ): Ast = {
    val parameters = parameterNodes(node, erasedTypeParameters)
    val signature  = composeSignature("void", parameters.map(_._2))
    val fullName   = s"$ownerFullName.${Defines.ConstructorMethodName}:$signature"
    val method = methodNode(
      node,
      Defines.ConstructorMethodName,
      declarationHeader(node),
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val methodScopeBindings = ("this" -> ownerFullName) +: (parameters.map { case (_, typ, name) => name -> typ } ++
      captureInfos.map(capture => capture.name -> capture.typeFullName))
    val thisParam = thisParameter(node, ownerFullName)
    val parameterAsts = parameters.zipWithIndex.map { case ((param, typ, _), index) =>
      parameterAst(param, index + 1, typ)
    }
    val captureParamAsts = captureInfos.zipWithIndex.map { case (capture, index) =>
      captureConstructorParameterAst(capture, parameters.size + index + 1)
    }
    val captureParamNodes    = captureParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val parameterRefBindings = parameterBindings(Ast(thisParam) +: (parameterAsts ++ captureParamAsts))
    val bodyAst = withReturnType("void") {
      withScope(methodScopeBindings) {
        withParameterRefs(parameterRefBindings) {
          val captureAssignments = captureInfos.zip(captureParamNodes).map { case (capture, captureParam) =>
            astForCaptureAssignment(capture, captureParam, ownerFullName, thisParam)
          }
          childByField(node, "body")
            .map { body =>
              val prefixAsts =
                if (startsWithExplicitConstructorInvocation(body)) Nil
                else instanceFieldInitializerAsts(instanceFieldDeclarations, ownerFullName) ++ captureAssignments
              astForBlock(body, prefixAsts = prefixAsts)
            }
            .getOrElse(emptyBlockAst(node))
        }
      }
    }
    val methodReturn = methodReturnNode(node, "void")
    val paramAsts    = Ast(thisParam) +: (parameterAsts ++ captureParamAsts)
    val modifiers    = methodModifiers(node, staticMethod = false, constructor = true)
    val annotations  = annotationAstsFor(node)
    methodAstWithAnnotations(method, paramAsts, bodyAst, methodReturn, modifiers, annotations)
  }

  private def astForDefaultConstructor(
    node: JavaAstNode,
    ownerFullName: String,
    instanceFieldDeclarations: Seq[JavaAstNode],
    captureInfos: Seq[CaptureInfo] = Nil,
    syntheticParameters: Seq[SyntheticConstructorParameterInfo] = Nil,
    superTypeFullName: Option[String] = None
  ): Ast = {
    val signature = composeSignature("void", syntheticParameters.map(_.typeFullName))
    val fullName  = s"$ownerFullName.${Defines.ConstructorMethodName}:$signature"
    val method = methodNode(
      node,
      Defines.ConstructorMethodName,
      "",
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val thisParam = thisParameter(node, ownerFullName)
    val syntheticParamAsts = syntheticParameters.zipWithIndex.map { case (parameter, index) =>
      syntheticConstructorParameterAst(parameter, index + 1)
    }
    val captureParamAsts = captureInfos.zipWithIndex.map { case (capture, index) =>
      captureConstructorParameterAst(capture, syntheticParameters.size + index + 1)
    }
    val captureParamNodes    = captureParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val parameterRefBindings = parameterBindings(Ast(thisParam) +: (syntheticParamAsts ++ captureParamAsts))
    val bodyAst = withReturnType("void") {
      val bindings = ("this" -> ownerFullName) +:
        (syntheticParameters.map(parameter => parameter.name -> parameter.typeFullName) ++
          captureInfos.map(capture => capture.name -> capture.typeFullName))
      withScope(bindings) {
        withParameterRefs(parameterRefBindings) {
          val captureAssignments = captureInfos.zip(captureParamNodes).map { case (capture, captureParam) =>
            astForCaptureAssignment(capture, captureParam, ownerFullName, thisParam)
          }
          val superConstructorCall =
            Option.when(syntheticParameters.nonEmpty)(
              astForSyntheticSuperConstructorInvocation(node, superTypeFullName, syntheticParameters)
            )
          blockAst(
            blockNode(node, "", Defines.Any),
            (superConstructorCall.toSeq ++ instanceFieldInitializerAsts(
              instanceFieldDeclarations,
              ownerFullName
            ) ++ captureAssignments).toList
          )
        }
      }
    }
    val methodReturn = methodReturnNode(node, "void")
    val paramAsts    = Ast(thisParam) +: (syntheticParamAsts ++ captureParamAsts)
    val modifiers    = List(ModifierTypes.CONSTRUCTOR, ModifierTypes.PUBLIC).map(modifierNode(node, _))
    methodAst(method, paramAsts, bodyAst, methodReturn, modifiers)
  }

  private def astForRecordCanonicalConstructor(
    node: JavaAstNode,
    ownerFullName: String,
    recordParameters: Seq[RecordParameterInfo],
    captureInfos: Seq[CaptureInfo] = Nil
  ): Ast = {
    val signature = composeSignature("void", recordParameters.map(_.typeFullName))
    val fullName  = s"$ownerFullName.${Defines.ConstructorMethodName}:$signature"
    val method = methodNode(
      node,
      Defines.ConstructorMethodName,
      "",
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val thisParam = thisParameter(node, ownerFullName)
    val recordParamAsts = recordParameters.zipWithIndex.map { case (parameter, index) =>
      recordConstructorParameterAst(parameter, index + 1)
    }
    val recordParamNodes = recordParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val captureParamAsts = captureInfos.zipWithIndex.map { case (capture, index) =>
      captureConstructorParameterAst(capture, recordParameters.size + index + 1)
    }
    val captureParamNodes = captureParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val bindings = ("this" -> ownerFullName) +: (recordParameters.map(param => param.name -> param.typeFullName) ++
      captureInfos.map(capture => capture.name -> capture.typeFullName))
    val bodyAst = withReturnType("void") {
      withScope(bindings) {
        val assignments = recordParameters.zip(recordParamNodes).map { case (parameter, paramNode) =>
          astForRecordParameterAssignment(parameter, paramNode, ownerFullName, thisParam)
        }
        val captureAssignments = captureInfos.zip(captureParamNodes).map { case (capture, captureParam) =>
          astForCaptureAssignment(capture, captureParam, ownerFullName, thisParam)
        }
        blockAst(blockNode(node, "", Defines.Any), (assignments ++ captureAssignments).toList)
      }
    }
    val methodReturn = methodReturnNode(node, "void")
    val modifiers    = List(ModifierTypes.CONSTRUCTOR, ModifierTypes.PUBLIC).map(modifierNode(node, _))
    methodAst(method, Ast(thisParam) +: (recordParamAsts ++ captureParamAsts), bodyAst, methodReturn, modifiers)
  }

  private def astForRecordCompactConstructor(
    node: JavaAstNode,
    ownerFullName: String,
    recordParameters: Seq[RecordParameterInfo],
    captureInfos: Seq[CaptureInfo] = Nil
  ): Ast = {
    val signature = composeSignature("void", recordParameters.map(_.typeFullName))
    val fullName  = s"$ownerFullName.${Defines.ConstructorMethodName}:$signature"
    val method = methodNode(
      node,
      Defines.ConstructorMethodName,
      declarationHeader(node),
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )
    val thisParam = thisParameter(node, ownerFullName)
    val recordParamAsts = recordParameters.zipWithIndex.map { case (parameter, index) =>
      recordConstructorParameterAst(parameter, index + 1)
    }
    val recordParamNodes = recordParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val captureParamAsts = captureInfos.zipWithIndex.map { case (capture, index) =>
      captureConstructorParameterAst(capture, recordParameters.size + index + 1)
    }
    val captureParamNodes    = captureParamAsts.flatMap(_.root).collect { case param: NewMethodParameterIn => param }
    val parameterRefBindings = parameterBindings(Ast(thisParam) +: (recordParamAsts ++ captureParamAsts))
    val bindings = ("this" -> ownerFullName) +: (recordParameters.map(param => param.name -> param.typeFullName) ++
      captureInfos.map(capture => capture.name -> capture.typeFullName))
    val bodyAst = withReturnType("void") {
      withScope(bindings) {
        withParameterRefs(parameterRefBindings) {
          val assignments = recordParameters.zip(recordParamNodes).map { case (parameter, paramNode) =>
            astForRecordParameterAssignment(parameter, paramNode, ownerFullName, thisParam)
          }
          val captureAssignments = captureInfos.zip(captureParamNodes).map { case (capture, captureParam) =>
            astForCaptureAssignment(capture, captureParam, ownerFullName, thisParam)
          }
          val bodyNode       = childByField(node, "body")
          val bodyStatements = bodyNode.map(body => namedChildren(body).flatMap(astsForStatement)).getOrElse(Nil)
          val blockNodeForBody = bodyNode
            .map(body => blockNode(body, "", Defines.Any))
            .getOrElse(blockNode(node, "", Defines.Any))
          blockAst(blockNodeForBody, (assignments ++ captureAssignments ++ bodyStatements).toList)
        }
      }
    }
    val methodReturn = methodReturnNode(node, "void")
    val modifiers    = methodModifiers(node, staticMethod = false, constructor = true)
    methodAst(method, Ast(thisParam) +: (recordParamAsts ++ captureParamAsts), bodyAst, methodReturn, modifiers)
  }

  private def recordConstructorParameterAst(parameter: RecordParameterInfo, index: Int): Ast = {
    Ast(
      parameterInNode(
        parameter.node,
        parameter.name,
        parameter.node.code,
        index = index,
        isVariadic = false,
        evaluationStrategy = evaluationStrategyFor(parameter.typeFullName),
        typeFullName = parameter.typeFullName
      )
    ).withChildren(annotationAstsFor(parameter.node))
  }

  private def captureConstructorParameterAst(capture: CaptureInfo, index: Int): Ast = {
    Ast(
      parameterInNode(
        capture.node,
        capture.name,
        s"${capture.typeFullName} ${capture.name}",
        index = index,
        isVariadic = false,
        evaluationStrategy = evaluationStrategyFor(capture.typeFullName),
        typeFullName = capture.typeFullName
      )
    )
  }

  private def syntheticConstructorParameterAst(parameter: SyntheticConstructorParameterInfo, index: Int): Ast = {
    Ast(
      parameterInNode(
        parameter.node,
        parameter.name,
        s"${parameter.typeFullName} ${parameter.name}",
        index = index,
        isVariadic = false,
        evaluationStrategy = evaluationStrategyFor(parameter.typeFullName),
        typeFullName = parameter.typeFullName
      )
    )
  }

  private def astForSyntheticSuperConstructorInvocation(
    node: JavaAstNode,
    superTypeFullName: Option[String],
    parameters: Seq[SyntheticConstructorParameterInfo]
  ): Ast = {
    val receiverType = superTypeFullName.getOrElse(Defines.Any)
    val receiverAst  = Ast(identifierNode(node, "this", "this", receiverType))
    val argAsts = parameters.map { parameter =>
      val identifier = identifierNode(parameter.node, parameter.name, parameter.name, parameter.typeFullName)
      parameterRef(parameter.name)
        .map(target => Ast(identifier).withRefEdge(identifier, target))
        .getOrElse(Ast(identifier))
    }
    val signature = composeSignature("void", parameters.map(_.typeFullName))
    val methodFullName =
      if (receiverType == Defines.Any) Defines.ConstructorMethodName
      else s"$receiverType.${Defines.ConstructorMethodName}:$signature"
    val call = callNode(
      node,
      s"super(${parameters.map(_.name).mkString(", ")})",
      Defines.ConstructorMethodName,
      methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(signature),
      Some("void")
    )
    callAst(call, argAsts, Some(receiverAst))
  }

  private def astForRecordParameterAssignment(
    parameter: RecordParameterInfo,
    parameterNode: NewMethodParameterIn,
    ownerFullName: String,
    thisParam: NewMethodParameterIn
  ): Ast = {
    val fieldAccess =
      recordFieldAccessAst(parameter.node, parameter.name, parameter.typeFullName, ownerFullName, Some(thisParam))
    val paramIdentifier    = identifierNode(parameter.node, parameter.name, parameter.name, parameter.typeFullName)
    val paramIdentifierAst = Ast(paramIdentifier).withRefEdge(paramIdentifier, parameterNode)
    callAst(
      callNode(
        parameter.node,
        s"this.${parameter.name} = ${parameter.name}",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(parameter.typeFullName)
      ),
      Seq(fieldAccess, paramIdentifierAst)
    )
  }

  private def astForCaptureAssignment(
    capture: CaptureInfo,
    captureParamNode: NewMethodParameterIn,
    ownerFullName: String,
    thisParam: NewMethodParameterIn
  ): Ast = {
    val fieldAccess =
      recordFieldAccessAst(capture.node, capture.name, capture.typeFullName, ownerFullName, Some(thisParam))
    val paramIdentifier    = identifierNode(capture.node, capture.name, capture.name, capture.typeFullName)
    val paramIdentifierAst = Ast(paramIdentifier).withRefEdge(paramIdentifier, captureParamNode)
    callAst(
      callNode(
        capture.node,
        s"this.${capture.name} = ${capture.name}",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(capture.typeFullName)
      ),
      Seq(fieldAccess, paramIdentifierAst)
    )
  }

  private def recordFieldAccessAst(
    node: JavaAstNode,
    fieldName: String,
    fieldType: String,
    ownerFullName: String,
    thisParam: Option[NewMethodParameterIn]
  ): Ast = {
    val thisIdentifier = identifierNode(node, "this", "this", ownerFullName)
    val thisAst = thisParam match {
      case Some(param) => Ast(thisIdentifier).withRefEdge(thisIdentifier, param)
      case None        => Ast(thisIdentifier)
    }
    val fieldIdentifier = Ast(fieldIdentifierNode(node, fieldName, fieldName))
    callAst(
      callNode(
        node,
        s"this.$fieldName",
        Operators.fieldAccess,
        Operators.fieldAccess,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(fieldType)
      ),
      Seq(thisAst, fieldIdentifier)
    )
  }

  private def hasCanonicalRecordConstructor(
    bodyChildren: Seq[JavaAstNode],
    recordParameters: Seq[RecordParameterInfo],
    erasedTypeParameters: Set[String]
  ): Boolean = {
    bodyChildren.exists(_.kind == "compact_constructor_declaration") ||
    bodyChildren.exists { child =>
      child.kind == "constructor_declaration" &&
      parameterNodes(child, erasedTypeParameters).map(_._2) == recordParameters.map(_.typeFullName)
    }
  }

  private def shouldCreateDefaultConstructor(node: JavaAstNode, bodyChildren: Seq[JavaAstNode]): Boolean = {
    node.kind == "class_declaration" && !hasConstructor(bodyChildren)
  }

  private def hasConstructor(bodyChildren: Seq[JavaAstNode]): Boolean = {
    val constructorKinds = Set("constructor_declaration", "compact_constructor_declaration")
    bodyChildren.exists(child => constructorKinds.contains(child.kind))
  }

  private def startsWithExplicitConstructorInvocation(bodyNode: JavaAstNode): Boolean = {
    namedChildren(bodyNode).headOption.exists(_.kind == "explicit_constructor_invocation")
  }

  private def astForStaticInitializerMethod(
    node: JavaAstNode,
    ownerFullName: String,
    fieldDeclarations: Seq[JavaAstNode],
    bodyChildren: Seq[JavaAstNode]
  ): Option[Ast] = {
    val signature = composeSignature("void", Nil)
    val fullName  = s"$ownerFullName.${Defines.StaticInitMethodName}:$signature"
    val initAsts = withMethodFullName(fullName) {
      withReturnType("void") {
        withScope(Nil) {
          enumConstantInitializerAsts(bodyChildren, ownerFullName) ++
            staticFieldInitializerAsts(fieldDeclarations, ownerFullName) ++
            bodyChildren.filter(_.kind == "static_initializer").flatMap(astsForStaticInitializer)
        }
      }
    }
    Option.when(initAsts.nonEmpty) {
      staticInitMethodAst(node, initAsts.toList, fullName, Some(signature), "void", Some(document.relativeName))
    }
  }

  private def staticFieldInitializerAsts(fieldDeclarations: Seq[JavaAstNode], ownerFullName: String): Seq[Ast] = {
    fieldDeclarations
      .filter(isStatic)
      .flatMap { field =>
        val baseTyp = childByField(field, "type").map(typeName).getOrElse(Defines.Any)
        variableDeclarators(field).flatMap { declarator =>
          val typ  = s"$baseTyp${declaratorDimensionsCode(declarator)}"
          val name = declaratorName(declarator)
          childByField(declarator, "value").toSeq.flatMap { initializer =>
            val initialPatternLocalCount = pendingPatternLocals.size
            val initializerAsts = initializer match {
              case objectCreation if objectCreation.kind == "object_creation_expression" =>
                astsForStaticObjectCreationFieldInitializer(declarator, name, typ, objectCreation, ownerFullName)
              case _ =>
                Seq(astForStaticFieldInitializer(declarator, name, typ, initializer, ownerFullName))
            }
            drainPendingPatternLocals(initialPatternLocalCount) ++ initializerAsts
          }
        }
      }
  }

  private def instanceFieldInitializerAsts(fieldDeclarations: Seq[JavaAstNode], ownerFullName: String): Seq[Ast] = {
    fieldDeclarations
      .filterNot(isStatic)
      .flatMap { field =>
        val baseTyp = childByField(field, "type").map(typeName).getOrElse(Defines.Any)
        variableDeclarators(field).flatMap { declarator =>
          val typ  = s"$baseTyp${declaratorDimensionsCode(declarator)}"
          val name = declaratorName(declarator)
          childByField(declarator, "value").toSeq.flatMap { initializer =>
            val initialPatternLocalCount = pendingPatternLocals.size
            val initializerAsts = initializer match {
              case objectCreation if objectCreation.kind == "object_creation_expression" =>
                astsForObjectCreationFieldInitializer(declarator, name, typ, objectCreation, ownerFullName)
              case _ =>
                Seq(astForFieldInitializer(declarator, name, typ, initializer, ownerFullName))
            }
            drainPendingPatternLocals(initialPatternLocalCount) ++ initializerAsts
          }
        }
      }
  }

  private def enumConstantInitializerAsts(bodyChildren: Seq[JavaAstNode], ownerFullName: String): Seq[Ast] = {
    bodyChildren
      .filter(_.kind == "enum_constant")
      .flatMap { enumConstant =>
        val name = childByField(enumConstant, "name").map(_.code).getOrElse(enumConstant.code.takeWhile(_ != '('))
        val constructedType = enumConstantConstructedType(enumConstant, ownerFullName)
        val targetAst       = staticFieldAccessAst(enumConstant, name, constructedType, ownerFullName)
        val allocAst        = objectCreationAllocAst(enumConstant, constructedType)
        val assignCall = callNode(
          enumConstant,
          s"${simpleTypeName(ownerFullName)}.$name = ${enumConstant.code}",
          Operators.assignment,
          Operators.assignment,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Some(constructedType)
        )
        val assignAst = callAst(assignCall, Seq(targetAst, allocAst))

        val initReceiver = identifierNode(enumConstant, name, name, constructedType)
        val initAst      = objectCreationInitAst(enumConstant, constructedType, Ast(initReceiver))
        Seq(assignAst, initAst)
      }
  }

  private def enumConstantConstructedType(node: JavaAstNode, ownerFullName: String): String = {
    enumAnonymousClassType(node, ownerFullName).getOrElse(registerType(ownerFullName))
  }

  private def enumAnonymousClassType(node: JavaAstNode, ownerFullName: String): Option[String] = {
    val key = node.startByte.toInt -> node.endByte.toInt
    node.children.find(_.kind == "class_body").map { body =>
      enumAnonymousTypes.getOrElseUpdate(
        key, {
          val index    = anonymousTypeCounters.getOrElse(ownerFullName -> "<enum>", 0)
          val anonName = s"${simpleTypeName(ownerFullName)}$$$index"
          val fullName = registerType(s"$ownerFullName$$$index")
          anonymousTypeCounters.update(ownerFullName -> "<enum>", index + 1)
          val bodyChildren                   = body.children.filter(_.named)
          val syntheticConstructorParameters = objectCreationArgumentInfos(node)
          val typeAst = withoutParameterRefs {
            astForTypeDeclaration(
              node,
              packageName = None,
              fullNameOverride = Some(fullName),
              codeOverride = Some(node.code),
              astParentTypeOverride = Some(NodeTypes.TYPE_DECL),
              astParentFullNameOverride = Some(ownerFullName),
              nameOverride = Some(anonName),
              inheritsOverride = Some(Seq(ownerFullName)),
              bodyChildrenOverride = Some(bodyChildren),
              forceDefaultConstructor = true,
              defaultConstructorParameters = syntheticConstructorParameters
            )
          }
          pendingAnonymousTypeAsts = typeAst :: pendingAnonymousTypeAsts
          fullName
        }
      )
    }
  }

  private def astForStaticFieldInitializer(
    declarator: JavaAstNode,
    fieldName: String,
    fieldType: String,
    initializer: JavaAstNode,
    ownerFullName: String
  ): Ast = {
    val fieldAccess = staticFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val rhs         = withExpectedExpressionType(Some(fieldType))(astForExpression(initializer))
    callAst(
      callNode(
        declarator,
        s"${simpleTypeName(ownerFullName)}.$fieldName = ${initializer.code}",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(registerType(fieldType))
      ),
      Seq(fieldAccess, rhs)
    )
  }

  private def astsForStaticObjectCreationFieldInitializer(
    declarator: JavaAstNode,
    fieldName: String,
    fieldType: String,
    initializer: JavaAstNode,
    ownerFullName: String
  ): Seq[Ast] = {
    val targetAst = staticFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val constructedType =
      withAnonymousOwnerBase(s"$ownerFullName.$fieldName")(constructedTypeForObjectCreation(initializer))
    val allocAst = objectCreationAllocAst(initializer, constructedType)
    val assignCall = callNode(
      declarator,
      s"${simpleTypeName(ownerFullName)}.$fieldName = ${initializer.code}",
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(registerType(fieldType))
    )
    val assignAst = callAst(assignCall, Seq(targetAst, allocAst))

    val initReceiver = staticFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val initAst      = objectCreationInitAst(initializer, constructedType, initReceiver)
    Seq(assignAst, initAst)
  }

  private def astsForObjectCreationFieldInitializer(
    declarator: JavaAstNode,
    fieldName: String,
    fieldType: String,
    initializer: JavaAstNode,
    ownerFullName: String
  ): Seq[Ast] = {
    val targetAst = instanceFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val constructedType =
      withAnonymousOwnerBase(s"$ownerFullName.$fieldName")(constructedTypeForObjectCreation(initializer))
    val allocAst = objectCreationAllocAst(initializer, constructedType)
    val assignCall = callNode(
      declarator,
      s"this.$fieldName = ${initializer.code}",
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(registerType(fieldType))
    )
    val assignAst = callAst(assignCall, Seq(targetAst, allocAst))

    val initReceiver = instanceFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val initAst      = objectCreationInitAst(initializer, constructedType, initReceiver)
    Seq(assignAst, initAst)
  }

  private def astForFieldInitializer(
    declarator: JavaAstNode,
    fieldName: String,
    fieldType: String,
    initializer: JavaAstNode,
    ownerFullName: String
  ): Ast = {
    val fieldAccess = instanceFieldAccessAst(declarator, fieldName, fieldType, ownerFullName)
    val rhs         = withExpectedExpressionType(Some(fieldType))(astForExpression(initializer))
    callAst(
      callNode(
        declarator,
        s"this.$fieldName = ${initializer.code}",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(registerType(fieldType))
      ),
      Seq(fieldAccess, rhs)
    )
  }

  private def instanceFieldAccessAst(
    node: JavaAstNode,
    fieldName: String,
    fieldType: String,
    ownerFullName: String
  ): Ast = {
    val thisAst         = Ast(identifierNode(node, "this", "this", ownerFullName))
    val fieldIdentifier = Ast(fieldIdentifierNode(node, fieldName, fieldName))
    callAst(
      callNode(
        node,
        s"this.$fieldName",
        Operators.fieldAccess,
        Operators.fieldAccess,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(registerType(fieldType))
      ),
      Seq(thisAst, fieldIdentifier)
    )
  }

  private def astsForStaticInitializer(node: JavaAstNode): Seq[Ast] = {
    namedChildren(node)
      .find(_.kind == "block")
      .map(block => namedChildren(block).flatMap(astsForStatement))
      .getOrElse(Nil)
  }

  private def staticFieldAccessAst(
    node: JavaAstNode,
    fieldName: String,
    fieldType: String,
    ownerFullName: String
  ): Ast = {
    val ownerTypeAst = Ast(typeRefNode(node, simpleTypeName(ownerFullName), ownerFullName))
    fieldAccessAst(
      node,
      node,
      ownerTypeAst,
      s"${simpleTypeName(ownerFullName)}.$fieldName",
      fieldName,
      registerType(fieldType)
    )
  }

  private def astForBlock(node: JavaAstNode, prefixAsts: Seq[Ast] = Nil, codeOverride: Option[String] = None): Ast = {
    val block = blockNode(node, node.code, Defines.Any)
    codeOverride.foreach(block.code)
    withScope(Nil) {
      blockAst(block, prefixAsts.toList ++ namedChildren(node).flatMap(astsForStatement))
    }
  }

  private def emptyBlockAst(node: JavaAstNode): Ast =
    Ast(NewBlock().typeFullName(Defines.Any).code("").lineNumber(line(node)).columnNumber(column(node)))

  private def astsForStatement(node: JavaAstNode): Seq[Ast] = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val statementAsts = node.kind match {
      case "block"                      => Seq(astForBlock(node))
      case "local_variable_declaration" => astsForLocalVariableDeclaration(node)
      case "expression_statement"       => namedChildren(node).headOption.map(astsForExpressionStatement).getOrElse(Nil)
      case "return_statement"           => Seq(astForReturnStatement(node))
      case "if_statement"               => Seq(astForIfStatement(node))
      case "while_statement"            => Seq(astForWhileStatement(node))
      case "do_statement"               => Seq(astForDoStatement(node))
      case "for_statement"              => Seq(astForForStatement(node))
      case "enhanced_for_statement"     => Seq(astForEnhancedForStatement(node))
      case "switch_expression"          => Seq(astForSwitchExpression(node, ControlStructureTypes.SWITCH))
      case "try_statement" | "try_with_resources_statement" => astsForTryStatement(node)
      case "synchronized_statement"                         => Seq(astForSynchronizedStatement(node))
      case "assert_statement"                               => Seq(astForAssertStatement(node))
      case "labeled_statement"                              => astsForLabeledStatement(node)
      case "yield_statement"                                => Seq(astForYieldStatement(node))
      case "throw_statement"                                => Seq(astForThrowStatement(node))
      case "break_statement"    => Seq(astForBreakOrContinueStatement(node, ControlStructureTypes.BREAK))
      case "continue_statement" => Seq(astForBreakOrContinueStatement(node, ControlStructureTypes.CONTINUE))
      case "explicit_constructor_invocation"           => Seq(astForExplicitConstructorInvocation(node))
      case kind if TypeDeclarationKinds.contains(kind) => Seq(astForLocalTypeDeclaration(node))
      case _                                           => Seq.empty
    }
    drainPendingPatternLocals(initialPatternLocalCount) ++ statementAsts
  }

  private def astForLocalTypeDeclaration(node: JavaAstNode): Ast = {
    val name = childByField(node, "name").map(_.code).getOrElse("<anonymous>")
    val ownerFullName =
      currentMethodFullNames.headOption.orElse(currentTypeFullNames.headOption).getOrElse(Defines.UnresolvedNamespace)
    val fullName        = s"$ownerFullName.$name"
    val localRecordCode = if (node.kind == "record_declaration") Some(s"record $name") else None
    val localCaptureInfos =
      node.kind match {
        case "record_declaration" => localRecordCaptureInfos(node, recordParameterInfos(node).map(_.name).toSet)
        case "class_declaration"  => localClassCaptureInfos(node)
        case _                    => Nil
      }
    if (node.kind == "record_declaration" || node.kind == "class_declaration") {
      localRecordCaptureInfosByType.update(fullName, localCaptureInfos)
      if (localCaptureInfos.exists(_.name == "outerClass")) {
        anonymousOuterClassTypes += fullName
      }
    }
    withoutParameterRefs {
      withoutEnclosingLocalScopes {
        astForTypeDeclaration(
          node,
          packageName = None,
          fullNameOverride = Some(fullName),
          codeOverride = localRecordCode,
          astParentTypeOverride = Some(NodeTypes.METHOD),
          astParentFullNameOverride = Some(ownerFullName),
          localCaptureInfos = localCaptureInfos
        )
      }
    }
  }

  private def localClassCaptureInfos(node: JavaAstNode): Seq[CaptureInfo] = {
    val outerCapture = visibleLocalType("this").toSeq.map { typeFullName =>
      CaptureInfo(node, "outerClass", typeFullName)
    }
    val declarationsInside = declaredNames(node)
    val excludedNames      = declarationsInside + "this" + "super"
    val capturedVisibleLocals = identifierUses(node).foldLeft(Vector.empty[CaptureInfo]) { (captures, identifier) =>
      val name = identifier.code
      if (excludedNames.contains(name) || captures.exists(_.name == name)) {
        captures
      } else {
        visibleLocalType(name)
          .map(typeFullName => captures :+ CaptureInfo(identifier, name, typeFullName))
          .getOrElse(captures)
      }
    }
    outerCapture ++ capturedVisibleLocals
  }

  private def localRecordCaptureInfos(node: JavaAstNode, recordParameterNames: Set[String]): Seq[CaptureInfo] = {
    val declarationsInsideRecord = declaredNames(node)
    val excludedNames            = recordParameterNames ++ declarationsInsideRecord + "this" + "super"
    identifierUses(node).foldLeft(Vector.empty[CaptureInfo]) { (captures, identifier) =>
      val name = identifier.code
      if (excludedNames.contains(name) || captures.exists(_.name == name)) {
        captures
      } else {
        visibleLocalType(name)
          .map(typeFullName => captures :+ CaptureInfo(identifier, name, typeFullName))
          .getOrElse(captures)
      }
    }
  }

  private def anonymousCaptureInfos(node: JavaAstNode): Seq[CaptureInfo] = {
    val outerCapture = visibleLocalType("this").toSeq.map { typeFullName =>
      CaptureInfo(node, "outerClass", typeFullName)
    }
    val bodyNode           = node.children.find(_.kind == "class_body")
    val declarationsInside = bodyNode.map(declaredNames).getOrElse(Set.empty)
    val excludedNames      = declarationsInside + "this" + "super"
    val capturedVisibleLocals =
      bodyNode.toSeq.flatMap(identifierUses).foldLeft(Vector.empty[CaptureInfo]) { (captures, identifier) =>
        val name = identifier.code
        if (excludedNames.contains(name) || captures.exists(_.name == name)) {
          captures
        } else {
          visibleLocalType(name)
            .map(typeFullName => captures :+ CaptureInfo(identifier, name, typeFullName))
            .getOrElse(captures)
        }
      }
    outerCapture ++ capturedVisibleLocals
  }

  private def anonymousOwnerBase(): String = {
    anonymousOwnerBases.headOption
      .map(stripFinalSignature)
      .orElse(currentMethodFullNames.headOption.map(stripFinalSignature))
      .orElse(currentTypeFullNames.headOption)
      .getOrElse(Defines.UnresolvedNamespace)
  }

  private def stripFinalSignature(fullName: String): String = {
    val signatureStart = fullName.lastIndexOf(':')
    if (signatureStart >= 0) fullName.take(signatureStart) else fullName
  }

  private def nextAnonymousTypeName(ownerFullName: String, baseSimpleName: String): String = {
    val key   = ownerFullName -> baseSimpleName
    val index = anonymousTypeCounters.getOrElse(key, 0)
    anonymousTypeCounters.update(key, index + 1)
    s"$baseSimpleName$$$index"
  }

  private def declaredNames(node: JavaAstNode): Set[String] = {
    val current = node.kind match {
      case "formal_parameter" | "catch_formal_parameter" =>
        childByField(node, "name").map(_.code).toSet
      case "local_variable_declaration" =>
        node.children
          .filter(_.kind == "variable_declarator")
          .flatMap(decl => childByField(decl, "name").map(_.code))
          .toSet
      case "field_declaration" =>
        variableDeclarators(node).flatMap(decl => childByField(decl, "name").map(_.code)).toSet
      case "enhanced_for_statement" =>
        childByField(node, "name").map(_.code).toSet
      case "lambda_expression" =>
        lambdaDeclaredNames(node)
      case _ =>
        Set.empty[String]
    }
    current ++ node.children.flatMap(declaredNames)
  }

  private def lambdaDeclaredNames(node: JavaAstNode): Set[String] = {
    childByField(node, "parameters").toSet.flatMap {
      case param if param.kind == "identifier" => Set(param.code)
      case params =>
        namedChildren(params).flatMap {
          case param if param.kind == "identifier"       => Some(param.code)
          case param if param.kind == "formal_parameter" => childByField(param, "name").map(_.code)
          case _                                         => None
        }.toSet
    }
  }

  private def astsForLocalVariableDeclaration(node: JavaAstNode): Seq[Ast] = {
    val typ = childByField(node, "type").map(typeName).getOrElse(Defines.Any)
    registerType(typ)
    node.children
      .filter(_.kind == "variable_declarator")
      .flatMap { declarator =>
        val name = childByField(declarator, "name").map(_.code).getOrElse(declarator.code.takeWhile(_ != '='))
        val ScopedLocal(local, emitLocal) = scopedLocalForDeclaration(name, typ) { localName =>
          localNode(declarator, localName, s"$typ $localName", typ)
        }
        val assignmentAsts = childByField(declarator, "value").map {
          case initializer if initializer.kind == "object_creation_expression" =>
            astsForLocalObjectCreationInitializer(declarator, initializer, local, name, typ)
          case initializer =>
            val targetNode = identifierNode(declarator, local.name, name, typ)
            val target     = Ast(targetNode).withRefEdge(targetNode, local)
            val rhs        = withExpectedExpressionType(Some(typ))(astForExpression(initializer))
            val call = callNode(
              declarator,
              s"$typ $name = ${initializer.code}",
              Operators.assignment,
              Operators.assignment,
              DispatchTypes.STATIC_DISPATCH,
              None,
              Some(typ)
            )
            Seq(callAst(call, Seq(target, rhs)))
        }
        Option.when(emitLocal)(Ast(local)).toSeq ++ assignmentAsts.toSeq.flatten
      }
  }

  private def astsForExpressionStatement(node: JavaAstNode): Seq[Ast] = {
    node match {
      case assignment if assignment.kind == "assignment_expression" =>
        astsForObjectCreationAssignment(assignment).getOrElse(Seq(astForExpression(assignment)))
      case _ =>
        Seq(astForExpression(node))
    }
  }

  private def astsForObjectCreationAssignment(node: JavaAstNode): Option[Seq[Ast]] = {
    val operator = childByField(node, "operator").map(_.code).getOrElse("=")
    for {
      left  <- childByField(node, "left")
      right <- childByField(node, "right")
      if operator == "=" && left.kind == "identifier" && right.kind == "object_creation_expression"
    } yield {
      val lhsAst          = astForExpression(left)
      val constructedType = constructedTypeForObjectCreation(right)
      val allocAst        = objectCreationAllocAst(right, constructedType)
      val assignCall = callNode(
        node,
        node.code,
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(registerType(firstKnownType(lhsAst, allocAst)))
      )
      val assignAst = callAst(assignCall, Seq(lhsAst, allocAst))

      val receiverAst = receiverAstForAssignmentInit(left)
      val initAst     = objectCreationInitAst(right, constructedType, receiverAst)
      Seq(assignAst, initAst)
    }
  }

  private def astForReturnStatement(node: JavaAstNode): Ast = {
    val returnArguments = namedChildren(node).filterNot(_.kind == "return").map { arg =>
      withExpectedExpressionType(currentMethodReturnTypes.headOption)(astForExpression(arg))
    }
    returnAst(returnNode(node, node.code), returnArguments)
  }

  private def astForYieldStatement(node: JavaAstNode): Ast = {
    val yieldArguments = namedChildren(node).map { arg =>
      withExpectedExpressionType(currentMethodReturnTypes.headOption)(astForExpression(arg))
    }
    returnAst(returnNode(node, node.code), yieldArguments)
  }

  private def astForIfStatement(node: JavaAstNode): Ast = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val conditionNode            = childByField(node, "condition")
    val condition                = conditionNode.map(astForExpression)
    val conditionPatternLocals   = newlyPendingPatternLocals(initialPatternLocalCount)
    val exposure                 = conditionNode.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
    val consequenceNode          = childByField(node, "consequence")
    val alternativeNode          = childByField(node, "alternative")
    val exposeAfter =
      (exposure.thenBranch && !exposure.elseBranch && alternativeNode.exists(definitelyExits)) ||
        (!exposure.thenBranch && exposure.elseBranch && consequenceNode.exists(definitelyExits))
    if (exposure.thenBranch) {
      bindPatternLocals(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    val consequence = consequenceNode
      .map(astForStatementBody)
      .getOrElse(emptyBlockAst(node))
    if (exposure.elseBranch) {
      bindPatternLocals(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    val alternative = alternativeNode.map { alt =>
      val elseNode = controlStructureNode(alt, ControlStructureTypes.ELSE, "else")
      Ast(elseNode).withChild(astForStatementBody(alt))
    }
    if (exposeAfter) {
      bindPatternLocals(conditionPatternLocals)
      markPatternLocalsSurviveStatement(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    ifThenElseAst(node, condition, consequence, alternative, Some(node.code))
  }

  private def astForWhileStatement(node: JavaAstNode): Ast = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val conditionNode            = childByField(node, "condition")
    val conditionAst             = conditionNode.map(astForExpression)
    val conditionPatternLocals   = newlyPendingPatternLocals(initialPatternLocalCount)
    val exposure                 = conditionNode.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
    if (exposure.thenBranch) {
      bindPatternLocals(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    val bodyAst = childByField(node, "body")
      .map(astForStatementBody)
      .getOrElse(emptyBlockAst(node))
    if (exposure.elseBranch) {
      bindPatternLocals(conditionPatternLocals)
      markPatternLocalsSurviveStatement(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    whileAst(
      node,
      conditionAst,
      List(bodyAst),
      Some(s"while (${conditionNode.map(unparenthesizedCode).getOrElse("")})")
    )
  }

  private def astForDoStatement(node: JavaAstNode): Ast = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val conditionNode            = childByField(node, "condition")
    val conditionAst             = conditionNode.map(astForExpression)
    val conditionPatternLocals   = newlyPendingPatternLocals(initialPatternLocalCount)
    val exposure                 = conditionNode.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
    restorePatternLocals(conditionPatternLocals)
    val bodyAst = childByField(node, "body")
      .map(astForStatementBody)
      .getOrElse(emptyBlockAst(node))
    if (exposure.elseBranch) {
      bindPatternLocals(conditionPatternLocals)
      markPatternLocalsSurviveStatement(conditionPatternLocals)
    }
    doWhileAst(
      node,
      conditionAst,
      List(bodyAst),
      Some(s"do {...} while (${conditionNode.map(unparenthesizedCode).getOrElse("")})")
    )
  }

  private def patternBranchExposure(node: JavaAstNode): PatternBranchExposure = {
    node.kind match {
      case "parenthesized_expression" =>
        namedChildren(node).headOption.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
      case "instanceof_expression" if isPatternInstanceOfExpression(node) =>
        PatternBranchExposure(thenBranch = true, elseBranch = false)
      case "unary_expression" if unaryOperatorCode(node).contains("!") =>
        val operand = childByField(node, "operand").orElse(namedChildren(node).lastOption)
        operand.map(patternBranchExposure).getOrElse(PatternBranchExposure.None).swapped
      case "binary_expression" if childByField(node, "operator").exists(_.code == "&&") =>
        val left  = childByField(node, "left").map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
        val right = childByField(node, "right").map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
        PatternBranchExposure(thenBranch = left.thenBranch || right.thenBranch, elseBranch = false)
      case "binary_expression" if childByField(node, "operator").exists(_.code == "||") =>
        val left  = childByField(node, "left").map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
        val right = childByField(node, "right").map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
        PatternBranchExposure(thenBranch = false, elseBranch = left.elseBranch || right.elseBranch)
      case _ =>
        PatternBranchExposure.None
    }
  }

  private def unaryOperatorCode(node: JavaAstNode): Option[String] =
    childByField(node, "operator").map(_.code).orElse(namedChildren(node).headOption.map(_.code))

  private def isPatternInstanceOfExpression(node: JavaAstNode): Boolean =
    childByField(node, "name").isDefined || childByField(node, "pattern").exists(_.kind == "record_pattern")

  private def definitelyExits(node: JavaAstNode): Boolean = {
    node.kind match {
      case "return_statement" | "throw_statement" =>
        true
      case "block" =>
        namedChildren(node).lastOption.exists(definitelyExits)
      case "if_statement" =>
        childByField(node, "consequence").exists(definitelyExits) &&
        childByField(node, "alternative").exists(definitelyExits)
      case _ =>
        false
    }
  }

  private def astForForStatement(node: JavaAstNode): Ast = {
    var conditionPatternLocalsAfterLoop = Seq.empty[PatternLocalInfo]
    val ast = withScope(Nil) {
      val initAsts =
        node.children.filter(_.fieldName.contains("init")).flatMap(astsForForComponent)
      val conditionNodes                    = node.children.filter(_.fieldName.contains("condition"))
      val initialConditionPatternLocalCount = pendingPatternLocals.size
      val conditionAsts =
        conditionNodes.map(astForExpression)
      val conditionPatternLocals = newlyPendingPatternLocals(initialConditionPatternLocalCount)
      val exposure = conditionNodes.headOption.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
      if (exposure.thenBranch) {
        bindPatternLocals(conditionPatternLocals)
      } else {
        restorePatternLocals(conditionPatternLocals)
      }
      val bodyAst = childByField(node, "body")
        .map(astForStatementBody)
        .getOrElse(emptyBlockAst(node))
      val updateAsts =
        node.children.filter(_.fieldName.contains("update")).flatMap(astsForForComponent)
      if (exposure.elseBranch) {
        conditionPatternLocalsAfterLoop = conditionPatternLocals
      } else if (exposure.thenBranch) {
        restorePatternLocals(conditionPatternLocals)
      }
      forAst(
        node,
        Nil,
        initAsts,
        conditionAsts,
        updateAsts,
        Seq(bodyAst),
        Some(declarationHeader(node))
      )
    }
    if (conditionPatternLocalsAfterLoop.nonEmpty) {
      bindPatternLocals(conditionPatternLocalsAfterLoop)
      markPatternLocalsSurviveStatement(conditionPatternLocalsAfterLoop)
    }
    ast
  }

  private def astForEnhancedForStatement(node: JavaAstNode): Ast = {
    withScope(Nil) {
      val typ  = childByField(node, "type").map(typeName).getOrElse(Defines.Any)
      val name = childByField(node, "name").map(_.code).getOrElse("value")
      registerType(typ)
      val ScopedLocal(local, emitLocal) = scopedLocalForDeclaration(name, typ) { localName =>
        localNode(node, localName, s"$typ $localName", typ)
      }
      val localAsts   = Option.when(emitLocal)(Ast(local)).toSeq
      val iterableAst = childByField(node, "value").map(astForExpression).toSeq
      val bodyAst = childByField(node, "body")
        .map(astForStatementBody)
        .getOrElse(emptyBlockAst(node))
      forAst(
        node,
        localAsts,
        Nil,
        iterableAst,
        Nil,
        Seq(bodyAst),
        Some(declarationHeader(node))
      )
    }
  }

  private def astsForForComponent(node: JavaAstNode): Seq[Ast] = {
    node.kind match {
      case "local_variable_declaration" => astsForLocalVariableDeclaration(node)
      case "expression_statement"       => namedChildren(node).headOption.map(astForExpression).toSeq
      case _                            => Seq(astForExpression(node))
    }
  }

  private def astForSwitchExpression(node: JavaAstNode, controlStructureType: String): Ast = {
    val conditionNode = childByField(node, "condition").getOrElse(node)
    val conditionAst  = astForExpression(conditionNode)
    val switchCode    = s"switch(${unparenthesizedCode(conditionNode)})"
    val bodyAst = childByField(node, "body")
      .map(astForSwitchBody(_, controlStructureType, conditionNode))
      .getOrElse(emptyBlockAst(node))
    if (controlStructureType == ControlStructureTypes.MATCH)
      matchAst(node, Some(conditionAst), List(bodyAst), Some(switchCode))
    else
      switchAst(node, Some(conditionAst), List(bodyAst), Some(switchCode))
  }

  private def astForSwitchBody(node: JavaAstNode, controlStructureType: String, selectorNode: JavaAstNode): Ast = {
    blockAst(
      blockNode(node),
      namedChildren(node).flatMap(astsForSwitchEntry(_, controlStructureType, selectorNode)).toList
    )
  }

  private def astsForSwitchEntry(
    node: JavaAstNode,
    controlStructureType: String,
    selectorNode: JavaAstNode
  ): Seq[Ast] = {
    val labelNodes = namedChildren(node).filter(_.kind == "switch_label")
    labelNodes
      .collectFirst(Function.unlift(switchPatternNode))
      .map { case (labelNode, patternNode) =>
        astsForSwitchPatternEntry(node, labelNode, patternNode, selectorNode)
      }
      .getOrElse {
        val labelAsts = labelNodes.flatMap(astsForSwitchLabel)
        val statementAsts = namedChildren(node)
          .filterNot(_.kind == "switch_label")
          .flatMap(astsForStatement)
        if (statementAsts.isEmpty) {
          labelAsts
        } else if (
          controlStructureType == ControlStructureTypes.MATCH && node.kind == "switch_rule" && statementAsts.size == 1
        ) {
          labelAsts ++ statementAsts
        } else {
          labelAsts :+ blockAst(blockNode(node), statementAsts.toList)
        }
      }
  }

  private def astsForSwitchPatternEntry(
    entryNode: JavaAstNode,
    labelNode: JavaAstNode,
    patternNode: JavaAstNode,
    selectorNode: JavaAstNode
  ): Seq[Ast] = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val conditionAst             = astForSwitchPatternCondition(labelNode, patternNode, selectorNode)
    val patternLocals            = newlyPendingPatternLocals(initialPatternLocalCount)
    val patternLocalAsts         = patternLocals.reverse.filter(_.emitLocal).map(info => Ast(info.local))
    bindPatternLocals(patternLocals)
    val statementAsts = namedChildren(entryNode)
      .filterNot(_.kind == "switch_label")
      .flatMap(astsForStatement)
    val ifBody = switchGuardExpressionNode(labelNode)
      .map { guardNode =>
        val guardAst  = astForExpression(guardNode)
        val guardBody = blockAst(blockNode(entryNode), statementAsts.toList)
        val guardIf = ifThenElseAst(
          guardNode,
          Some(guardAst),
          guardBody,
          None,
          Some(s"if (${guardNode.code})")
        )
        blockAst(blockNode(entryNode), List(guardIf))
      }
      .getOrElse(blockAst(blockNode(entryNode), statementAsts.toList))
    restorePatternLocals(patternLocals)
    pendingPatternLocals = pendingPatternLocals.drop(patternLocals.size)

    val ifAst = ifThenElseAst(
      labelNode,
      Some(conditionAst),
      ifBody,
      None,
      Some(s"if (${astRootCode(conditionAst)})")
    )
    Seq(
      Ast(jumpTargetNode(labelNode, "case", patternNode.code)),
      blockAst(blockNode(entryNode), (patternLocalAsts :+ ifAst).toList)
    )
  }

  private def astForSwitchPatternCondition(
    labelNode: JavaAstNode,
    patternNode: JavaAstNode,
    selectorNode: JavaAstNode
  ): Ast = {
    val selectorExpressionNode = unparenthesizedExpressionNode(selectorNode)
    patternNode.kind match {
      case "type_pattern" =>
        val children = namedChildren(patternNode)
        val typeNode = children.find(_.kind != "identifier").getOrElse(patternNode)
        val nameNode = children.reverse.find(_.kind == "identifier").getOrElse(patternNode)
        astForTypePatternExpression(labelNode, selectorExpressionNode, typeNode, nameNode)
      case "record_pattern" =>
        astForRecordPatternExpression(labelNode, selectorExpressionNode, patternNode)
      case _ =>
        astForExpression(patternNode)
    }
  }

  private def astsForSwitchLabel(node: JavaAstNode): Seq[Ast] = {
    val labelExpressions = namedChildren(node)
    if (node.children.exists(_.kind == "default") || labelExpressions.isEmpty) {
      Seq(Ast(jumpTargetNode(node, "default", "default")))
    } else {
      labelExpressions.flatMap { label =>
        Seq(Ast(jumpTargetNode(label, "case", label.code)), astForExpression(label))
      }
    }
  }

  private def switchPatternNode(labelNode: JavaAstNode): Option[(JavaAstNode, JavaAstNode)] = {
    namedChildren(labelNode).find(_.kind == "pattern").flatMap { patternWrapper =>
      namedChildren(patternWrapper).headOption.collect {
        case patternNode if patternNode.kind == "type_pattern" || patternNode.kind == "record_pattern" =>
          labelNode -> patternNode
      }
    }
  }

  private def switchGuardExpressionNode(labelNode: JavaAstNode): Option[JavaAstNode] =
    namedChildren(labelNode).find(_.kind == "guard").flatMap(namedChildren(_).headOption)

  private def unparenthesizedExpressionNode(node: JavaAstNode): JavaAstNode =
    if (node.kind == "parenthesized_expression") namedChildren(node).headOption.getOrElse(node) else node

  private def astsForTryStatement(node: JavaAstNode): Seq[Ast] = {
    withScope(Nil) {
      val resourceAsts = node.children
        .filter(_.fieldName.exists(_.contains("resource")))
        .flatMap(astsForTryResource)
      val tryBody = childByField(node, "body")
        .map(astForBlock(_, codeOverride = Some("try")))
        .getOrElse(emptyBlockAst(node))
      val catchAsts  = namedChildren(node).filter(_.kind == "catch_clause").map(astForCatchClause)
      val finallyAst = namedChildren(node).find(_.kind == "finally_clause").map(astForFinallyClause)
      resourceAsts :+ tryCatchAst(node, tryBody, catchAsts, finallyAst, Some("try"))
    }
  }

  private def astsForTryResource(node: JavaAstNode): Seq[Ast] = {
    node.kind match {
      case "resource_specification" =>
        namedChildren(node).flatMap(astsForTryResource)
      case "resource" if childByField(node, "type").isDefined =>
        astsForResourceDeclaration(node)
      case "local_variable_declaration" =>
        astsForLocalVariableDeclaration(node)
      case "resource" =>
        namedChildren(node).map(astForExpression)
      case _ =>
        astsForForComponent(node)
    }
  }

  private def astsForResourceDeclaration(node: JavaAstNode): Seq[Ast] = {
    val typeNode = childByField(node, "type")
    val typ      = typeNode.map(typeName).getOrElse(Defines.Any)
    val typeCode = typeNode.map(_.code).getOrElse(typ)
    val name     = childByField(node, "name").map(_.code).getOrElse(node.code.takeWhile(_ != '=').trim)
    registerType(typ)
    val ScopedLocal(local, emitLocal) = scopedLocalForDeclaration(name, typ) { localName =>
      localNode(node, localName, s"$typeCode $localName", typ)
    }
    val assignmentAsts = childByField(node, "value").map {
      case initializer if initializer.kind == "object_creation_expression" =>
        astsForLocalObjectCreationInitializer(node, initializer, local, name, typ, typeCode)
      case initializer =>
        val target    = identifierNode(node, local.name, name, typ)
        val targetAst = Ast(target).withRefEdge(target, local)
        val rhs       = withExpectedExpressionType(Some(typ))(astForExpression(initializer))
        val call = callNode(
          node,
          s"$typeCode $name = ${initializer.code}",
          Operators.assignment,
          Operators.assignment,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Some(typ)
        )
        Seq(callAst(call, Seq(targetAst, rhs)))
    }
    Option.when(emitLocal)(Ast(local)).toSeq ++ assignmentAsts.toSeq.flatten
  }

  private def astForCatchClause(node: JavaAstNode): Ast = {
    val parameter = namedChildren(node).find(_.kind == "catch_formal_parameter")
    val parameterBinding = parameter.map { param =>
      val typ  = namedChildren(param).find(_.kind == "catch_type").map(typeName).getOrElse(Defines.Any)
      val name = childByField(param, "name").map(_.code).getOrElse("ex")
      registerType(typ)
      name -> typ
    }
    val parameterLocalAst = parameter.zip(parameterBinding).map { case (param, (name, typ)) =>
      Ast(localNode(param, name, s"$typ $name", typ))
    }
    val body = withScope(parameterBinding.toList) {
      childByField(node, "body")
        .map(astForBlock(_, prefixAsts = parameterLocalAst.toSeq))
        .getOrElse(emptyBlockAst(node))
    }
    Ast(controlStructureNode(node, ControlStructureTypes.CATCH, "catch")).withChild(body)
  }

  private def astForFinallyClause(node: JavaAstNode): Ast = {
    val body = namedChildren(node)
      .find(_.kind == "block")
      .map(astForBlock(_, codeOverride = Some("finally")))
      .getOrElse(emptyBlockAst(node))
    Ast(controlStructureNode(node, ControlStructureTypes.FINALLY, "finally")).withChild(body)
  }

  private def astForThrowStatement(node: JavaAstNode): Ast = {
    val args = namedChildren(node).map(astForExpression)
    val op   = "<operator>.throw"
    val call = callNode(node, node.code, op, op, DispatchTypes.STATIC_DISPATCH, None, Some(Defines.Any))
    callAst(call, args)
  }

  private def astForSynchronizedStatement(node: JavaAstNode): Ast = {
    val lockAst = namedChildren(node).find(_.kind != "block").map(astForExpression).toSeq
    val bodyAst = childByField(node, "body")
      .map(block => astForBlock(block))
      .getOrElse(emptyBlockAst(node))
    Ast(NewBlock().lineNumber(line(node)).columnNumber(column(node)))
      .withChild(Ast(modifierNode(node, "SYNCHRONIZED")))
      .withChildren(lockAst)
      .withChild(bodyAst)
  }

  private def astForAssertStatement(node: JavaAstNode): Ast = {
    val args = namedChildren(node).headOption
      .map(condition => withExpectedExpressionType(Some("boolean"))(astForExpression(condition)))
      .toSeq
    val call = callNode(node, node.code, "assert", "assert", DispatchTypes.STATIC_DISPATCH, None, None)
    callAst(call, args)
  }

  private def astsForLabeledStatement(node: JavaAstNode): Seq[Ast] = {
    val children = namedChildren(node)
    val labelAst = children.headOption
      .map(label => Ast(jumpTargetNode(label, label.code, label.code)))
      .toSeq
    val statementAsts = children.drop(1).headOption.toSeq.flatMap(astsForStatement)
    labelAst ++ statementAsts
  }

  private def astForBreakOrContinueStatement(node: JavaAstNode, controlStructureType: String): Ast = {
    Ast(controlStructureNode(node, controlStructureType, node.code))
  }

  private def astForExplicitConstructorInvocation(node: JavaAstNode): Ast = {
    val receiverType =
      if (isSuperConstructorInvocation(node)) currentSuperTypeFullName
      else lookupType("this")
    val receiverAst = Ast(identifierNode(node, "this", "this", receiverType))
    val args        = childByField(node, "arguments").map(argumentAsts(_)).getOrElse(Nil)
    val captureArgs =
      Option.when(isThisConstructorInvocation(node))(localRecordCaptureArgumentAsts(receiverType, node)).getOrElse(Nil)
    val signature = composeSignature("void", args.map(expressionType))
    val methodFullName =
      if (receiverType == Defines.Any) Defines.ConstructorMethodName
      else s"$receiverType.${Defines.ConstructorMethodName}:$signature"
    val call = callNode(
      node,
      node.code.stripSuffix(";"),
      Defines.ConstructorMethodName,
      methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(signature),
      Some("void")
    )
    callAst(call, args ++ captureArgs, Some(receiverAst))
  }

  private def isThisConstructorInvocation(node: JavaAstNode): Boolean =
    childByField(node, "constructor").exists(_.kind == "this") || node.code.trim.startsWith("this")

  private def isSuperConstructorInvocation(node: JavaAstNode): Boolean =
    childByField(node, "constructor").exists(_.kind == "super") || node.code.trim.startsWith("super")

  private def currentSuperTypeFullName: String =
    currentTypeInherits.headOption.flatMap(_.headOption).getOrElse(Defines.Any)

  private def astForStatementBody(node: JavaAstNode): Ast = {
    node.kind match {
      case "block" => astForBlock(node)
      case _ =>
        val block = blockNode(node, "", Defines.Any)
        blockAst(block, astsForStatement(node).toList)
    }
  }

  private def astForExpression(node: JavaAstNode): Ast = {
    node.kind match {
      case "identifier"                 => astForIdentifier(node)
      case "this"                       => astForThisExpression(node)
      case "super"                      => astForSuperExpression(node)
      case kind if LiteralKinds(kind)   => astForLiteral(node)
      case "parenthesized_expression"   => namedChildren(node).headOption.map(astForExpression).getOrElse(Ast())
      case "binary_expression"          => astForBinaryExpression(node)
      case "unary_expression"           => astForUnaryExpression(node)
      case "ternary_expression"         => astForTernaryExpression(node)
      case "cast_expression"            => astForCastExpression(node)
      case "instanceof_expression"      => astForInstanceOfExpression(node)
      case "assignment_expression"      => astForAssignmentExpression(node)
      case "update_expression"          => astForUpdateExpression(node)
      case "field_access"               => astForFieldAccess(node)
      case "array_access"               => astForArrayAccess(node)
      case "method_invocation"          => astForMethodInvocation(node)
      case "method_reference"           => astForMethodReference(node)
      case "lambda_expression"          => astForLambdaExpression(node)
      case "object_creation_expression" => astForObjectCreationExpression(node)
      case "array_creation_expression"  => astForArrayCreationExpression(node)
      case "array_initializer"          => astForArrayInitializer(node)
      case "switch_expression"          => astForSwitchExpression(node, ControlStructureTypes.MATCH)
      case "class_literal"              => astForClassLiteral(node)
      case _                            => namedChildren(node).headOption.map(astForExpression).getOrElse(Ast())
    }
  }

  private def astForIdentifier(node: JavaAstNode): Ast = {
    val name = node.code
    localRef(name)
      .map { local =>
        val identifier = identifierNode(node, local.name, name, lookupType(name))
        Ast(identifier).withRefEdge(identifier, local)
      }
      .orElse(parameterRef(name).map { target =>
        val identifier = identifierNode(node, name, name, lookupType(name))
        Ast(identifier).withRefEdge(identifier, target)
      })
      .orElse(implicitMemberAccessAst(node, name))
      .getOrElse(Ast(identifierNode(node, name, name, lookupTypeOrTypeName(name))))
  }

  private def implicitMemberAccessAst(node: JavaAstNode, fieldName: String): Option[Ast] = {
    val thisType = lookupType("this")
    val thisAccess = Option
      .when(thisType != Defines.Any) {
        thisIdentifierAst(node, "this", thisType)
      }
      .flatten
    thisAccess
      .flatMap(memberAccessAstForType(node, _, thisType, fieldName, "this"))
      .orElse {
        for {
          baseAst   <- thisAccess
          fieldType <- memberTypeNames.get(fieldName)
        } yield fieldAccessAst(node, node, baseAst, s"this.$fieldName", fieldName, registerType(fieldType))
      }
  }

  private def memberAccessAstForType(
    node: JavaAstNode,
    receiverAst: Ast,
    receiverType: String,
    fieldName: String,
    receiverCode: String,
    visitedTypes: Set[String] = Set.empty
  ): Option[Ast] = {
    if (visitedTypes.contains(receiverType)) {
      None
    } else {
      memberTypeForDeclaredOrInheritedType(receiverType, fieldName)
        .map { fieldType =>
          fieldAccessAst(node, node, receiverAst, s"$receiverCode.$fieldName", fieldName, registerType(fieldType))
        }
        .orElse {
          localRecordCaptureInfosByType.get(receiverType).flatMap(_.find(_.name == "outerClass")).flatMap { capture =>
            val outerCode = s"$receiverCode.outerClass"
            val outerAst =
              fieldAccessAst(node, node, receiverAst, outerCode, "outerClass", registerType(capture.typeFullName))
            memberAccessAstForType(
              node,
              outerAst,
              capture.typeFullName,
              fieldName,
              outerCode,
              visitedTypes + receiverType
            )
          }
        }
    }
  }

  private def memberTypeForType(
    receiverType: String,
    fieldName: String,
    visitedTypes: Set[String] = Set.empty
  ): Option[String] = {
    if (visitedTypes.contains(receiverType)) {
      None
    } else {
      memberTypeForDeclaredOrInheritedType(receiverType, fieldName)
        .orElse {
          localRecordCaptureInfosByType.get(receiverType).flatMap(_.find(_.name == "outerClass")).flatMap { capture =>
            memberTypeForType(capture.typeFullName, fieldName, visitedTypes + receiverType)
          }
        }
    }
  }

  private def thisIdentifierAst(node: JavaAstNode, code: String, typeFullName: String): Option[Ast] = {
    Option.when(typeFullName != Defines.Any) {
      val thisIdentifier = identifierNode(node, "this", code, typeFullName)
      localRef("this")
        .map(local => Ast(thisIdentifier).withRefEdge(thisIdentifier, local))
        .orElse(parameterRef("this").map(target => Ast(thisIdentifier).withRefEdge(thisIdentifier, target)))
        .getOrElse(Ast(thisIdentifier))
    }
  }

  private def implicitThisReceiverAst(node: JavaAstNode): Option[Ast] = {
    val thisType = lookupType("this")
    thisReceiverAst(node, "this", thisType)
  }

  private def implicitReceiverAstForMethod(node: JavaAstNode, methodInfo: MethodSignatureInfo): Option[Ast] = {
    val thisType   = lookupType("this")
    val targetType = ownerTypeForMethodInfo(methodInfo)
    implicitThisReceiverAst(node).flatMap { thisAst =>
      if (targetType == thisType || targetType == Defines.Any) {
        Some(thisAst)
      } else {
        outerClassReceiverAstForType(node, thisAst, thisType, targetType, "this")
      }
    }
  }

  private def outerClassReceiverAstForType(
    node: JavaAstNode,
    baseAst: Ast,
    currentType: String,
    targetType: String,
    code: String,
    visitedTypes: Set[String] = Set.empty
  ): Option[Ast] = {
    if (visitedTypes.contains(currentType)) {
      None
    } else {
      localRecordCaptureInfosByType.get(currentType).flatMap(_.find(_.name == "outerClass")).flatMap { capture =>
        val outerCode = s"$code.outerClass"
        val outerAst  = fieldAccessAst(node, node, baseAst, outerCode, "outerClass", registerType(capture.typeFullName))
        if (capture.typeFullName == targetType) {
          Some(outerAst)
        } else {
          outerClassReceiverAstForType(
            node,
            outerAst,
            capture.typeFullName,
            targetType,
            outerCode,
            visitedTypes + currentType
          )
        }
      }
    }
  }

  private def thisReceiverAst(node: JavaAstNode, code: String, typeFullName: String): Option[Ast] = {
    if (typeFullName != Defines.Any && (localRef("this").isDefined || parameterRef("this").isDefined)) {
      thisIdentifierAst(node, code, typeFullName)
    } else {
      None
    }
  }

  private def astForThisExpression(node: JavaAstNode): Ast = {
    thisIdentifierAst(node, node.code, lookupType("this")).getOrElse(
      Ast(identifierNode(node, "this", node.code, lookupType("this")))
    )
  }

  private def astForSuperExpression(node: JavaAstNode): Ast = {
    thisIdentifierAst(node, node.code, currentSuperTypeFullName).getOrElse(
      Ast(identifierNode(node, "this", node.code, currentSuperTypeFullName))
    )
  }

  private def astForLiteral(node: JavaAstNode): Ast = {
    val typ = literalTypeName(node)
    registerType(typ)
    Ast(literalNode(node, node.code, typ))
  }

  private def astForBinaryExpression(node: JavaAstNode): Ast = {
    val operator                 = childByField(node, "operator").map(_.code).getOrElse("")
    val leftNode                 = childByField(node, "left")
    val initialPatternLocalCount = pendingPatternLocals.size
    val lhs                      = leftNode.map(astForExpression).getOrElse(Ast())
    val leftPatternLocals        = newlyPendingPatternLocals(initialPatternLocalCount)
    val leftExposure             = leftNode.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)
    val exposeLeftLocalsToRight = operator match {
      case "&&" => leftExposure.thenBranch
      case "||" => leftExposure.elseBranch
      case _    => false
    }
    if (exposeLeftLocalsToRight) {
      bindPatternLocals(leftPatternLocals)
    } else {
      restorePatternLocals(leftPatternLocals)
    }
    val rhs = childByField(node, "right").map(astForExpression).getOrElse(Ast())
    restorePatternLocals(newlyPendingPatternLocals(initialPatternLocalCount))
    val op   = binaryOperatorName(operator)
    val typ  = if (BooleanBinaryOperators.contains(operator)) "boolean" else firstKnownType(lhs, rhs)
    val call = callNode(node, node.code, op, op, DispatchTypes.STATIC_DISPATCH, None, Some(registerType(typ)))
    callAst(call, Seq(lhs, rhs))
  }

  private def astForUnaryExpression(node: JavaAstNode): Ast = {
    val operand =
      childByField(node, "operand").orElse(namedChildren(node).lastOption).map(astForExpression).getOrElse(Ast())
    val operator =
      childByField(node, "operator").map(_.code).orElse(namedChildren(node).headOption.map(_.code)).getOrElse("")
    val op = operator match {
      case "!" => Operators.logicalNot
      case "~" => Operators.not
      case "-" => Operators.minus
      case "+" => Operators.plus
      case _   => Defines.Unknown
    }
    val typ  = if (operator == "!") "boolean" else expressionType(operand)
    val call = callNode(node, node.code, op, op, DispatchTypes.STATIC_DISPATCH, None, Some(registerType(typ)))
    callAst(call, Seq(operand))
  }

  private def astForTernaryExpression(node: JavaAstNode): Ast = {
    val initialPatternLocalCount = pendingPatternLocals.size
    val conditionNode            = childByField(node, "condition")
    val conditionAst             = conditionNode.map(astForExpression).getOrElse(Ast())
    val conditionPatternLocals   = newlyPendingPatternLocals(initialPatternLocalCount)
    val exposure                 = conditionNode.map(patternBranchExposure).getOrElse(PatternBranchExposure.None)

    if (exposure.thenBranch) {
      bindPatternLocals(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    val consequenceInitialPatternLocalCount = pendingPatternLocals.size
    val consequenceAst                      = childByField(node, "consequence").map(astForExpression).getOrElse(Ast())
    restorePatternLocals(newlyPendingPatternLocals(consequenceInitialPatternLocalCount))

    if (exposure.elseBranch) {
      bindPatternLocals(conditionPatternLocals)
    } else {
      restorePatternLocals(conditionPatternLocals)
    }
    val alternativeInitialPatternLocalCount = pendingPatternLocals.size
    val alternativeAst                      = childByField(node, "alternative").map(astForExpression).getOrElse(Ast())
    restorePatternLocals(newlyPendingPatternLocals(alternativeInitialPatternLocalCount))
    restorePatternLocals(conditionPatternLocals)

    val typ = firstKnownType(consequenceAst, alternativeAst)
    val call = callNode(
      node,
      node.code,
      Operators.conditional,
      Operators.conditional,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(registerType(typ))
    )
    callAst(call, Seq(conditionAst, consequenceAst, alternativeAst))
  }

  private def astForCastExpression(node: JavaAstNode): Ast = {
    val typeNode = childByField(node, "type").getOrElse(node)
    val typ      = registerType(typeName(typeNode))
    val typeAst  = Ast(typeRefNode(typeNode, typeNode.code, typ))
    val valueAst = childByField(node, "value").map(astForExpression).getOrElse(Ast())
    val call = callNode(node, node.code, Operators.cast, Operators.cast, DispatchTypes.STATIC_DISPATCH, None, Some(typ))
    callAst(call, Seq(typeAst, valueAst))
  }

  private def astForInstanceOfExpression(node: JavaAstNode): Ast = {
    childByField(node, "name") match {
      case Some(nameNode) => return astForTypePatternInstanceOfExpression(node, nameNode)
      case None           =>
    }
    childByField(node, "pattern") match {
      case Some(patternNode) if patternNode.kind == "record_pattern" =>
        return astForRecordPatternInstanceOfExpression(node, patternNode)
      case _ =>
    }
    val lhsAst   = childByField(node, "left").map(astForExpression).getOrElse(Ast())
    val typeNode = childByField(node, "right").getOrElse(node)
    val typ      = registerType(typeName(typeNode))
    val typeAst  = Ast(typeRefNode(typeNode, typeNode.code, typ))
    val call =
      callNode(
        node,
        node.code,
        Operators.instanceOf,
        Operators.instanceOf,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some("boolean")
      )
    callAst(call, Seq(lhsAst, typeAst))
  }

  private def astForTypePatternInstanceOfExpression(node: JavaAstNode, nameNode: JavaAstNode): Ast = {
    val lhsNode  = childByField(node, "left").getOrElse(node)
    val typeNode = childByField(node, "right").getOrElse(node)
    astForTypePatternExpression(node, lhsNode, typeNode, nameNode)
  }

  private def astForTypePatternExpression(
    node: JavaAstNode,
    lhsNode: JavaAstNode,
    typeNode: JavaAstNode,
    nameNode: JavaAstNode
  ): Ast = {
    val patternType = registerType(patternTypeName(typeNode))
    val patternName = nameNode.code
    val lhsAccess   = typePatternLhsAccess(node, lhsNode)

    val typeCheckAccess = lhsAccess()
    val typeCheckCode   = s"${recordPatternInstanceOfLhsCode(typeCheckAccess)} instanceof ${typeNode.code}"
    val typeCheckCall =
      callNode(
        node,
        typeCheckCode,
        Operators.instanceOf,
        Operators.instanceOf,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some("boolean")
      )
    val typeCheckAst =
      callAst(typeCheckCall, Seq(typeCheckAccess.ast, Ast(typeRefNode(typeNode, typeNode.code, patternType))))

    val patternLocal = declarePatternLocal(
      patternName,
      patternType,
      localName => localNode(nameNode, localName, s"${typeNode.code} $localName", patternType)
    )
    val castAccess = lhsAccess()
    val castCode   = s"(${typeNode.code}) ${castAccess.code}"
    val castCall =
      callNode(node, castCode, Operators.cast, Operators.cast, DispatchTypes.STATIC_DISPATCH, None, Some(patternType))
    val castAst = callAst(castCall, Seq(Ast(typeRefNode(typeNode, typeNode.code, patternType)), castAccess.ast))

    val patternIdentifier    = identifierNode(nameNode, patternLocal.name, patternName, patternType)
    val patternIdentifierAst = Ast(patternIdentifier).withRefEdge(patternIdentifier, patternLocal)
    val assignmentCode       = s"$patternName = $castCode"
    val assignmentCall =
      callNode(
        node,
        assignmentCode,
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(patternType)
      )
    val assignmentAst       = callAst(assignmentCall, Seq(patternIdentifierAst, castAst))
    val trueAst             = Ast(literalNode(node, "true", "boolean"))
    val assignmentBlockCode = s"{ $assignmentCode; true; }"
    val assignmentBlock =
      blockAst(blockNode(node, assignmentBlockCode, "boolean"), List(assignmentAst, trueAst))

    val logicalAndCode = s"($typeCheckCode) && $assignmentBlockCode"
    val logicalAndCall =
      callNode(
        node,
        logicalAndCode,
        Operators.logicalAnd,
        Operators.logicalAnd,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some("boolean")
      )
    callAst(logicalAndCall, Seq(typeCheckAst, assignmentBlock))
  }

  private def typePatternLhsAccess(node: JavaAstNode, lhsNode: JavaAstNode): () => RecordPatternAccess = {
    val lhsType = Option(expressionTypeNameForSignature(lhsNode)).filter(_ != Defines.Any).getOrElse(Defines.Any)
    val requiresTemporary = lhsNode.kind != "identifier" && lhsNode.kind != "field_access"
    var firstUse          = true
    lazy val tmpName      = nextObjectCreationTempName()
    lazy val tmpLocal     = localNode(lhsNode, tmpName, tmpName, lhsType)

    () =>
      if (!requiresTemporary) {
        val ast = astForExpression(lhsNode)
        RecordPatternAccess(ast, lhsNode.code, expressionType(ast))
      } else if (firstUse) {
        firstUse = false
        val initializer     = astForExpression(lhsNode)
        val initializerCode = astRootCode(initializer)
        declarePatternLocal(tmpName, lhsType, _ => tmpLocal)
        val tmpIdentifier    = identifierNode(node, tmpName, tmpName, lhsType)
        val tmpIdentifierAst = Ast(tmpIdentifier).withRefEdge(tmpIdentifier, tmpLocal)
        val assignmentCode   = s"$tmpName = $initializerCode"
        val assignmentCall =
          callNode(
            node,
            assignmentCode,
            Operators.assignment,
            Operators.assignment,
            DispatchTypes.STATIC_DISPATCH,
            None,
            Some(lhsType)
          )
        RecordPatternAccess(callAst(assignmentCall, Seq(tmpIdentifierAst, initializer)), assignmentCode, lhsType)
      } else {
        val tmpIdentifier = identifierNode(node, tmpName, tmpName, lhsType)
        RecordPatternAccess(Ast(tmpIdentifier).withRefEdge(tmpIdentifier, tmpLocal), tmpName, lhsType)
      }
  }

  private def astForRecordPatternInstanceOfExpression(node: JavaAstNode, patternNode: JavaAstNode): Ast = {
    val lhsNode = childByField(node, "left").getOrElse(node)
    astForRecordPatternExpression(node, lhsNode, patternNode)
  }

  private def astForRecordPatternExpression(node: JavaAstNode, lhsNode: JavaAstNode, patternNode: JavaAstNode): Ast = {
    val typeNode    = recordPatternTypeNode(patternNode).getOrElse(patternNode)
    val patternType = registerType(patternTypeName(typeNode))

    val lhsInitType = Option(expressionTypeNameForSignature(lhsNode)).filter(_ != Defines.Any)
    val rootInit = new RecordPatternInitNode(
      patternNode,
      patternType,
      typeNode,
      lhsInitType,
      () => RecordPatternAccess(astForExpression(lhsNode), lhsNode.code, lhsInitType.getOrElse(Defines.Any)),
      requiresTemporaryVariable = recordPatternRootRequiresTemporary(lhsNode),
      isRoot = true
    )
    val typePatternInits = mutable.ListBuffer.empty[RecordPatternInitNode]
    val typeCheckAst     = recordPatternTypeCheckAst(patternNode, rootInit, typePatternInits).getOrElse(Ast())

    val assignmentEntries   = typePatternInits.toList.flatMap(recordPatternAssignmentEntry)
    val trueAst             = Ast(literalNode(patternNode, "true", "boolean"))
    val assignmentAsts      = assignmentEntries.map(_._1)
    val assignmentBlockAsts = assignmentAsts :+ trueAst
    val assignmentBlockCode = s"{ ${(assignmentEntries.map(_._2) :+ "true").mkString("; ")}; }"
    val assignmentBlock =
      blockAst(blockNode(patternNode, assignmentBlockCode, "boolean"), assignmentBlockAsts.toList)

    val logicalAndCode = s"(${astRootCode(typeCheckAst)}) && $assignmentBlockCode"
    val logicalAndCall =
      callNode(
        node,
        logicalAndCode,
        Operators.logicalAnd,
        Operators.logicalAnd,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some("boolean")
      )
    callAst(logicalAndCall, Seq(typeCheckAst, assignmentBlock))
  }

  private def recordPatternTypeCheckAst(
    patternNode: JavaAstNode,
    initNode: RecordPatternInitNode,
    typePatternInits: mutable.ListBuffer[RecordPatternInitNode]
  ): Option[Ast] = {
    val instanceOfRequired =
      initNode.isRoot || !isResolvedTypeFullName(initNode.patternTypeFullName) ||
        !initNode.typeFullName.contains(initNode.patternTypeFullName)
    val instanceOfAst = Option.when(instanceOfRequired)(recordPatternInstanceOfAst(patternNode, initNode))

    if (recordPatternIsTypePattern(patternNode)) {
      instanceOfAst
    } else {
      val fieldInitNodes = recordPatternFieldInitNodes(patternNode, initNode)
      val fieldInstanceOfAsts = fieldInitNodes.flatMap { fieldInitNode =>
        if (recordPatternIsTypePattern(fieldInitNode.patternNode)) {
          typePatternInits.append(fieldInitNode)
        }
        recordPatternTypeCheckAst(fieldInitNode.patternNode, fieldInitNode, typePatternInits)
      }
      combineRecordPatternTypeChecks(patternNode, instanceOfAst.toSeq ++ fieldInstanceOfAsts)
    }
  }

  private def combineRecordPatternTypeChecks(node: JavaAstNode, checks: Seq[Ast]): Option[Ast] = {
    checks.reverse match {
      case Nil => None
      case accumulator +: rest =>
        Some {
          rest.foldLeft(accumulator) { case (accumulatorAst, astToAdd) =>
            val logicalAndCode = s"(${astRootCode(astToAdd)}) && (${astRootCode(accumulatorAst)})"
            val logicalAndCall =
              callNode(
                node,
                logicalAndCode,
                Operators.logicalAnd,
                Operators.logicalAnd,
                DispatchTypes.STATIC_DISPATCH,
                None,
                Some("boolean")
              )
            callAst(logicalAndCall, Seq(astToAdd, accumulatorAst))
          }
        }
    }
  }

  private def recordPatternFieldInitNodes(
    patternNode: JavaAstNode,
    parentInitNode: RecordPatternInitNode
  ): Seq[RecordPatternInitNode] = {
    val recordParams = recordParameterInfosByType.getOrElse(parentInitNode.patternTypeFullName, Nil)
    recordPatternComponents(patternNode).zipWithIndex.flatMap { case (componentNode, index) =>
      if (recordPatternIsMatchAll(componentNode)) {
        None
      } else {
        for {
          childTypeNode <- recordPatternTypeNode(componentNode)
        } yield {
          val childPatternType = registerType(patternTypeName(childTypeNode))
          val accessorName     = recordParams.lift(index).map(_.name).getOrElse(Defines.UnknownField)
          val fieldType        = recordParams.lift(index).map(_.typeFullName)
          val childType        = Option(childPatternType).filter(isResolvedTypeFullName)
          val childIsBranching =
            componentNode.kind == "record_pattern" && recordPatternComponents(componentNode).size > 1
          val requiresTemporaryVariable = childIsBranching || childType.isEmpty || childType != fieldType
          new RecordPatternInitNode(
            componentNode,
            childPatternType,
            childTypeNode,
            fieldType,
            () =>
              recordPatternAccessorAccess(
                componentNode,
                parentInitNode,
                accessorName,
                fieldType.getOrElse(Defines.Any)
              ),
            requiresTemporaryVariable
          )
        }
      }
    }
  }

  private def recordPatternInstanceOfAst(patternNode: JavaAstNode, initNode: RecordPatternInitNode): Ast = {
    val initializer    = initNode.getAccess()
    val lhsCode        = recordPatternInstanceOfLhsCode(initializer)
    val typeNode       = initNode.patternTypeNode
    val typeCode       = typeNode.code
    val instanceOfCode = s"$lhsCode instanceof $typeCode"
    val instanceOfCall =
      callNode(
        patternNode,
        instanceOfCode,
        Operators.instanceOf,
        Operators.instanceOf,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some("boolean")
      )
    callAst(instanceOfCall, Seq(initializer.ast, Ast(typeRefNode(typeNode, typeCode, initNode.patternTypeFullName))))
  }

  private def recordPatternAccessorAccess(
    node: JavaAstNode,
    parentInitNode: RecordPatternInitNode,
    accessorName: String,
    fallbackReturnType: String
  ): RecordPatternAccess = {
    val parentAccess = parentInitNode.getAccess()
    val receiver = recordPatternCastAccessIfNecessary(
      node,
      parentAccess,
      parentInitNode.patternTypeNode,
      parentInitNode.patternTypeFullName
    )
    val methodInfo = methodSignatureInfosByType.get(parentInitNode.patternTypeFullName -> accessorName)
    val returnType = methodInfo.map(_.returnType).getOrElse(fallbackReturnType)
    val signature = methodInfo.map(_.signature).getOrElse {
      if (parentInitNode.patternTypeFullName == Defines.Any) s"${Defines.UnresolvedSignature}(0)"
      else composeSignature(returnType, Nil)
    }
    val fallbackOwner =
      if (parentInitNode.patternTypeFullName == Defines.Any)
        s"${Defines.UnresolvedNamespace}.${parentInitNode.patternTypeNode.code}"
      else parentInitNode.patternTypeFullName
    val fullName     = methodInfo.map(_.fullName).getOrElse(s"$fallbackOwner.$accessorName:$signature")
    val accessorCode = s"${recordPatternReceiverCode(receiver)}.$accessorName()"
    val accessorCall =
      callNode(
        node,
        accessorCode,
        accessorName,
        fullName,
        DispatchTypes.DYNAMIC_DISPATCH,
        Some(signature),
        Some(returnType)
      )
    RecordPatternAccess(callAst(accessorCall, Nil, Some(receiver.ast)), accessorCode, returnType)
  }

  private def recordPatternAssignmentEntry(initNode: RecordPatternInitNode): Option[(Ast, String)] = {
    val children = namedChildren(initNode.patternNode)
    val nameNode = children.reverse.find(_.kind == "identifier")
    val typeNode = nameNode.flatMap(name => children.find(child => child != name && child.kind != "record_pattern"))
    for {
      name <- nameNode
      typ  <- typeNode
    } yield {
      val typeFullName = registerType(patternTypeName(typ))
      val patternLocal = declarePatternLocal(
        name.code,
        typeFullName,
        localName => localNode(name, localName, s"${typ.code} $localName", typeFullName)
      )
      val initializer =
        recordPatternCastAccessIfNecessary(initNode.patternNode, initNode.getAccess(), typ, typeFullName)
      val patternIdentifier    = identifierNode(name, patternLocal.name, name.code, typeFullName)
      val patternIdentifierAst = Ast(patternIdentifier).withRefEdge(patternIdentifier, patternLocal)
      val assignmentCode       = s"${name.code} = ${initializer.code}"
      val assignmentCall =
        callNode(
          initNode.patternNode,
          assignmentCode,
          Operators.assignment,
          Operators.assignment,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Some(typeFullName)
        )
      callAst(assignmentCall, Seq(patternIdentifierAst, initializer.ast)) -> assignmentCode
    }
  }

  private def recordPatternCastAccessIfNecessary(
    node: JavaAstNode,
    initializer: RecordPatternAccess,
    typeNode: JavaAstNode,
    patternType: String
  ): RecordPatternAccess = {
    if (isResolvedTypeFullName(patternType) && initializer.typeFullName == patternType) {
      initializer
    } else {
      val castCode = s"(${typeNode.code}) ${initializer.code}"
      val castCall =
        callNode(node, castCode, Operators.cast, Operators.cast, DispatchTypes.STATIC_DISPATCH, None, Some(patternType))
      RecordPatternAccess(
        callAst(castCall, Seq(Ast(typeRefNode(typeNode, typeNode.code, patternType)), initializer.ast)),
        castCode,
        patternType
      )
    }
  }

  private def recordPatternInstanceOfLhsCode(access: RecordPatternAccess): String = {
    access.ast.root match {
      case Some(_: NewIdentifier)                                    => access.code
      case Some(call: NewCall) if call.name == Operators.fieldAccess => access.code
      case Some(_)                                                   => s"(${access.code})"
      case None                                                      => access.code
    }
  }

  private def recordPatternReceiverCode(access: RecordPatternAccess): String = {
    access.ast.root match {
      case Some(call: NewCall) if call.name.startsWith("<operator") => s"(${access.code})"
      case _                                                        => access.code
    }
  }

  private def recordPatternRootRequiresTemporary(lhsNode: JavaAstNode): Boolean =
    lhsNode.kind != "identifier" && lhsNode.kind != "field_access"

  private def isResolvedTypeFullName(typeFullName: String): Boolean =
    typeFullName != Defines.Any && !typeFullName.startsWith(Defines.UnresolvedNamespace)

  private def recordPatternIsTypePattern(patternNode: JavaAstNode): Boolean =
    patternNode.kind == "record_pattern_component" && !recordPatternIsMatchAll(patternNode) &&
      namedChildren(patternNode).exists(_.kind == "identifier")

  private def recordPatternIsMatchAll(patternNode: JavaAstNode): Boolean =
    namedChildren(patternNode).exists(_.kind == "underscore_pattern")

  private def recordPatternTypeNode(patternNode: JavaAstNode): Option[JavaAstNode] =
    namedChildren(patternNode).find(_.kind != "record_pattern_body")

  private def recordPatternComponents(patternNode: JavaAstNode): Seq[JavaAstNode] =
    namedChildren(patternNode)
      .find(_.kind == "record_pattern_body")
      .toSeq
      .flatMap(_.children.filter(child => child.kind == "record_pattern_component" || child.kind == "record_pattern"))

  private def astForAssignmentExpression(node: JavaAstNode): Ast = {
    val lhs = childByField(node, "left").map(astForExpression).getOrElse(Ast())
    val rhs = childByField(node, "right")
      .map(right => withExpectedExpressionType(Some(expressionType(lhs)))(astForExpression(right)))
      .getOrElse(Ast())
    val operator = childByField(node, "operator").map(_.code).getOrElse("=")
    val op       = assignmentOperatorName(operator)
    val typ      = firstKnownType(lhs, rhs)
    val call     = callNode(node, node.code, op, op, DispatchTypes.STATIC_DISPATCH, None, Some(registerType(typ)))
    callAst(call, Seq(lhs, rhs))
  }

  private def astForUpdateExpression(node: JavaAstNode): Ast = {
    val operandAst     = namedChildren(node).headOption.map(astForExpression).getOrElse(Ast())
    val operatorNodeIx = node.children.indexWhere(child => child.code == "++" || child.code == "--")
    val operandNodeIx  = node.children.indexWhere(_.named)
    val operator       = node.children.lift(operatorNodeIx).map(_.code).getOrElse("")
    val prefix         = operatorNodeIx >= 0 && operandNodeIx >= 0 && operatorNodeIx < operandNodeIx
    val op = operator match {
      case "++" if prefix => Operators.preIncrement
      case "++"           => Operators.postIncrement
      case "--" if prefix => Operators.preDecrement
      case "--"           => Operators.postDecrement
      case _              => Defines.Unknown
    }
    val typ  = expressionType(operandAst)
    val call = callNode(node, node.code, op, op, DispatchTypes.STATIC_DISPATCH, None, Some(registerType(typ)))
    callAst(call, Seq(operandAst))
  }

  private def astForFieldAccess(node: JavaAstNode): Ast = {
    if (isQualifiedThisAccess(node)) {
      return astForQualifiedThisAccess(node)
    }
    val base      = childByField(node, "object").map(astForExpression).getOrElse(Ast())
    val fieldNode = childByField(node, "field").getOrElse(node)
    val fieldName = fieldNode.code
    if (fieldName == "length" && expressionType(base).endsWith("[]")) {
      val typ = registerType("int")
      val call =
        callNode(node, node.code, Operators.sizeOf, Operators.sizeOf, DispatchTypes.STATIC_DISPATCH, None, Some(typ))
      callAst(call, Seq(base))
    } else {
      val baseType = expressionType(base)
      val typ =
        memberTypeForType(baseType, fieldName).orElse(memberTypeNames.get(fieldName)).getOrElse(Defines.Any)
      registerType(typ)
      fieldAccessAst(node, fieldNode, base, node.code, fieldName, typ)
    }
  }

  private def isQualifiedThisAccess(node: JavaAstNode): Boolean =
    node.kind == "field_access" && childByField(node, "field").exists(_.kind == "this")

  private def astForQualifiedThisAccess(node: JavaAstNode): Ast = {
    val targetType = childByField(node, "object")
      .map(typeQualifier => registerType(normalizeTypeName(typeQualifier.code)))
      .getOrElse(lookupType("this"))
    qualifiedThisReceiverAst(node, targetType, node.code)
  }

  private def qualifiedThisReceiverAst(node: JavaAstNode, targetType: String, code: String): Ast = {
    val currentThisType = lookupType("this")
    val thisIdentifier  = identifierNode(node, "this", code, currentThisType)
    val thisAst = parameterRef("this")
      .map(target => Ast(thisIdentifier).withRefEdge(thisIdentifier, target))
      .getOrElse(Ast(thisIdentifier))

    if (currentThisType == targetType || currentThisType == Defines.Any) {
      thisIdentifier.typeFullName(targetType)
      thisAst
    } else {
      outerClassAccessChain(node, thisAst, currentThisType, targetType, code).getOrElse(thisAst)
    }
  }

  private def outerClassAccessChain(
    node: JavaAstNode,
    baseAst: Ast,
    currentType: String,
    targetType: String,
    code: String
  ): Option[Ast] = {
    localRecordCaptureInfosByType.get(currentType).flatMap(_.find(_.name == "outerClass")).flatMap { capture =>
      val fieldAccess = fieldAccessAst(node, node, baseAst, code, "outerClass", registerType(capture.typeFullName))
      if (capture.typeFullName == targetType) {
        Some(fieldAccess)
      } else {
        outerClassAccessChain(node, fieldAccess, capture.typeFullName, targetType, code)
      }
    }
  }

  private def astForArrayAccess(node: JavaAstNode): Ast = {
    val arrayAst = childByField(node, "array").map(astForExpression).getOrElse(Ast())
    val indexAst = childByField(node, "index").map(astForExpression).getOrElse(Ast())
    val typ      = elementType(expressionType(arrayAst))
    val call = callNode(
      node,
      node.code,
      Operators.indexAccess,
      Operators.indexAccess,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(registerType(typ))
    )
    callAst(call, Seq(arrayAst, indexAst))
  }

  private def astForMethodInvocation(node: JavaAstNode): Ast = {
    val name          = childByField(node, "name").map(_.code).getOrElse(node.code.takeWhile(_ != '('))
    val objectNode    = childByField(node, "object")
    val baseAst       = objectNode.map(astForExpression)
    val receiverType  = baseAst.map(expressionType).filter(_ != Defines.Any)
    val argumentNodes = childByField(node, "arguments").map(argumentExpressionNodes).getOrElse(Nil)
    val argumentTypes = argumentNodes.map(expressionTypeNameForSignature)
    val methodInfo    = methodInfoForInvocation(name, objectNode, receiverType, argumentTypes)
    val args = argumentNodes.zipWithIndex.map { case (argument, index) =>
      withExpectedExpressionType(methodInfo.flatMap(_.parameterTypes.lift(index)))(astForExpression(argument))
    }
    val implicitReceiverAst =
      Option
        .when(baseAst.isEmpty)(methodInfo.filterNot(_.isStatic).flatMap(implicitReceiverAstForMethod(node, _)))
        .flatten
    val dispatchType =
      if (methodInfo.exists(_.isStatic)) DispatchTypes.STATIC_DISPATCH
      else if (baseAst.isDefined || implicitReceiverAst.isDefined) DispatchTypes.DYNAMIC_DISPATCH
      else DispatchTypes.STATIC_DISPATCH
    val call = callNode(
      node,
      node.code,
      name,
      methodInfo.map(_.fullName).getOrElse(name),
      dispatchType,
      methodInfo.map(_.signature),
      Some(methodInfo.map(_.returnType).getOrElse(Defines.Any))
    )
    callAst(call, args, baseAst.orElse(implicitReceiverAst))
  }

  private def astForMethodReference(node: JavaAstNode): Ast = {
    val parts                  = namedChildren(node)
    val isConstructorReference = node.children.exists(_.kind == "new")
    val methodName =
      if (isConstructorReference) "new"
      else parts.lastOption.map(_.code).getOrElse(node.code.split("::").lastOption.getOrElse(node.code))
    val targetNode =
      if (isConstructorReference) parts.lastOption
      else parts.dropRight(1).lastOption
    val namespace = targetNode.map(methodReferenceTargetType).filter(_ != Defines.Any)
    val constructorInfo =
      Option
        .when(isConstructorReference)(
          namespace.flatMap(typeFullName =>
            methodSignatureInfosByType.get(typeFullName -> Defines.ConstructorMethodName)
          )
        )
        .flatten
    val referencedMethodInfo =
      if (isConstructorReference) constructorInfo
      else methodInfoForInvocation(methodName, targetNode, namespace)
    val methodFullName = referencedMethodInfo
      .map(_.fullName)
      .getOrElse(s"${namespace.getOrElse(Defines.UnresolvedNamespace)}.$methodName:${Defines.UnresolvedSignature}")
    Ast(methodRefNode(node, node.code, methodFullName, namespace.getOrElse(Defines.Any)))
  }

  private def astForLambdaExpression(node: JavaAstNode): Ast = {
    val lambdaName    = nextLambdaName()
    val ownerFullName = currentTypeFullNames.headOption.getOrElse(Defines.UnresolvedNamespace)
    val signatureInfo = lambdaSignatureInfo(currentExpectedExpressionType)
    val lambdaParams  = lambdaParameterInfos(node, signatureInfo.parameterTypes)
    val paramTypes    = lambdaParams.map(_.typeFullName)
    val returnType    = registerType(signatureInfo.returnType)
    val signature     = composeSignature(returnType, paramTypes)
    val fullName      = s"$ownerFullName.$lambdaName:$signature"
    val bodyNode      = childByField(node, "body").orElse(namedChildren(node).lastOption).getOrElse(node)
    val paramBindings = lambdaParams.map(param => param.name -> param.typeFullName)
    val lambdaCaptures = lambdaCaptureInfos(
      bodyNode,
      paramBindings.map(_._1).toSet,
      forceThisCapture = containsThisReference(node) || containsEnclosingInstanceTypeCreation(bodyNode)
    )
    val capturesThis = lambdaCaptures.exists(_.name == "this")

    val lambdaMethod = methodNode(
      node,
      lambdaName,
      "<lambda>",
      fullName,
      Some(signature),
      document.relativeName,
      Some(NodeTypes.TYPE_DECL),
      Some(ownerFullName)
    )

    val thisParam = Option.when(capturesThis)(Ast(thisParameter(node, ownerFullName))).toSeq
    val paramAsts = thisParam ++ lambdaParams.zipWithIndex.map { case (param, index) =>
      Ast(lambdaParameterNode(param, index + 1))
    }
    val parameterRefBindings = parameterBindings(paramAsts)
    val captureLocals = lambdaCaptures.map { capture =>
      val closureBindingId = lambdaClosureBindingId(lambdaName, capture.name)
      val local = localNode(bodyNode, capture.name, capture.name, capture.typeFullName, Some(closureBindingId))
      LambdaCaptureLocal(capture, local, closureBindingNode(closureBindingId, EvaluationStrategies.BY_SHARING))
    }
    val capturedLocalAsts = captureLocals.map(capture => Ast(capture.local))

    val bodyAst = withMethodFullName(fullName) {
      withReturnType(returnType) {
        withScope(paramBindings ++ captureLocals.map(capture => capture.local.name -> capture.local.typeFullName)) {
          withParameterRefs(parameterRefBindings) {
            captureLocals.foreach(capture =>
              declareLocal(capture.local.name, capture.local.typeFullName, Some(capture.local))
            )
            if (bodyNode.kind == "block") {
              astForBlock(bodyNode, prefixAsts = capturedLocalAsts)
            } else {
              val bodyStatements =
                if (returnType == "void") {
                  capturedLocalAsts :+ withExpectedExpressionType(Some(returnType))(astForExpression(bodyNode))
                } else {
                  val exprAst = withExpectedExpressionType(Some(returnType))(astForExpression(bodyNode))
                  capturedLocalAsts :+ returnAst(returnNode(bodyNode, s"return ${bodyNode.code};"), Seq(exprAst))
                }
              blockAst(blockNode(bodyNode, bodyNode.code, Defines.Any), bodyStatements.toList)
            }
          }
        }
      }
    }

    val modifiers =
      List(ModifierTypes.PRIVATE, ModifierTypes.LAMBDA) ++ Option.unless(capturesThis)(ModifierTypes.STATIC)
    val lambdaMethodAst = methodAst(
      lambdaMethod,
      paramAsts,
      bodyAst,
      methodReturnNode(node, returnType),
      modifiers.map(modifierNode(node, _))
    )

    val interfaceType = registerType(signatureInfo.interfaceType)
    val lambdaTypeDecl = typeDeclNode(
      node,
      lambdaName,
      fullName,
      document.relativeName,
      lambdaName,
      NodeTypes.TYPE_DECL,
      ownerFullName,
      Seq(interfaceType)
    )
    val lambdaBindingAsts = lambdaBindingAstsFor(node, signatureInfo, fullName, signature)
    val lambdaTypeDeclAst = typeDeclAstWithBindings(Ast(lambdaTypeDecl), lambdaTypeDecl, lambdaBindingAsts)
    pendingLambdaAsts = lambdaMethodAst :: lambdaTypeDeclAst :: pendingLambdaAsts

    val methodRef = methodRefNode(node, node.code, fullName, fullName)
    captureLocals.foldLeft(Ast(methodRef)) { case (methodRefAst, capture) =>
      methodRefAst
        .merge(Ast(capture.closureBinding).withRefEdge(capture.closureBinding, capture.info.refTarget))
        .withCaptureEdge(methodRef, capture.closureBinding)
    }
  }

  private def astForObjectCreationExpression(node: JavaAstNode): Ast = {
    val typ      = constructedTypeForObjectCreation(node)
    val tmpName  = nextObjectCreationTempName()
    val tmpLocal = localNode(node, tmpName, tmpName, typ)

    val assignTarget    = identifierNode(node, tmpName, tmpName, typ)
    val assignTargetAst = Ast(assignTarget).withRefEdge(assignTarget, tmpLocal)
    val allocAst        = objectCreationAllocAst(node, typ)
    val assignCall = callNode(
      node,
      s"$tmpName = ${node.code}",
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(typ)
    )
    val assignAst = callAst(assignCall, Seq(assignTargetAst, allocAst))

    val initReceiver    = identifierNode(node, tmpName, tmpName, typ)
    val initReceiverAst = Ast(initReceiver).withRefEdge(initReceiver, tmpLocal)
    val initAst         = objectCreationInitAst(node, typ, initReceiverAst)

    val returnedIdentifier    = identifierNode(node, tmpName, tmpName, typ)
    val returnedIdentifierAst = Ast(returnedIdentifier).withRefEdge(returnedIdentifier, tmpLocal)
    blockAst(blockNode(node, node.code, typ), List(Ast(tmpLocal), assignAst, initAst, returnedIdentifierAst))
  }

  private def astsForLocalObjectCreationInitializer(
    declarator: JavaAstNode,
    initializer: JavaAstNode,
    local: NewLocal,
    name: String,
    declaredType: String,
    codeType: String = ""
  ): Seq[Ast] = {
    val constructedType = constructedTypeForObjectCreation(initializer)
    val displayType     = Option(codeType).filter(_.nonEmpty).getOrElse(declaredType)
    val target          = identifierNode(declarator, local.name, name, declaredType)
    val targetAst       = Ast(target).withRefEdge(target, local)
    val allocAst        = objectCreationAllocAst(initializer, constructedType)
    val assignCall = callNode(
      declarator,
      s"$displayType $name = ${initializer.code}",
      Operators.assignment,
      Operators.assignment,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(declaredType)
    )
    val assignAst = callAst(assignCall, Seq(targetAst, allocAst))

    val initReceiver    = identifierNode(initializer, local.name, name, declaredType)
    val initReceiverAst = Ast(initReceiver).withRefEdge(initReceiver, local)
    val initAst         = objectCreationInitAst(initializer, constructedType, initReceiverAst)
    Seq(assignAst, initAst)
  }

  private def constructedTypeForObjectCreation(node: JavaAstNode): String = {
    anonymousClassType(node).getOrElse(objectCreationType(node))
  }

  private def anonymousClassType(node: JavaAstNode): Option[String] = {
    node.children.find(_.kind == "class_body").map { body =>
      val baseType                       = objectCreationType(node)
      val ownerBase                      = anonymousOwnerBase()
      val simpleName                     = simpleTypeName(baseType)
      val anonName                       = nextAnonymousTypeName(ownerBase, simpleName)
      val fullName                       = registerType(s"$ownerBase.$anonName")
      val captures                       = anonymousCaptureInfos(node)
      val syntheticConstructorParameters = objectCreationArgumentInfos(node)
      if (captures.nonEmpty) {
        localRecordCaptureInfosByType.update(fullName, captures)
        if (captures.exists(_.name == "outerClass")) {
          anonymousOuterClassTypes += fullName
        }
      }
      val bodyChildren = body.children.filter(_.named)
      val typeAst = withoutParameterRefs {
        withoutEnclosingLocalScopes {
          astForTypeDeclaration(
            node,
            packageName = None,
            fullNameOverride = Some(fullName),
            codeOverride = Some(node.code),
            astParentTypeOverride = Some(NodeTypes.TYPE_DECL),
            astParentFullNameOverride = currentTypeFullNames.headOption,
            localCaptureInfos = captures,
            nameOverride = Some(anonName),
            inheritsOverride = Some(Seq(baseType)),
            bodyChildrenOverride = Some(bodyChildren),
            forceDefaultConstructor = true,
            defaultConstructorParameters = syntheticConstructorParameters
          )
        }
      }
      pendingAnonymousTypeAsts = typeAst :: pendingAnonymousTypeAsts
      fullName
    }
  }

  private def objectCreationType(node: JavaAstNode): String = {
    val typ = childByField(node, "type").map(typeName).getOrElse(typeNameFromAllocationCode(node.code))
    registerType(typ)
  }

  private def arrayCreationType(node: JavaAstNode): String = {
    val baseType = childByField(node, "type").map(typeName).getOrElse(typeNameFromArrayCreationCode(node.code))
    val typ = if (baseType.endsWith("[]")) {
      baseType
    } else {
      s"$baseType${List.fill(math.max(arrayDimensionCount(node), 1))("[]").mkString}"
    }
    registerType(typ)
  }

  private def objectCreationAllocAst(node: JavaAstNode, typeFullName: String): Ast = {
    val call = callNode(
      node,
      node.code,
      Operators.alloc,
      Operators.alloc,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(typeFullName)
    )
    callAst(call)
  }

  private def objectCreationArgumentInfos(node: JavaAstNode): Seq[SyntheticConstructorParameterInfo] = {
    childByField(node, "arguments")
      .map(namedChildren)
      .getOrElse(Nil)
      .zipWithIndex
      .map { case (argument, index) =>
        SyntheticConstructorParameterInfo(
          argument,
          s"arg$index",
          registerType(expressionTypeNameForSignature(argument))
        )
      }
  }

  private def expressionTypeNameForSignature(node: JavaAstNode): String = {
    node.kind match {
      case "identifier"               => lookupTypeOrTypeName(node.code)
      case "this"                     => lookupType("this")
      case "super"                    => currentSuperTypeFullName
      case kind if LiteralKinds(kind) => literalTypeName(node)
      case "parenthesized_expression" =>
        namedChildren(node).headOption.map(expressionTypeNameForSignature).getOrElse(Defines.Any)
      case "cast_expression" =>
        childByField(node, "type").map(typeName).getOrElse(Defines.Any)
      case "array_creation_expression" =>
        arrayCreationType(node)
      case "object_creation_expression" =>
        objectCreationType(node)
      case "array_access" =>
        childByField(node, "array").map(expressionTypeNameForSignature).map(elementType).getOrElse(Defines.Any)
      case "field_access" =>
        val fieldName =
          childByField(node, "field").map(_.code).orElse(namedChildren(node).lastOption.map(_.code)).getOrElse("")
        val receiverType = childByField(node, "object").map(expressionTypeNameForSignature).filter(_ != Defines.Any)
        receiverType
          .flatMap(memberTypeForType(_, fieldName))
          .orElse(memberTypeNames.get(fieldName))
          .getOrElse(Defines.Any)
      case "method_invocation" =>
        val name         = childByField(node, "name").map(_.code).getOrElse(node.code.takeWhile(_ != '('))
        val objectNode   = childByField(node, "object")
        val receiverType = objectNode.map(expressionTypeNameForSignature).filter(_ != Defines.Any)
        val argumentTypes = childByField(node, "arguments").map(argumentExpressionNodes).getOrElse(Nil).map {
          expressionTypeNameForSignature
        }
        methodInfoForInvocation(name, objectNode, receiverType, argumentTypes).map(_.returnType).getOrElse(Defines.Any)
      case "binary_expression" =>
        val operator = childByField(node, "operator").map(_.code).getOrElse("")
        if (BooleanBinaryOperators.contains(operator)) "boolean"
        else {
          firstKnownTypeName(
            childByField(node, "left").map(expressionTypeNameForSignature),
            childByField(node, "right").map(expressionTypeNameForSignature)
          )
        }
      case "unary_expression" =>
        val operator =
          childByField(node, "operator").map(_.code).orElse(namedChildren(node).headOption.map(_.code)).getOrElse("")
        if (operator == "!") "boolean"
        else
          childByField(node, "operand")
            .orElse(namedChildren(node).lastOption)
            .map(expressionTypeNameForSignature)
            .getOrElse(Defines.Any)
      case "ternary_expression" =>
        firstKnownTypeName(
          childByField(node, "consequence").map(expressionTypeNameForSignature),
          childByField(node, "alternative").map(expressionTypeNameForSignature)
        )
      case "assignment_expression" =>
        firstKnownTypeName(
          childByField(node, "left").map(expressionTypeNameForSignature),
          childByField(node, "right").map(expressionTypeNameForSignature)
        )
      case _ => Defines.Any
    }
  }

  private def firstKnownTypeName(types: Option[String]*): String = {
    types.flatten.find(_ != Defines.Any).getOrElse(Defines.Any)
  }

  private def objectCreationInitAst(node: JavaAstNode, typeFullName: String, receiverAst: Ast): Ast = {
    val explicitArgInfos = objectCreationArgumentInfos(node)
    val explicitArgs =
      childByField(node, "arguments").map(argumentAsts(_, explicitArgInfos.map(_.typeFullName))).getOrElse(Nil)
    val captureArgs = localRecordCaptureArgumentAsts(typeFullName, node)
    val signature   = composeSignature("void", explicitArgInfos.map(_.typeFullName))
    val methodFullName =
      if (typeFullName == Defines.Any) Defines.ConstructorMethodName
      else s"$typeFullName.${Defines.ConstructorMethodName}:$signature"
    val initCall = callNode(
      node,
      node.code,
      Defines.ConstructorMethodName,
      methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(signature),
      Some("void")
    )
    callAst(initCall, explicitArgs ++ captureArgs, Some(receiverAst))
  }

  private def receiverAstForAssignmentInit(node: JavaAstNode): Ast = {
    node.kind match {
      case "identifier" =>
        val name       = node.code
        val identifier = identifierNode(node, name, name, lookupType(name))
        localRef(name)
          .map(local => Ast(identifier).withRefEdge(identifier, local))
          .orElse(parameterRef(name).map(target => Ast(identifier).withRefEdge(identifier, target)))
          .getOrElse(Ast(identifier))
      case _ =>
        astForExpression(node)
    }
  }

  private def astForArrayCreationExpression(node: JavaAstNode): Ast = {
    val typ = arrayCreationType(node)
    registerType(typ)
    childByField(node, "value") match {
      case Some(initializer) if initializer.kind == "array_initializer" =>
        astForArrayInitializer(initializer, Some(typ), Some(node.code))
      case _ =>
        val args = childByField(node, "arguments").map(argumentAsts(_)).getOrElse(Nil) ++
          node.children.filter(_.fieldName.contains("dimensions")).flatMap(namedChildren).map(astForExpression)
        val call =
          callNode(node, node.code, Operators.alloc, Operators.alloc, DispatchTypes.STATIC_DISPATCH, None, Some(typ))
        callAst(call, args)
    }
  }

  private def astForArrayInitializer(
    node: JavaAstNode,
    typeFullNameOverride: Option[String] = None,
    codeOverride: Option[String] = None
  ): Ast = {
    val typ = registerType(typeFullNameOverride.orElse(currentExpectedExpressionType).getOrElse(Defines.Any))
    val expectedElement = Option(elementType(typ)).filter(_ != Defines.Any)
    val args = namedChildren(node).map { child =>
      withExpectedExpressionType(expectedElement)(astForExpression(child))
    }
    val call = callNode(
      node,
      codeOverride.getOrElse(node.code),
      Operators.arrayInitializer,
      Operators.arrayInitializer,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(typ)
    )
    callAst(call, args)
  }

  private def astForClassLiteral(node: JavaAstNode): Ast = {
    val typeNode = namedChildren(node).headOption.getOrElse(node)
    val typ      = registerType(typeName(typeNode))
    val target   = Ast(identifierNode(typeNode, typeNode.code, typeNode.code, typ))
    val fieldNode = node.children
      .find(_.kind == "class")
      .getOrElse(node)
    fieldAccessAst(node, fieldNode, target, node.code, "class", registerType("java.lang.Class"))
  }

  private def argumentAsts(node: JavaAstNode, expectedTypes: Seq[String] = Nil): Seq[Ast] =
    argumentExpressionNodes(node).zipWithIndex.map { case (argument, index) =>
      withExpectedExpressionType(expectedTypes.lift(index))(astForExpression(argument))
    }

  private def argumentExpressionNodes(argumentsNode: JavaAstNode): List[JavaAstNode] =
    namedChildren(argumentsNode)

  private def thisParameter(node: JavaAstNode, ownerFullName: String): NewMethodParameterIn = {
    registerType(ownerFullName)
    parameterInNode(
      node,
      "this",
      "this",
      index = 0,
      isVariadic = false,
      evaluationStrategy = EvaluationStrategies.BY_SHARING,
      typeFullName = ownerFullName
    )
  }

  private def parameterNode(node: JavaAstNode, index: Int, typ: String): NewMethodParameterIn = {
    registerType(typ)
    val name = childByField(node, "name").map(_.code).getOrElse(s"param$index")
    parameterInNode(
      node,
      name,
      node.code,
      index = index,
      isVariadic = false,
      evaluationStrategy = evaluationStrategyFor(typ),
      typeFullName = typ
    )
  }

  private def parameterAst(node: JavaAstNode, index: Int, typ: String): Ast = {
    Ast(parameterNode(node, index, typ)).withChildren(annotationAstsFor(node))
  }

  private def parameterNodes(
    node: JavaAstNode,
    erasedTypeParameters: Set[String] = Set.empty
  ): List[(JavaAstNode, String, String)] = {
    childByField(node, "parameters").toList
      .flatMap(_.children)
      .filter(_.kind == "formal_parameter")
      .map { parameter =>
        val rawTyp = childByField(parameter, "type").map(typeName).getOrElse(Defines.Any)
        val typ    = eraseTypeParameters(rawTyp, erasedTypeParameters)
        val name   = childByField(parameter, "name").map(_.code).getOrElse("param")
        (parameter, typ, name)
      }
  }

  private def lambdaParameterInfos(node: JavaAstNode, expectedTypes: Seq[String]): List[LambdaParameterInfo] = {
    val paramsNode = childByField(node, "parameters")
    val rawParams = paramsNode.toList.flatMap {
      case param if param.kind == "identifier" =>
        List((param, param.code, Option.empty[String]))
      case params if params.kind == "inferred_parameters" =>
        namedChildren(params).filter(_.kind == "identifier").map(param => (param, param.code, Option.empty[String]))
      case params if params.kind == "formal_parameters" =>
        namedChildren(params).zipWithIndex.collect {
          case (param, index) if param.kind == "formal_parameter" =>
            val name = childByField(param, "name").map(_.code).getOrElse(s"param${index + 1}")
            val typ  = childByField(param, "type").map(typeName)
            (param, name, typ)
          case (param, _) if param.kind == "identifier" =>
            (param, param.code, Option.empty[String])
        }
      case param =>
        List((param, param.code, Option.empty[String]))
    }

    rawParams.zipWithIndex.map { case ((paramNode, name, explicitType), index) =>
      val typ = registerType(explicitType.orElse(expectedTypes.lift(index)).getOrElse("java.lang.Object"))
      val code =
        if (explicitType.isDefined) paramNode.code
        else s"$typ $name"
      LambdaParameterInfo(paramNode, name, typ, code)
    }
  }

  private def lambdaParameterNode(parameter: LambdaParameterInfo, index: Int): NewMethodParameterIn = {
    parameterInNode(
      parameter.node,
      parameter.name,
      parameter.code,
      index = index,
      isVariadic = false,
      evaluationStrategy = evaluationStrategyFor(parameter.typeFullName),
      typeFullName = parameter.typeFullName
    )
  }

  private def lambdaSignatureInfo(expectedType: Option[String]): LambdaSignatureInfo = {
    val expected                = expectedType.map(normalizeTypeName).filter(_ != Defines.Any).getOrElse(Defines.Any)
    val (baseType, genericArgs) = typeApplication(expected)
    val normalizedArgs          = genericArgs.map(normalizeTypeName)
    val interfaceType           = functionalInterfaceTypeName(baseType)

    functionalInterfaceInfos
      .get(baseType)
      .orElse(functionalInterfaceInfos.get(interfaceType))
      .orElse(functionalInterfaceInfos.get(simpleTypeName(baseType)))
      .map(_.instantiate(normalizedArgs))
      .getOrElse {
        val objectType = "java.lang.Object"
        simpleTypeName(interfaceType) match {
          case "Function" =>
            LambdaSignatureInfo(
              interfaceType,
              "apply",
              Seq(normalizedArgs.headOption.getOrElse(objectType)),
              normalizedArgs.lift(1).getOrElse(objectType)
            )
          case "Consumer" =>
            LambdaSignatureInfo(interfaceType, "accept", Seq(normalizedArgs.headOption.getOrElse(objectType)), "void")
          case "Supplier" =>
            LambdaSignatureInfo(interfaceType, "get", Nil, normalizedArgs.headOption.getOrElse(objectType))
          case "Predicate" =>
            LambdaSignatureInfo(interfaceType, "test", Seq(normalizedArgs.headOption.getOrElse(objectType)), "boolean")
          case "BiFunction" =>
            LambdaSignatureInfo(
              interfaceType,
              "apply",
              Seq(normalizedArgs.headOption.getOrElse(objectType), normalizedArgs.lift(1).getOrElse(objectType)),
              normalizedArgs.lift(2).getOrElse(objectType)
            )
          case "BiConsumer" =>
            LambdaSignatureInfo(
              interfaceType,
              "accept",
              Seq(normalizedArgs.headOption.getOrElse(objectType), normalizedArgs.lift(1).getOrElse(objectType)),
              "void"
            )
          case "UnaryOperator" =>
            val typ = normalizedArgs.headOption.getOrElse(objectType)
            LambdaSignatureInfo(interfaceType, "apply", Seq(typ), typ)
          case "BinaryOperator" =>
            val typ = normalizedArgs.headOption.getOrElse(objectType)
            LambdaSignatureInfo(interfaceType, "apply", Seq(typ, typ), typ)
          case _ =>
            LambdaSignatureInfo(
              if (interfaceType == Defines.Any) objectType else interfaceType,
              "apply",
              Nil,
              objectType
            )
        }
      }
  }

  private def lambdaBindingAstsFor(
    node: JavaAstNode,
    signatureInfo: LambdaSignatureInfo,
    lambdaFullName: String,
    lambdaSignature: String
  ): Seq[Ast] = {
    val concreteBinding = Ast(
      NewBinding()
        .name(signatureInfo.methodName)
        .signature(lambdaSignature)
        .methodFullName(lambdaFullName)
    )
    val erasedSignature = composeSignature(
      eraseLambdaBindingType(signatureInfo.returnType),
      signatureInfo.parameterTypes.map(eraseLambdaBindingType)
    )
    if (erasedSignature == lambdaSignature) {
      Seq(concreteBinding)
    } else {
      Seq(
        Ast(
          NewBinding()
            .name(signatureInfo.methodName)
            .signature(erasedSignature)
            .methodFullName(lambdaFullName)
        ),
        concreteBinding
      )
    }
  }

  private def eraseLambdaBindingType(typeFullName: String): String = {
    if (typeFullName == "void" || PrimitiveTypes.contains(typeFullName)) typeFullName else "java.lang.Object"
  }

  private def lambdaCaptureInfos(
    bodyNode: JavaAstNode,
    parameterNames: Set[String],
    forceThisCapture: Boolean
  ): Seq[LambdaCaptureInfo] = {
    val excludedNames = parameterNames ++ declaredNames(bodyNode) + "super"

    def appendUnique(captures: Vector[LambdaCaptureInfo], capture: LambdaCaptureInfo): Vector[LambdaCaptureInfo] =
      if (captures.exists(_.name == capture.name)) captures else captures :+ capture

    val initialCaptures =
      if (forceThisCapture) thisCaptureInfo.toVector
      else Vector.empty[LambdaCaptureInfo]

    identifierUses(bodyNode)
      .map(_.code)
      .distinct
      .filterNot(excludedNames)
      .foldLeft(initialCaptures) { (captures, name) =>
        directLambdaCaptureInfo(name)
          .map(capture => appendUnique(captures, capture))
          .orElse {
            Option.when(memberTypeVisibleFromThis(name).isDefined) {
              thisCaptureInfo.map(capture => appendUnique(captures, capture)).getOrElse(captures)
            }
          }
          .getOrElse(captures)
      }
  }

  private def directLambdaCaptureInfo(name: String): Option[LambdaCaptureInfo] = {
    val refTarget = localRef(name).map(identity[NewNode]).orElse(parameterRef(name))
    refTarget.flatMap { target =>
      val typ = typeFullNameForRef(target, lookupType(name))
      Option.when(typ != Defines.Any)(LambdaCaptureInfo(name, typ, target))
    }
  }

  private def thisCaptureInfo: Option[LambdaCaptureInfo] = {
    val refTarget = localRef("this").map(identity[NewNode]).orElse(parameterRef("this"))
    refTarget.flatMap { target =>
      val typ = typeFullNameForRef(target, lookupType("this"))
      Option.when(typ != Defines.Any)(LambdaCaptureInfo("this", typ, target))
    }
  }

  private def memberTypeVisibleFromThis(fieldName: String): Option[String] = {
    val thisType = lookupType("this")
    Option
      .when(thisType != Defines.Any) {
        memberTypeForType(thisType, fieldName).orElse(memberTypeNames.get(fieldName))
      }
      .flatten
  }

  private def typeFullNameForRef(ref: NewNode, fallback: String): String = {
    ref match {
      case local: NewLocal                 => local.typeFullName
      case parameter: NewMethodParameterIn => parameter.typeFullName
      case _                               => fallback
    }
  }

  private def lambdaClosureBindingId(lambdaName: String, captureName: String): String =
    s"${document.relativeName}:$lambdaName:$captureName"

  private def identifierUses(node: JavaAstNode): List[JavaAstNode] = {
    val self =
      if (node.kind == "identifier" && isIdentifierUse(node)) List(node)
      else Nil
    self ++ node.children.flatMap(identifierUses)
  }

  private def isIdentifierUse(node: JavaAstNode): Boolean = {
    !node.fieldName.exists {
      case "field" | "label" | "name" | "type" => true
      case _                                   => false
    }
  }

  private def containsThisReference(node: JavaAstNode): Boolean = {
    node.kind == "this" || node.kind == "super" || node.children.exists(containsThisReference)
  }

  private def containsEnclosingInstanceTypeCreation(node: JavaAstNode): Boolean = {
    val currentNodeNeedsThis =
      (node.kind == "object_creation_expression" && node.children.exists(_.kind == "class_body")) ||
        node.kind == "class_declaration"
    currentNodeNeedsThis || node.children.exists(containsEnclosingInstanceTypeCreation)
  }

  private def annotationAstsFor(node: JavaAstNode): Seq[Ast] = {
    declarationAnnotationNodes(node).map(astForAnnotation)
  }

  private def declarationAnnotationNodes(node: JavaAstNode): Seq[JavaAstNode] = {
    node.children.flatMap {
      case child if AnnotationKinds.contains(child.kind) => Seq(child)
      case modifiers if modifiers.kind == "modifiers" =>
        modifiers.children.filter(child => AnnotationKinds.contains(child.kind))
      case _ => Nil
    }
  }

  private def astForAnnotation(node: JavaAstNode): Ast = {
    val name =
      childByField(node, "name").map(_.code).orElse(namedChildren(node).headOption.map(_.code)).getOrElse(node.code)
    val fullName   = registerType(annotationTypeFullName(name))
    val annotation = annotationNode(node, annotationCode(node, name), name, fullName)
    val argumentAsts = childByField(node, "arguments")
      .map(annotationArgumentAsts)
      .getOrElse(Nil)
    annotationAst(annotation, argumentAsts)
  }

  private def annotationArgumentAsts(argumentsNode: JavaAstNode): Seq[Ast] = {
    val arguments = namedChildren(argumentsNode)
    if (arguments.forall(_.kind == "element_value_pair")) {
      arguments.map { pair =>
        val key       = childByField(pair, "key").map(_.code).getOrElse("value")
        val valueNode = childByField(pair, "value").orElse(namedChildren(pair).lastOption)
        val valueAst  = valueNode.map(astForAnnotationValue).getOrElse(Ast())
        annotationAssignmentAst(key, annotationAssignmentCode(key, valueNode, pair.code), valueAst)
      }
    } else {
      arguments.map { value =>
        annotationAssignmentAst("value", annotationValueCode(value), astForAnnotationValue(value))
      }
    }
  }

  private def astForAnnotationValue(node: JavaAstNode): Ast = {
    node.kind match {
      case kind if AnnotationKinds.contains(kind) =>
        astForAnnotation(node)
      case "element_value_array_initializer" =>
        val elementAsts = namedChildren(node).map(astForAnnotationValue)
        setArgumentIndices(elementAsts)
        Ast(NewArrayInitializer().code(annotationArrayCode(node))).withChildren(elementAsts)
      case kind if LiteralKinds(kind) =>
        Ast(annotationLiteralNode(node, annotationLiteralName(node)))
      case _ =>
        Ast(annotationLiteralNode(node, annotationValueCode(node)))
    }
  }

  private def annotationCode(node: JavaAstNode, name: String): String = {
    childByField(node, "arguments") match {
      case Some(argumentsNode) =>
        val argumentCodes = annotationArgumentAsts(argumentsNode).flatMap(_.root).collect {
          case assignment: NewAnnotationParameterAssign => assignment.code
        }
        if (argumentCodes.nonEmpty) s"@$name(${argumentCodes.mkString(", ")})" else node.code
      case None =>
        node.code
    }
  }

  private def annotationAssignmentCode(key: String, valueNode: Option[JavaAstNode], fallback: String): String = {
    valueNode match {
      case Some(value) => s"$key = ${annotationValueCode(value)}"
      case None        => fallback
    }
  }

  private def annotationValueCode(node: JavaAstNode): String = {
    node.kind match {
      case "element_value_array_initializer" => annotationArrayCode(node)
      case _                                 => node.code
    }
  }

  private def annotationArrayCode(node: JavaAstNode): String = {
    val values = namedChildren(node).map(annotationValueCode)
    s"{ ${values.mkString(", ")} }"
  }

  private def annotationLiteralName(node: JavaAstNode): String = {
    node.kind match {
      case "string_literal" =>
        val fragments = namedChildren(node).filter(_.kind == "string_fragment").map(_.code)
        if (fragments.nonEmpty) fragments.mkString else stripLiteralQuotes(node.code)
      case "character_literal"                  => stripLiteralQuotes(node.code)
      case "true" | "false" | "boolean_literal" => node.code
      case "null_literal" | "null"              => "null"
      case _                                    => stripLiteralQuotes(node.code)
    }
  }

  private def stripLiteralQuotes(value: String): String = {
    val trimmed = value.trim
    if (
      trimmed.length >= 2 && ((trimmed.startsWith("\"") && trimmed
        .endsWith("\"")) || (trimmed.startsWith("'") && trimmed.endsWith("'")))
    ) {
      trimmed.substring(1, trimmed.length - 1)
    } else {
      trimmed
    }
  }

  private def annotationTypeFullName(name: String): String = {
    if (name.contains(".")) normalizeTypeName(name)
    else
      importAliases
        .get(name)
        .orElse(resolveSingleWildcardImport(name))
        .orElse(resolveModuleImportedType(name))
        .orElse(currentPackageName.map(pkg => s"$pkg.$name"))
        .getOrElse(name)
  }

  private def composeSignature(returnType: String, parameterTypes: Seq[String]): String =
    s"$returnType(${parameterTypes.mkString(",")})"

  private lazy val JavaLibraryMethodSignatures: Map[(String, String, Seq[String]), MethodSignatureInfo] = {
    def info(
      ownerTypeFullName: String,
      name: String,
      returnType: String,
      parameterTypes: Seq[String] = Nil,
      isStatic: Boolean = false
    ): ((String, String, Seq[String]), MethodSignatureInfo) = {
      val signature = composeSignature(returnType, parameterTypes)
      (ownerTypeFullName, name, parameterTypes) -> MethodSignatureInfo(
        parameterTypes,
        returnType,
        s"$ownerTypeFullName.$name:$signature",
        signature,
        isStatic
      )
    }

    Seq(
      info("java.lang.String", "length", "int"),
      info("java.lang.String", "trim", "java.lang.String"),
      info("java.lang.String", "strip", "java.lang.String"),
      info("java.lang.String", "isEmpty", "boolean"),
      info("java.lang.String", "isBlank", "boolean"),
      info("java.lang.String", "valueOf", "java.lang.String", Seq("boolean"), isStatic = true),
      info("java.util.Base64", "getDecoder", "java.util.Base64$Decoder", isStatic = true),
      info("java.util.Base64$Decoder", "decode", "byte[]", Seq("java.lang.String"))
    ).toMap
  }

  private def methodModifiers(node: JavaAstNode, staticMethod: Boolean, constructor: Boolean): List[NewModifier] = {
    val explicit = modifierTypes(node, isInterface = false)
    val dispatch =
      if (constructor) Nil
      else if (staticMethod) ModifierTypes.STATIC :: Nil
      else ModifierTypes.VIRTUAL :: Nil
    val ctor = Option.when(constructor)(ModifierTypes.CONSTRUCTOR).toList
    (explicit ++ dispatch ++ ctor).distinct.map(modifierNode(node, _))
  }

  private def modifierTypes(node: JavaAstNode, isInterface: Boolean): List[String] = {
    val explicit = node.children
      .filter(_.kind == "modifiers")
      .flatMap(_.children)
      .flatMap { modifier =>
        modifier.kind match {
          case "public"    => Some(ModifierTypes.PUBLIC)
          case "protected" => Some(ModifierTypes.PROTECTED)
          case "private"   => Some(ModifierTypes.PRIVATE)
          case "abstract"  => Some(ModifierTypes.ABSTRACT)
          case "static"    => Some(ModifierTypes.STATIC)
          case "final"     => Some(ModifierTypes.FINAL)
          case _           => None
        }
      }
      .toList
    if (isInterface && !explicit.contains(ModifierTypes.ABSTRACT)) {
      explicit :+ ModifierTypes.ABSTRACT
    } else {
      explicit
    }
  }

  private def registerMethodDeclaration(
    node: JavaAstNode,
    ownerFullName: String,
    enclosingTypeParameters: Set[String] = Set.empty
  ): Unit = {
    val name                 = childByField(node, "name").map(_.code).getOrElse("<anonymous>")
    val erasedTypeParameters = enclosingTypeParameters ++ typeParameterNames(node)
    val returnType = childByField(node, "type")
      .map(typeNode => eraseTypeParameters(typeName(typeNode), erasedTypeParameters))
      .getOrElse("void")
    val parameterTypes = parameterNodes(node, erasedTypeParameters).map(_._2)
    val signature      = composeSignature(returnType, parameterTypes)
    val info = MethodSignatureInfo(
      parameterTypes,
      returnType,
      s"$ownerFullName.$name:$signature",
      signature,
      isStatic = isStatic(node)
    )
    methodSignatureInfos.update(name, info)
    methodSignatureInfosByType.update((ownerFullName, name), info)
  }

  private def registerConstructorDeclaration(
    node: JavaAstNode,
    ownerFullName: String,
    erasedTypeParameters: Set[String] = Set.empty
  ): Unit = {
    val parameterTypes = parameterNodes(node, erasedTypeParameters).map(_._2)
    registerConstructorInfo(ownerFullName, parameterTypes)
  }

  private def registerConstructorInfo(ownerFullName: String, parameterTypes: Seq[String]): Unit = {
    val signature = composeSignature("void", parameterTypes)
    val info = MethodSignatureInfo(
      parameterTypes,
      "void",
      s"$ownerFullName.${Defines.ConstructorMethodName}:$signature",
      signature,
      isStatic = false
    )
    methodSignatureInfosByType.update((ownerFullName, Defines.ConstructorMethodName), info)
  }

  private def registerFunctionalInterface(
    node: JavaAstNode,
    fullName: String,
    simpleName: String,
    bodyChildren: Seq[JavaAstNode]
  ): Unit = {
    val abstractMethods = bodyChildren.collect {
      case method if method.kind == "method_declaration" && childByField(method, "body").isEmpty && !isStatic(method) =>
        val name           = childByField(method, "name").map(_.code).getOrElse("<anonymous>")
        val returnType     = childByField(method, "type").map(typeName).getOrElse("void")
        val parameterTypes = parameterNodes(method).map(_._2)
        FunctionalMethodInfo(name, parameterTypes, returnType)
    }
    abstractMethods match {
      case Seq(method) =>
        val info = FunctionalInterfaceInfo(fullName, typeParameterNames(node), method)
        functionalInterfaceInfos.update(fullName, info)
        functionalInterfaceInfos.update(simpleName, info)
      case _ =>
    }
  }

  private def methodInfoForInvocation(
    name: String,
    objectNode: Option[JavaAstNode],
    receiverType: Option[String] = None,
    argumentTypes: Seq[String] = Nil
  ): Option[MethodSignatureInfo] = {
    val currentSimpleType = currentTypeFullNames.headOption.map(simpleTypeName)
    val objectIsCurrentType = objectNode.exists { obj =>
      obj.code == "this" || currentSimpleType.contains(obj.code) || currentTypeFullNames.headOption.contains(obj.code)
    }
    receiverType
      .flatMap(typeFullName => methodInfoForReceiverType(typeFullName, name))
      .orElse(Option.when(objectNode.isEmpty || objectIsCurrentType)(methodSignatureInfos.get(name)).flatten)
      .orElse(Option.when(objectNode.isEmpty || objectIsCurrentType)(inheritedMethodInfoForCurrentType(name)).flatten)
      .orElse(receiverType.flatMap(javaLibraryMethodInfo(_, name, argumentTypes)))
  }

  private def methodInfoForReceiverType(receiverType: String, name: String): Option[MethodSignatureInfo] = {
    methodSignatureInfosByType
      .get(receiverType -> name)
      .orElse(inheritedMethodInfoForReceiverType(receiverType, name))
  }

  private def javaLibraryMethodInfo(
    ownerTypeFullName: String,
    name: String,
    argumentTypes: Seq[String]
  ): Option[MethodSignatureInfo] =
    JavaLibraryMethodSignatures
      .get((ownerTypeFullName, name, argumentTypes))
      .orElse {
        val candidates = JavaLibraryMethodSignatures.collect {
          case ((ownerType, methodName, parameterTypes), info)
              if ownerType == ownerTypeFullName && methodName == name && parameterTypes.size == argumentTypes.size =>
            info
        }.toSeq
        Option.when(candidates.sizeCompare(1) == 0)(candidates.head)
      }

  private def inheritedMethodInfoForCurrentType(name: String): Option[MethodSignatureInfo] = {
    currentTypeFullNames.headOption.flatMap { currentTypeFullName =>
      inheritedMethodInfoForReceiverType(currentTypeFullName, name)
    }
  }

  private def inheritedMethodInfoForReceiverType(receiverType: String, name: String): Option[MethodSignatureInfo] = {
    inheritedTypeNames(receiverType).collectFirst(Function.unlift { inheritedTypeFullName =>
      methodSignatureInfosByType.get((inheritedTypeFullName, name)).map { info =>
        if (info.isStatic) info else info.copy(fullName = s"$receiverType.$name:${info.signature}", isStatic = false)
      }
    })
  }

  private def memberTypeForDeclaredOrInheritedType(receiverType: String, fieldName: String): Option[String] = {
    declaredAndInheritedTypeNames(receiverType).collectFirst(Function.unlift { typeFullName =>
      memberTypeNamesByType.get(typeFullName -> fieldName)
    })
  }

  private def declaredAndInheritedTypeNames(typeFullName: String): Seq[String] = {
    if (typeFullName == Defines.Any) Nil else (typeFullName +: inheritedTypeNames(typeFullName)).distinct
  }

  private def inheritedTypeNames(typeFullName: String, visitedTypes: Set[String] = Set.empty): Seq[String] = {
    if (typeFullName == Defines.Any || visitedTypes.contains(typeFullName)) {
      Nil
    } else {
      inheritedTypeNamesByType
        .getOrElse(typeFullName, Nil)
        .flatMap { inheritedTypeFullName =>
          inheritedTypeFullName +: inheritedTypeNames(inheritedTypeFullName, visitedTypes + typeFullName)
        }
        .distinct
    }
  }

  private def ownerTypeForMethodInfo(methodInfo: MethodSignatureInfo): String = {
    val withoutSignature = methodInfo.fullName.stripSuffix(s":${methodInfo.signature}")
    val separatorIndex   = withoutSignature.lastIndexOf('.')
    if (separatorIndex >= 0) withoutSignature.take(separatorIndex) else Defines.Any
  }

  private def declarationHeader(node: JavaAstNode): String =
    node.code.takeWhile(ch => ch != '{' && ch != ';').trim

  private def packageNameFor(root: JavaAstNode): Option[String] = {
    root.children
      .find(_.kind == "package_declaration")
      .map { packageDecl =>
        packageDecl.code.stripPrefix("package").stripSuffix(";").trim
      }
      .filter(_.nonEmpty)
  }

  private def typeName(node: JavaAstNode): String = {
    normalizeTypeName(node.code)
  }

  private def patternTypeName(node: JavaAstNode): String = {
    val normalized = typeName(node)
    val rawBase    = baseTypeName(node.code)
    val base       = baseTypeName(normalized)
    if (isKnownPatternType(rawBase, base)) normalized else Defines.Any
  }

  private def isKnownPatternType(rawBase: String, baseType: String): Boolean = {
    PrimitiveTypes.contains(baseType) ||
    SimpleTypeAliases.contains(rawBase) ||
    rawBase.contains(".") ||
    importAliases.get(rawBase).contains(baseType) ||
    wildcardImports.exists(importedPackage => baseType == s"$importedPackage.$rawBase") ||
    resolveModuleImportedType(rawBase).contains(baseType) ||
    recordParameterInfosByType.contains(baseType) ||
    methodSignatureInfosByType.keys.exists(_._1 == baseType) ||
    memberTypeNamesByType.keys.exists(_._1 == baseType) ||
    currentTypeFullNames.contains(baseType)
  }

  private def normalizeTypeName(typeCode: String): String = {
    val normalized = typeCode.trim
      .stripSuffix("...")
      .replaceAll("\\s+", " ")
    if (normalized.endsWith("[]")) {
      s"${normalizeTypeName(normalized.stripSuffix("[]"))}[]"
    } else if (normalized.startsWith("? extends ")) {
      normalizeTypeName(normalized.stripPrefix("? extends "))
    } else if (normalized.startsWith("? super ")) {
      normalizeTypeName(normalized.stripPrefix("? super "))
    } else if (normalized == "?") {
      "java.lang.Object"
    } else {
      val (baseType, genericArgs) = typeApplication(normalized)
      if (genericArgs.nonEmpty) {
        s"${normalizeSimpleTypeName(baseType)}<${genericArgs.map(normalizeTypeName).mkString(",")}>"
      } else {
        normalizeSimpleTypeName(baseType)
      }
    }
  }

  private def normalizeSimpleTypeName(typeName: String): String = {
    val trimmed = typeName.trim
    SimpleTypeAliases
      .get(trimmed)
      .orElse(importAliases.get(trimmed))
      .orElse(resolveImportedNestedType(trimmed))
      .orElse(resolveSingleWildcardImport(trimmed))
      .orElse(resolveModuleImportedType(trimmed))
      .getOrElse(trimmed)
  }

  private def resolveImportedNestedType(typeName: String): Option[String] = {
    val parts = typeName.split('.').toList
    parts match {
      case head :: tail if tail.nonEmpty =>
        importAliases.get(head).map(importedType => importedType + "$" + tail.mkString("$"))
      case _ => None
    }
  }

  private def typeApplication(typeName: String): (String, List[String]) = {
    val trimmed = typeName.trim
    val start   = trimmed.indexOf('<')
    if (start >= 0 && trimmed.endsWith(">")) {
      trimmed.take(start).trim -> splitTopLevel(trimmed.substring(start + 1, trimmed.length - 1))
    } else {
      trimmed -> Nil
    }
  }

  private def baseTypeName(typeName: String): String = {
    val withoutVarargs = typeName.trim.stripSuffix("...")
    val withoutArrays  = Iterator.iterate(withoutVarargs)(_.stripSuffix("[]")).dropWhile(_.endsWith("[]")).next()
    typeApplication(withoutArrays)._1
  }

  private def splitTopLevel(value: String): List[String] = {
    val parts   = List.newBuilder[String]
    val current = new StringBuilder
    var depth   = 0
    value.foreach {
      case '<' =>
        depth += 1
        current.append('<')
      case '>' =>
        depth -= 1
        current.append('>')
      case ',' if depth == 0 =>
        val part = current.toString().trim
        if (part.nonEmpty) parts += part
        current.clear()
      case ch =>
        current.append(ch)
    }
    val last = current.toString().trim
    if (last.nonEmpty) parts += last
    parts.result()
  }

  private def typeParameterNames(node: JavaAstNode): Seq[String] = {
    val header = declarationHeader(node)
    val start  = header.indexOf('<')
    val end    = header.lastIndexOf('>')
    if (start >= 0 && end > start) {
      splitTopLevel(header.substring(start + 1, end))
        .map { parameter =>
          parameter.takeWhile(ch => Character.isJavaIdentifierPart(ch)).trim
        }
        .filter(_.nonEmpty)
    } else {
      Nil
    }
  }

  private def eraseTypeParameters(typeFullName: String, typeParameters: Set[String]): String = {
    if (typeParameters.isEmpty) {
      typeFullName
    } else if (typeParameters.contains(typeFullName)) {
      "java.lang.Object"
    } else if (typeFullName.endsWith("[]")) {
      s"${eraseTypeParameters(typeFullName.stripSuffix("[]"), typeParameters)}[]"
    } else {
      val (baseType, genericArgs) = typeApplication(typeFullName)
      if (genericArgs.nonEmpty) {
        s"$baseType<${genericArgs.map(eraseTypeParameters(_, typeParameters)).mkString(",")}>"
      } else {
        typeFullName
      }
    }
  }

  private def functionalInterfaceTypeName(baseType: String): String = {
    val normalized = normalizeSimpleTypeName(baseType)
    JavaUtilFunctionAliases.getOrElse(normalized, normalized)
  }

  private def simpleTypeName(typeName: String): String = {
    val (baseType, _) = typeApplication(typeName)
    baseType.split('.').lastOption.getOrElse(baseType)
  }

  private def resolveSingleWildcardImport(typeName: String): Option[String] = {
    Option.when(wildcardImports.size == 1 && typeName.headOption.exists(_.isUpper)) {
      s"${wildcardImports.head}.$typeName"
    }
  }

  private def resolveModuleImportedType(typeName: String): Option[String] = {
    if (moduleImports.contains("java.base")) JavaBaseModuleAliases.get(typeName) else None
  }

  private def namedChildren(node: JavaAstNode): List[JavaAstNode] =
    node.children.filter(_.named)

  private def childByField(node: JavaAstNode, fieldName: String): Option[JavaAstNode] =
    node.children.find(_.fieldName.contains(fieldName))

  private def methodReferenceTargetType(node: JavaAstNode): String = {
    node.kind match {
      case "array_type" | "type_identifier" | "scoped_type_identifier" | "generic_type" =>
        registerType(typeName(node))
      case "this" =>
        lookupType("this")
      case "super" =>
        currentSuperTypeFullName
      case _ =>
        val scopedType = lookupType(node.code)
        if (scopedType != Defines.Any) scopedType
        else if (node.code.headOption.exists(_.isUpper)) registerType(normalizeTypeName(node.code))
        else Defines.Any
    }
  }

  private def unparenthesizedCode(node: JavaAstNode): String = {
    val trimmed = node.code.trim
    if (node.kind == "parenthesized_expression" && trimmed.startsWith("(") && trimmed.endsWith(")")) {
      trimmed.substring(1, trimmed.length - 1).trim
    } else {
      trimmed
    }
  }

  private def withScope[T](bindings: Iterable[(String, String)])(f: => T): T = {
    localScopes = mutable.Map.from(bindings) :: localScopes
    localRefScopes = mutable.Map.empty[String, NewLocal] :: localRefScopes
    localDeclarationScopes = mutable.Map.empty[String, List[NewLocal]] :: localDeclarationScopes
    try f
    finally {
      localScopes = localScopes.tail
      localRefScopes = localRefScopes.tail
      localDeclarationScopes = localDeclarationScopes.tail
    }
  }

  private def withParameterRefs[T](bindings: Iterable[(String, NewNode)])(f: => T): T = {
    parameterRefScopes = mutable.Map.from(bindings) :: parameterRefScopes
    try f
    finally parameterRefScopes = parameterRefScopes.tail
  }

  private def withoutParameterRefs[T](f: => T): T = {
    val savedParameterRefScopes = parameterRefScopes
    val savedLocalRefScopes     = localRefScopes
    parameterRefScopes = Nil
    localRefScopes = Nil
    try f
    finally {
      parameterRefScopes = savedParameterRefScopes
      localRefScopes = savedLocalRefScopes
    }
  }

  private def withoutEnclosingLocalScopes[T](f: => T): T = {
    val savedLocalScopes            = localScopes
    val savedLocalRefScopes         = localRefScopes
    val savedLocalDeclarationScopes = localDeclarationScopes
    localScopes = Nil
    localRefScopes = Nil
    localDeclarationScopes = Nil
    try f
    finally {
      localScopes = savedLocalScopes
      localRefScopes = savedLocalRefScopes
      localDeclarationScopes = savedLocalDeclarationScopes
    }
  }

  private def withReturnType[T](typeFullName: String)(f: => T): T = {
    currentMethodReturnTypes = typeFullName :: currentMethodReturnTypes
    try f
    finally currentMethodReturnTypes = currentMethodReturnTypes.tail
  }

  private def withMethodFullName[T](fullName: String)(f: => T): T = {
    currentMethodFullNames = fullName :: currentMethodFullNames
    try f
    finally currentMethodFullNames = currentMethodFullNames.tail
  }

  private def withExpectedExpressionType[T](typeFullName: Option[String])(f: => T): T = {
    expectedExpressionTypes = typeFullName :: expectedExpressionTypes
    try f
    finally expectedExpressionTypes = expectedExpressionTypes.tail
  }

  private def currentExpectedExpressionType: Option[String] =
    expectedExpressionTypes.collectFirst { case Some(typeFullName) => typeFullName }

  private def withAnonymousOwnerBase[T](ownerBase: String)(f: => T): T = {
    anonymousOwnerBases = ownerBase :: anonymousOwnerBases
    try f
    finally anonymousOwnerBases = anonymousOwnerBases.tail
  }

  private def nextLambdaName(): String = {
    val name = s"<lambda>$lambdaCounter"
    lambdaCounter += 1
    name
  }

  private def nextObjectCreationTempName(): String = {
    val name = s"$$obj$objectCreationTempCounter"
    objectCreationTempCounter += 1
    name
  }

  private def localRecordCaptureArgumentAsts(typeFullName: String, originNode: JavaAstNode): Seq[Ast] = {
    localRecordCaptureInfosByType.getOrElse(typeFullName, Nil).map { capture =>
      if (anonymousOuterClassTypes.contains(typeFullName) && capture.name == "outerClass") {
        val identifier = identifierNode(originNode, "this", "this", capture.typeFullName)
        localRef("this")
          .map(local => Ast(identifier).withRefEdge(identifier, local))
          .orElse(parameterRef("this").map(target => Ast(identifier).withRefEdge(identifier, target)))
          .getOrElse(Ast(identifier))
      } else {
        val identifier = identifierNode(originNode, capture.name, capture.name, capture.typeFullName)
        localRef(capture.name)
          .map(local => Ast(identifier).withRefEdge(identifier, local))
          .orElse(parameterRef(capture.name).map(target => Ast(identifier).withRefEdge(identifier, target)))
          .getOrElse(Ast(identifier))
      }
    }
  }

  private def declareLocal(name: String, typeFullName: String, local: Option[NewLocal] = None): Unit = {
    val normalized = name.trim
    localScopes.headOption.foreach(_.update(normalized, typeFullName))
    local.foreach(localNode => localRefScopes.headOption.foreach(_.update(normalized, localNode)))
  }

  private def scopedLocalForDeclaration(sourceName: String, typeFullName: String)(
    createLocal: String => NewLocal
  ): ScopedLocal = {
    reusableLocal(sourceName, typeFullName) match {
      case Some(local) =>
        declareLocal(sourceName, typeFullName, Some(local))
        ScopedLocal(local, emitLocal = false)
      case None =>
        val localName = uniqueLocalName(sourceName)
        val local     = createLocal(localName)
        reserveLocalName(sourceName, local)
        declareLocal(sourceName, typeFullName, Some(local))
        ScopedLocal(local, emitLocal = true)
    }
  }

  private def declarePatternLocal(
    sourceName: String,
    typeFullName: String,
    createLocal: String => NewLocal
  ): NewLocal = {
    val normalized    = sourceName.trim
    val previousType  = localScopes.headOption.flatMap(_.get(normalized))
    val previousLocal = localRefScopes.headOption.flatMap(_.get(normalized))
    val reusable      = reusableLocal(sourceName, typeFullName)
    val local         = reusable.getOrElse(createLocal(uniqueLocalName(sourceName)))
    declareLocal(sourceName, typeFullName, Some(local))
    pendingPatternLocals = PatternLocalInfo(
      normalized,
      local,
      previousType,
      previousLocal,
      emitLocal = reusable.isEmpty
    ) :: pendingPatternLocals
    local
  }

  private def reusableLocal(sourceName: String, typeFullName: String): Option[NewLocal] = {
    val normalized = sourceName.trim
    localDeclarationScopes.collectFirst {
      case scope if scope.get(normalized).exists(_.exists(_.typeFullName == typeFullName)) =>
        scope(normalized).find(_.typeFullName == typeFullName).get
    }
  }

  private def reserveLocalName(sourceName: String, local: NewLocal): Unit = {
    val normalized = sourceName.trim
    localDeclarationScopes.headOption.foreach { scope =>
      scope.update(normalized, local :: scope.getOrElse(normalized, Nil))
    }
  }

  private def uniqueLocalName(sourceName: String): String = {
    val normalized = sourceName.trim
    val usedNames = localDeclarationScopes
      .flatMap(_.values.flatten.map(_.name))
      .toSet
    if (!usedNames.contains(normalized)) {
      normalized
    } else {
      Iterator
        .from(0)
        .map(index => s"$normalized$$$index")
        .find(name => !usedNames.contains(name))
        .getOrElse(normalized)
    }
  }

  private def drainPendingPatternLocals(initialCount: Int): Seq[Ast] = {
    val createdCount = pendingPatternLocals.size - initialCount
    if (createdCount <= 0) {
      Seq.empty
    } else {
      val created = newlyPendingPatternLocals(initialCount)
      pendingPatternLocals = pendingPatternLocals.drop(createdCount)
      restorePatternLocals(created.filterNot(_.survivesStatement))
      created.reverse.flatMap { info =>
        Option.when(info.emitLocal) {
          reserveLocalName(info.sourceName, info.local)
          Ast(info.local)
        }
      }
    }
  }

  private def newlyPendingPatternLocals(initialCount: Int): Seq[PatternLocalInfo] =
    pendingPatternLocals.take(math.max(pendingPatternLocals.size - initialCount, 0))

  private def bindPatternLocals(locals: Seq[PatternLocalInfo]): Unit = {
    locals.reverse.foreach { info =>
      val normalized = info.sourceName.trim
      localScopes.headOption.foreach(_.update(normalized, info.local.typeFullName))
      localRefScopes.headOption.foreach(_.update(normalized, info.local))
    }
  }

  private def markPatternLocalsSurviveStatement(locals: Seq[PatternLocalInfo]): Unit = {
    val survivingLocals = locals.map(_.local).toSet
    pendingPatternLocals = pendingPatternLocals.map { info =>
      if (survivingLocals.contains(info.local)) info.copy(survivesStatement = true) else info
    }
  }

  private def restorePatternLocals(locals: Seq[PatternLocalInfo]): Unit = {
    locals.foreach { info =>
      val normalized = info.sourceName.trim
      info.previousType match {
        case Some(typeFullName) => localScopes.headOption.foreach(_.update(normalized, typeFullName))
        case None               => localScopes.headOption.foreach(_.remove(normalized))
      }
      info.previousLocal match {
        case Some(local) => localRefScopes.headOption.foreach(_.update(normalized, local))
        case None        => localRefScopes.headOption.foreach(_.remove(normalized))
      }
    }
  }

  private def parameterBindings(parameterAsts: Seq[Ast]): Seq[(String, NewNode)] = {
    parameterAsts.flatMap(_.root).collect { case parameter: NewMethodParameterIn => parameter.name -> parameter }
  }

  private def parameterRef(name: String): Option[NewNode] = {
    val normalized = name.trim
    parameterRefScopes.collectFirst {
      case scope if scope.contains(normalized) => scope(normalized)
    }
  }

  private def visibleLocalType(name: String): Option[String] = {
    val normalized = name.trim
    localScopes.collectFirst {
      case scope if scope.contains(normalized) => scope(normalized)
    }
  }

  private def localRef(name: String): Option[NewLocal] = {
    val normalized = name.trim
    localRefScopes.collectFirst {
      case scope if scope.contains(normalized) => scope(normalized)
    }
  }

  private def bindingAstsForMethods(methodAsts: Seq[Ast]): Seq[Ast] = {
    methodAsts.flatMap(_.root).collect { case method: NewMethod =>
      Ast(NewBinding().name(method.name).signature(method.signature).methodFullName(method.fullName))
    }
  }

  private def typeDeclAstWithBindings(typeDeclAst: Ast, typeDecl: NewTypeDecl, bindingAsts: Seq[Ast]): Ast = {
    val withBindingNodes = bindingAsts.foldLeft(typeDeclAst) { case (ast, bindingAst) => ast.merge(bindingAst) }
    withBindingNodes.withBindsEdges(typeDecl, bindingAsts.flatMap(_.root).toList)
  }

  private def lookupType(name: String): String = {
    val normalized = name.trim
    localScopes
      .collectFirst {
        case scope if scope.contains(normalized) => scope(normalized)
      }
      .orElse(memberTypeNames.get(normalized))
      .getOrElse(Defines.Any)
  }

  private def lookupTypeOrTypeName(name: String): String = {
    val localType = lookupType(name)
    if (localType != Defines.Any) {
      localType
    } else if (name.trim.headOption.exists(_.isUpper)) {
      registerType(normalizeTypeName(name))
    } else {
      Defines.Any
    }
  }

  private def expressionType(ast: Ast): String = {
    ast.root
      .collect {
        case call: NewCall             => call.typeFullName
        case identifier: NewIdentifier => identifier.typeFullName
        case literal: NewLiteral       => literal.typeFullName
        case local: NewLocal           => local.typeFullName
        case block: NewBlock           => block.typeFullName
        case methodRef: NewMethodRef   => methodRef.typeFullName
      }
      .filter(typeFullName => typeFullName != null && typeFullName.nonEmpty)
      .getOrElse(Defines.Any)
  }

  private def astRootCode(ast: Ast): String =
    ast.root.collect { case node: AstNodeNew => node.code }.getOrElse("")

  private def firstKnownType(asts: Ast*): String = {
    asts.map(expressionType).find(_ != Defines.Any).getOrElse(Defines.Any)
  }

  private def elementType(typeFullName: String): String = {
    if (typeFullName.endsWith("[]")) typeFullName.stripSuffix("[]")
    else Defines.Any
  }

  private def literalTypeName(node: JavaAstNode): String = {
    node.kind match {
      case "string_literal" | "text_block"          => "java.lang.String"
      case "character_literal"                      => "char"
      case "true" | "false" | "boolean_literal"     => "boolean"
      case "null_literal" | "null"                  => "null"
      case kind if kind.contains("floating_point")  => "double"
      case kind if kind.endsWith("integer_literal") => "int"
      case _                                        => Defines.Any
    }
  }

  private def binaryOperatorName(operator: String): String = {
    operator match {
      case "||"  => Operators.logicalOr
      case "&&"  => Operators.logicalAnd
      case "|"   => Operators.or
      case "^"   => Operators.xor
      case "&"   => Operators.and
      case "=="  => Operators.equals
      case "!="  => Operators.notEquals
      case "<"   => Operators.lessThan
      case ">"   => Operators.greaterThan
      case "<="  => Operators.lessEqualsThan
      case ">="  => Operators.greaterEqualsThan
      case "<<"  => Operators.shiftLeft
      case ">>"  => Operators.logicalShiftRight
      case ">>>" => Operators.arithmeticShiftRight
      case "+"   => Operators.addition
      case "-"   => Operators.subtraction
      case "*"   => Operators.multiplication
      case "/"   => Operators.division
      case "%"   => Operators.modulo
      case _     => Defines.Unknown
    }
  }

  private def assignmentOperatorName(operator: String): String = {
    operator match {
      case "="    => Operators.assignment
      case "+="   => Operators.assignmentPlus
      case "-="   => Operators.assignmentMinus
      case "*="   => Operators.assignmentMultiplication
      case "/="   => Operators.assignmentDivision
      case "%="   => Operators.assignmentModulo
      case "&="   => Operators.assignmentAnd
      case "|="   => Operators.assignmentOr
      case "^="   => Operators.assignmentXor
      case "<<="  => Operators.assignmentShiftLeft
      case ">>="  => Operators.assignmentArithmeticShiftRight
      case ">>>=" => Operators.assignmentLogicalShiftRight
      case _      => Defines.Unknown
    }
  }

  private def typeNameFromAllocationCode(code: String): String = {
    val withoutNew = code.stripPrefix("new").trim
    val raw        = withoutNew.takeWhile(ch => !ch.isWhitespace && ch != '(' && ch != '{').trim
    if (raw.nonEmpty) normalizeTypeName(raw) else Defines.Any
  }

  private def typeNameFromArrayCreationCode(code: String): String = {
    val withoutNew = code.stripPrefix("new").trim
    val raw        = withoutNew.takeWhile(ch => !ch.isWhitespace && ch != '[' && ch != '(' && ch != '{').trim
    if (raw.nonEmpty) normalizeTypeName(raw) else Defines.Any
  }

  private def arrayDimensionCount(node: JavaAstNode): Int = {
    val countFromChildren = node.children
      .filter(_.fieldName.contains("dimensions"))
      .map(_.code.count(_ == '['))
      .sum
    if (countFromChildren > 0) countFromChildren else node.code.count(_ == '[')
  }

  private def isStatic(node: JavaAstNode): Boolean =
    modifierTypes(node, isInterface = false).contains(ModifierTypes.STATIC)

  private def evaluationStrategyFor(typeFullName: String): String =
    if (PrimitiveTypes.contains(typeFullName)) EvaluationStrategies.BY_VALUE else EvaluationStrategies.BY_SHARING

  private def registerType(typeFullName: String): String = {
    usedTypeNames += typeFullName
    typeFullName
  }

  private def registerMember(ownerFullName: String, name: String, typeFullName: String): Unit = {
    memberTypeNames.update(name, typeFullName)
    memberTypeNamesByType.update(ownerFullName -> name, typeFullName)
  }

  private case class MethodSignatureInfo(
    parameterTypes: Seq[String],
    returnType: String,
    fullName: String,
    signature: String,
    isStatic: Boolean
  )
  private case class LambdaParameterInfo(node: JavaAstNode, name: String, typeFullName: String, code: String)
  private case class LambdaSignatureInfo(
    interfaceType: String,
    methodName: String,
    parameterTypes: Seq[String],
    returnType: String
  )
  private case class RecordParameterInfo(node: JavaAstNode, name: String, typeFullName: String, typeCode: String)
  private case class CaptureInfo(node: JavaAstNode, name: String, typeFullName: String)
  private case class LambdaCaptureInfo(name: String, typeFullName: String, refTarget: NewNode)
  private case class LambdaCaptureLocal(info: LambdaCaptureInfo, local: NewLocal, closureBinding: NewClosureBinding)
  private case class SyntheticConstructorParameterInfo(node: JavaAstNode, name: String, typeFullName: String)
  private case class RecordPatternAccess(ast: Ast, code: String, typeFullName: String)
  private class RecordPatternInitNode(
    val patternNode: JavaAstNode,
    val patternTypeFullName: String,
    val patternTypeNode: JavaAstNode,
    val typeFullName: Option[String],
    createInitialAccess: () => RecordPatternAccess,
    requiresTemporaryVariable: Boolean,
    val isRoot: Boolean = false
  ) {
    private var firstUse      = true
    private lazy val tmpType  = typeFullName.getOrElse(Defines.Any)
    private lazy val tmpName  = nextObjectCreationTempName()
    private lazy val tmpLocal = localNode(patternNode, tmpName, tmpName, tmpType)

    def getAccess(): RecordPatternAccess = {
      if (!requiresTemporaryVariable) {
        createInitialAccess()
      } else if (firstUse) {
        firstUse = false
        val initializer = createInitialAccess()
        declarePatternLocal(tmpName, tmpType, _ => tmpLocal)
        val tmpIdentifier    = identifierNode(patternNode, tmpName, tmpName, tmpType)
        val tmpIdentifierAst = Ast(tmpIdentifier).withRefEdge(tmpIdentifier, tmpLocal)
        val assignmentCode   = s"$tmpName = ${initializer.code}"
        val assignmentCall =
          callNode(
            patternNode,
            assignmentCode,
            Operators.assignment,
            Operators.assignment,
            DispatchTypes.STATIC_DISPATCH,
            None,
            Some(tmpType)
          )
        RecordPatternAccess(callAst(assignmentCall, Seq(tmpIdentifierAst, initializer.ast)), assignmentCode, tmpType)
      } else {
        val tmpIdentifier = identifierNode(patternNode, tmpName, tmpName, tmpType)
        RecordPatternAccess(Ast(tmpIdentifier).withRefEdge(tmpIdentifier, tmpLocal), tmpName, tmpType)
      }
    }
  }
  private case class ScopedLocal(local: NewLocal, emitLocal: Boolean)
  private case class PatternLocalInfo(
    sourceName: String,
    local: NewLocal,
    previousType: Option[String],
    previousLocal: Option[NewLocal],
    emitLocal: Boolean,
    survivesStatement: Boolean = false
  )
  private case class PatternBranchExposure(thenBranch: Boolean, elseBranch: Boolean) {
    def swapped: PatternBranchExposure = PatternBranchExposure(thenBranch = elseBranch, elseBranch = thenBranch)
  }
  private object PatternBranchExposure {
    val None: PatternBranchExposure = PatternBranchExposure(thenBranch = false, elseBranch = false)
  }
  private case class FunctionalMethodInfo(name: String, parameterTypes: Seq[String], returnType: String)
  private case class FunctionalInterfaceInfo(
    interfaceType: String,
    typeParameters: Seq[String],
    method: FunctionalMethodInfo
  ) {
    def instantiate(typeArguments: Seq[String]): LambdaSignatureInfo = {
      val substitutions                            = typeParameters.zip(typeArguments).toMap
      def substitute(typeFullName: String): String = substitutions.getOrElse(typeFullName, typeFullName)
      LambdaSignatureInfo(
        interfaceType,
        method.name,
        method.parameterTypes.map(substitute),
        substitute(method.returnType)
      )
    }
  }

  private val SimpleTypeAliases = Map(
    "String"    -> "java.lang.String",
    "Object"    -> "java.lang.Object",
    "Boolean"   -> "java.lang.Boolean",
    "Byte"      -> "java.lang.Byte",
    "Character" -> "java.lang.Character",
    "Short"     -> "java.lang.Short",
    "Integer"   -> "java.lang.Integer",
    "Long"      -> "java.lang.Long",
    "Float"     -> "java.lang.Float",
    "Double"    -> "java.lang.Double",
    "Void"      -> "java.lang.Void"
  )

  private val JavaUtilFunctionAliases = Map(
    "Function"       -> "java.util.function.Function",
    "Consumer"       -> "java.util.function.Consumer",
    "Supplier"       -> "java.util.function.Supplier",
    "Predicate"      -> "java.util.function.Predicate",
    "BiFunction"     -> "java.util.function.BiFunction",
    "BiConsumer"     -> "java.util.function.BiConsumer",
    "UnaryOperator"  -> "java.util.function.UnaryOperator",
    "BinaryOperator" -> "java.util.function.BinaryOperator"
  )

  private val JavaBaseModuleAliases = JavaUtilFunctionAliases ++ Map(
    "Appendable"                     -> "java.lang.Appendable",
    "ArithmeticException"            -> "java.lang.ArithmeticException",
    "ArrayDeque"                     -> "java.util.ArrayDeque",
    "ArrayIndexOutOfBoundsException" -> "java.lang.ArrayIndexOutOfBoundsException",
    "ArrayList"                      -> "java.util.ArrayList",
    "Arrays"                         -> "java.util.Arrays",
    "AutoCloseable"                  -> "java.lang.AutoCloseable",
    "Base64"                         -> "java.util.Base64",
    "BigDecimal"                     -> "java.math.BigDecimal",
    "BigInteger"                     -> "java.math.BigInteger",
    "BitSet"                         -> "java.util.BitSet",
    "BlockingDeque"                  -> "java.util.concurrent.BlockingDeque",
    "BlockingQueue"                  -> "java.util.concurrent.BlockingQueue",
    "Boolean"                        -> "java.lang.Boolean",
    "Buffer"                         -> "java.nio.Buffer",
    "BufferedReader"                 -> "java.io.BufferedReader",
    "BufferedWriter"                 -> "java.io.BufferedWriter",
    "Byte"                           -> "java.lang.Byte",
    "ByteArrayInputStream"           -> "java.io.ByteArrayInputStream",
    "ByteArrayOutputStream"          -> "java.io.ByteArrayOutputStream",
    "ByteBuffer"                     -> "java.nio.ByteBuffer",
    "Callable"                       -> "java.util.concurrent.Callable",
    "Calendar"                       -> "java.util.Calendar",
    "CharBuffer"                     -> "java.nio.CharBuffer",
    "CharSequence"                   -> "java.lang.CharSequence",
    "Character"                      -> "java.lang.Character",
    "Cipher"                         -> "javax.crypto.Cipher",
    "Class"                          -> "java.lang.Class",
    "ClassCastException"             -> "java.lang.ClassCastException",
    "ClassLoader"                    -> "java.lang.ClassLoader",
    "Closeable"                      -> "java.io.Closeable",
    "Clock"                          -> "java.time.Clock",
    "Collection"                     -> "java.util.Collection",
    "Collections"                    -> "java.util.Collections",
    "Collector"                      -> "java.util.stream.Collector",
    "Collectors"                     -> "java.util.stream.Collectors",
    "Comparable"                     -> "java.lang.Comparable",
    "CompletableFuture"              -> "java.util.concurrent.CompletableFuture",
    "ConcurrentHashMap"              -> "java.util.concurrent.ConcurrentHashMap",
    "ConcurrentMap"                  -> "java.util.concurrent.ConcurrentMap",
    "CopyOnWriteArrayList"           -> "java.util.concurrent.CopyOnWriteArrayList",
    "CopyOnWriteArraySet"            -> "java.util.concurrent.CopyOnWriteArraySet",
    "CountDownLatch"                 -> "java.util.concurrent.CountDownLatch",
    "Currency"                       -> "java.util.Currency",
    "DataInput"                      -> "java.io.DataInput",
    "DataInputStream"                -> "java.io.DataInputStream",
    "DataOutput"                     -> "java.io.DataOutput",
    "DataOutputStream"               -> "java.io.DataOutputStream",
    "Date"                           -> "java.util.Date",
    "DateFormat"                     -> "java.text.DateFormat",
    "DayOfWeek"                      -> "java.time.DayOfWeek",
    "DecimalFormat"                  -> "java.text.DecimalFormat",
    "Deque"                          -> "java.util.Deque",
    "Deprecated"                     -> "java.lang.Deprecated",
    "DirectoryStream"                -> "java.nio.file.DirectoryStream",
    "Double"                         -> "java.lang.Double",
    "DoubleBuffer"                   -> "java.nio.DoubleBuffer",
    "DoubleStream"                   -> "java.util.stream.DoubleStream",
    "Duration"                       -> "java.time.Duration",
    "Enum"                           -> "java.lang.Enum",
    "EnumMap"                        -> "java.util.EnumMap",
    "EnumSet"                        -> "java.util.EnumSet",
    "Enumeration"                    -> "java.util.Enumeration",
    "Error"                          -> "java.lang.Error",
    "Exception"                      -> "java.lang.Exception",
    "Executor"                       -> "java.util.concurrent.Executor",
    "ExecutorService"                -> "java.util.concurrent.ExecutorService",
    "Executors"                      -> "java.util.concurrent.Executors",
    "File"                           -> "java.io.File",
    "FileInputStream"                -> "java.io.FileInputStream",
    "FileOutputStream"               -> "java.io.FileOutputStream",
    "FileReader"                     -> "java.io.FileReader",
    "FileSystem"                     -> "java.nio.file.FileSystem",
    "FileSystems"                    -> "java.nio.file.FileSystems",
    "FileWriter"                     -> "java.io.FileWriter",
    "Files"                          -> "java.nio.file.Files",
    "Float"                          -> "java.lang.Float",
    "FloatBuffer"                    -> "java.nio.FloatBuffer",
    "Flushable"                      -> "java.io.Flushable",
    "Format"                         -> "java.text.Format",
    "Future"                         -> "java.util.concurrent.Future",
    "GregorianCalendar"              -> "java.util.GregorianCalendar",
    "HashMap"                        -> "java.util.HashMap",
    "HashSet"                        -> "java.util.HashSet",
    "Hashtable"                      -> "java.util.Hashtable",
    "HttpURLConnection"              -> "java.net.HttpURLConnection",
    "IdentityHashMap"                -> "java.util.IdentityHashMap",
    "InetAddress"                    -> "java.net.InetAddress",
    "InetSocketAddress"              -> "java.net.InetSocketAddress",
    "InputStream"                    -> "java.io.InputStream",
    "Instant"                        -> "java.time.Instant",
    "IntBuffer"                      -> "java.nio.IntBuffer",
    "IntStream"                      -> "java.util.stream.IntStream",
    "Integer"                        -> "java.lang.Integer",
    "InterruptedException"           -> "java.lang.InterruptedException",
    "IOException"                    -> "java.io.IOException",
    "Iterable"                       -> "java.lang.Iterable",
    "Iterator"                       -> "java.util.Iterator",
    "JarEntry"                       -> "java.util.jar.JarEntry",
    "JarFile"                        -> "java.util.jar.JarFile",
    "Key"                            -> "java.security.Key",
    "KeyPair"                        -> "java.security.KeyPair",
    "KeyPairGenerator"               -> "java.security.KeyPairGenerator",
    "LinkedHashMap"                  -> "java.util.LinkedHashMap",
    "LinkedHashSet"                  -> "java.util.LinkedHashSet",
    "LinkedList"                     -> "java.util.LinkedList",
    "LinkOption"                     -> "java.nio.file.LinkOption",
    "List"                           -> "java.util.List",
    "LocalDate"                      -> "java.time.LocalDate",
    "LocalDateTime"                  -> "java.time.LocalDateTime",
    "LocalTime"                      -> "java.time.LocalTime",
    "Locale"                         -> "java.util.Locale",
    "Long"                           -> "java.lang.Long",
    "LongBuffer"                     -> "java.nio.LongBuffer",
    "LongStream"                     -> "java.util.stream.LongStream",
    "Mac"                            -> "javax.crypto.Mac",
    "Map"                            -> "java.util.Map",
    "Matcher"                        -> "java.util.regex.Matcher",
    "Math"                           -> "java.lang.Math",
    "MathContext"                    -> "java.math.MathContext",
    "MessageDigest"                  -> "java.security.MessageDigest",
    "Module"                         -> "java.lang.Module",
    "Month"                          -> "java.time.Month",
    "NoSuchElementException"         -> "java.util.NoSuchElementException",
    "NullPointerException"           -> "java.lang.NullPointerException",
    "Number"                         -> "java.lang.Number",
    "NumberFormat"                   -> "java.text.NumberFormat",
    "Object"                         -> "java.lang.Object",
    "ObjectInputStream"              -> "java.io.ObjectInputStream",
    "ObjectOutputStream"             -> "java.io.ObjectOutputStream",
    "Objects"                        -> "java.util.Objects",
    "OffsetDateTime"                 -> "java.time.OffsetDateTime",
    "OpenOption"                     -> "java.nio.file.OpenOption",
    "Optional"                       -> "java.util.Optional",
    "OptionalDouble"                 -> "java.util.OptionalDouble",
    "OptionalInt"                    -> "java.util.OptionalInt",
    "OptionalLong"                   -> "java.util.OptionalLong",
    "OutputStream"                   -> "java.io.OutputStream",
    "Override"                       -> "java.lang.Override",
    "ParseException"                 -> "java.text.ParseException",
    "Path"                           -> "java.nio.file.Path",
    "Paths"                          -> "java.nio.file.Paths",
    "Pattern"                        -> "java.util.regex.Pattern",
    "Period"                         -> "java.time.Period",
    "Principal"                      -> "java.security.Principal",
    "PrintStream"                    -> "java.io.PrintStream",
    "PrintWriter"                    -> "java.io.PrintWriter",
    "PriorityQueue"                  -> "java.util.PriorityQueue",
    "Process"                        -> "java.lang.Process",
    "ProcessBuilder"                 -> "java.lang.ProcessBuilder",
    "Properties"                     -> "java.util.Properties",
    "Queue"                          -> "java.util.Queue",
    "Random"                         -> "java.util.Random",
    "Reader"                         -> "java.io.Reader",
    "Record"                         -> "java.lang.Record",
    "ResourceBundle"                 -> "java.util.ResourceBundle",
    "RoundingMode"                   -> "java.math.RoundingMode",
    "Runnable"                       -> "java.lang.Runnable",
    "Runtime"                        -> "java.lang.Runtime",
    "RuntimeException"               -> "java.lang.RuntimeException",
    "Scanner"                        -> "java.util.Scanner",
    "SecureRandom"                   -> "java.security.SecureRandom",
    "Semaphore"                      -> "java.util.concurrent.Semaphore",
    "Serializable"                   -> "java.io.Serializable",
    "Set"                            -> "java.util.Set",
    "Short"                          -> "java.lang.Short",
    "ShortBuffer"                    -> "java.nio.ShortBuffer",
    "Signature"                      -> "java.security.Signature",
    "SimpleDateFormat"               -> "java.text.SimpleDateFormat",
    "Socket"                         -> "java.net.Socket",
    "Stack"                          -> "java.util.Stack",
    "StackTraceElement"              -> "java.lang.StackTraceElement",
    "StandardOpenOption"             -> "java.nio.file.StandardOpenOption",
    "Stream"                         -> "java.util.stream.Stream",
    "String"                         -> "java.lang.String",
    "StringBuffer"                   -> "java.lang.StringBuffer",
    "StringBuilder"                  -> "java.lang.StringBuilder",
    "StringReader"                   -> "java.io.StringReader",
    "StringTokenizer"                -> "java.util.StringTokenizer",
    "StringWriter"                   -> "java.io.StringWriter",
    "SuppressWarnings"               -> "java.lang.SuppressWarnings",
    "System"                         -> "java.lang.System",
    "Thread"                         -> "java.lang.Thread",
    "Throwable"                      -> "java.lang.Throwable",
    "TimeUnit"                       -> "java.util.concurrent.TimeUnit",
    "TimeZone"                       -> "java.util.TimeZone",
    "Timer"                          -> "java.util.Timer",
    "TimerTask"                      -> "java.util.TimerTask",
    "TreeMap"                        -> "java.util.TreeMap",
    "TreeSet"                        -> "java.util.TreeSet",
    "URI"                            -> "java.net.URI",
    "URL"                            -> "java.net.URL",
    "URLDecoder"                     -> "java.net.URLDecoder",
    "URLEncoder"                     -> "java.net.URLEncoder",
    "UUID"                           -> "java.util.UUID",
    "Vector"                         -> "java.util.Vector",
    "Void"                           -> "java.lang.Void",
    "WatchEvent"                     -> "java.nio.file.WatchEvent",
    "WatchKey"                       -> "java.nio.file.WatchKey",
    "WatchService"                   -> "java.nio.file.WatchService",
    "WeakHashMap"                    -> "java.util.WeakHashMap",
    "Writer"                         -> "java.io.Writer",
    "ZipEntry"                       -> "java.util.zip.ZipEntry",
    "ZipFile"                        -> "java.util.zip.ZipFile",
    "ZoneId"                         -> "java.time.ZoneId",
    "ZoneOffset"                     -> "java.time.ZoneOffset",
    "ZonedDateTime"                  -> "java.time.ZonedDateTime"
  )

  private val PrimitiveTypes     = Set("boolean", "byte", "char", "short", "int", "long", "float", "double", "void")
  private val InheritedTypeKinds = Set("type_identifier", "scoped_type_identifier", "generic_type")
  private val LiteralKinds = Set(
    "decimal_integer_literal",
    "hex_integer_literal",
    "octal_integer_literal",
    "binary_integer_literal",
    "decimal_floating_point_literal",
    "hex_floating_point_literal",
    "string_literal",
    "character_literal",
    "text_block",
    "true",
    "false",
    "boolean_literal",
    "null_literal",
    "null"
  )
  private val BooleanBinaryOperators = Set("||", "&&", "==", "!=", "<", ">", "<=", ">=")
  private val AnnotationKinds        = Set("annotation", "marker_annotation")

  private val TypeDeclarationKinds = Set(
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration"
  )
}
