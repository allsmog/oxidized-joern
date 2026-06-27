package io.joern.rust2cpg.astcreation

import io.joern.rust2cpg.parser.RustNodeSyntax.*
import io.joern.x2cpg.datastructures.Stack.*
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DispatchTypes,
  EvaluationStrategies,
  ModifierTypes,
  Operators
}
import io.shiftleft.codepropertygraph.generated.nodes.{NewBlock, NewCall, NewFile, NewModifier, NewNamespaceBlock}

import scala.annotation.tailrec

trait RustVisitor(implicit withValidationMode: ValidationMode) { this: AstCreator =>

  private var macroDefinitionScopes = List(Map.empty[String, Seq[SimpleMacroRule]])

  private def withMacroDefinitionScope[A](body: => A): A = {
    macroDefinitionScopes = Map.empty[String, Seq[SimpleMacroRule]] :: macroDefinitionScopes
    try body
    finally macroDefinitionScopes = macroDefinitionScopes.tail
  }

  private def bindMacroDefinition(name: String, rules: Seq[SimpleMacroRule]): Unit = {
    if (name.nonEmpty && rules.nonEmpty) {
      macroDefinitionScopes = macroDefinitionScopes match {
        case frame :: rest => frame.updated(name, rules) :: rest
        case Nil           => List(Map(name -> rules))
      }
    }
  }

  private def lookupMacroDefinition(name: String): Option[Seq[SimpleMacroRule]] = {
    macroDefinitionScopes.collectFirst {
      case frame if frame.contains(name) => frame(name)
    }
  }

  // SourceFile =
  //  '#shebang'?
  //  '#frontmatter'?
  //  Attr*
  //  Item*
  protected def visitSourceFile(sourceFile: SourceFile): Ast = {
    val fileNode = NewFile().name(parseResult.filename).order(0)
    Option.unless(config.disableFileContent)(parseResult.fileContent).foreach(fileNode.content(_))

    val namespaceBlockNode = globalNamespaceBlockNode()
    val globalMethodAst    = astInFakeMethod(sourceFile, namespaceBlockNode)

    Ast(fileNode).withChild(Ast(namespaceBlockNode).withChild(globalMethodAst))
  }

  private def astInFakeMethod(sourceFile: SourceFile, namespaceBlockNode: NewNamespaceBlock): Ast = {
    val method = globalFakeMethodNode(sourceFile, namespaceBlockNode)

    methodAstParentStack.push(namespaceBlockNode)
    methodAstParentStack.push(method)
    sourceFile.item.foreach(bindImportsForItem)
    val itemAsts = sourceFile.item.flatMap(visitItem)
    methodAstParentStack.pop()
    methodAstParentStack.pop()

    val block        = blockNode(sourceFile)
    val methodReturn = methodReturnNode(sourceFile, "()")
    val modifiers    = Seq(modifierNode(sourceFile, ModifierTypes.MODULE))
    methodAst(method, parameters = Nil, body = Ast(block).withChildren(itemAsts), methodReturn, modifiers)
  }

  private def visitItem(item: Item): Seq[Ast] = item match {
    case const: Const             => visitConst(const)
    case enumItem: Enum           => visitEnum(enumItem) :: Nil
    case externBlock: ExternBlock => visitExternBlock(externBlock)
    case externCrate: ExternCrate => visitExternCrate(externCrate) :: Nil
    case fn: Fn                   => visitFn(fn) :: Nil
    case implItem: Impl           => visitImpl(implItem)
    case macroCall: MacroCall =>
      visitMacroCall(macroCall) :: Nil
    case macroRules: MacroRules => visitMacroRules(macroRules) :: Nil
    case macroDef: MacroDef     => visitMacroDef(macroDef) :: Nil
    case module: Module         => visitModule(module)
    case static: Static         => visitStatic(static)
    case struct: Struct         => visitStruct(struct) :: Nil
    case traitItem: Trait       => visitTrait(traitItem) :: Nil
    case typeAlias: TypeAlias   => visitTypeAlias(typeAlias) :: Nil
    case union: Union           => visitUnion(union) :: Nil
    case useItem: Use           => visitUse(useItem)
    case asmExpr: AsmExpr       => visitAsmExpr(asmExpr) :: Nil
  }

  private def visitStmt(stmt: Stmt): Seq[Ast] = stmt match {
    case exprStmt: ExprStmt => visitExpr(exprStmt.expr) :: Nil
    case item: Item         => visitItem(item)
    case letStmt: LetStmt   => visitLetStmt(letStmt)
  }

  @tailrec
  private def visitExpr(expr: Expr): Ast = expr match {
    case arrayExpr: ArrayExpr           => visitArrayExpr(arrayExpr)
    case asmExpr: AsmExpr               => visitAsmExpr(asmExpr)
    case awaitExpr: AwaitExpr           => visitAwaitExpr(awaitExpr)
    case binExpr: BinExpr               => visitBinExpr(binExpr)
    case blockExpr: BlockExpr           => visitBlockExpr(blockExpr)
    case breakExpr: BreakExpr           => visitBreakExpr(breakExpr)
    case callExpr: CallExpr             => visitCallExpr(callExpr)
    case castExpr: CastExpr             => visitCastExpr(castExpr)
    case closureExpr: ClosureExpr       => visitClosureExpr(closureExpr)
    case continueExpr: ContinueExpr     => visitContinueExpr(continueExpr)
    case fieldExpr: FieldExpr           => visitFieldExpr(fieldExpr)
    case forExpr: ForExpr               => visitForExpr(forExpr)
    case formatArgsExpr: FormatArgsExpr => visitFormatArgsExpr(formatArgsExpr)
    case ifExpr: IfExpr                 => visitIfExpr(ifExpr)
    case indexExpr: IndexExpr           => visitIndexExpr(indexExpr)
    case literal: Literal               => visitLiteral(literal)
    case loopExpr: LoopExpr             => visitLoopExpr(loopExpr)
    case macroExpr: MacroExpr           => visitMacroExpr(macroExpr)
    case matchExpr: MatchExpr           => visitMatchExpr(matchExpr)
    case methodCallExpr: MethodCallExpr => visitMethodCallExpr(methodCallExpr)
    case offsetOfExpr: OffsetOfExpr     => visitOffsetOfExpr(offsetOfExpr)
    case expr: ParenExpr                => visitExpr(expr.expr)
    case pathExpr: PathExpr             => visitPathExpr(pathExpr)
    case prefixExpr: PrefixExpr         => visitPrefixExpr(prefixExpr)
    case rangeExpr: RangeExpr           => visitRangeExpr(rangeExpr)
    case recordExpr: RecordExpr         => visitRecordExpr(recordExpr)
    case refExpr: RefExpr               => visitRefExpr(refExpr)
    case returnExpr: ReturnExpr         => visitReturnExpr(returnExpr)
    case becomeExpr: BecomeExpr         => visitBecomeExpr(becomeExpr)
    case tryExpr: TryExpr               => visitTryExpr(tryExpr)
    case tupleExpr: TupleExpr           => visitTupleExpr(tupleExpr)
    case whileExpr: WhileExpr           => visitWhileExpr(whileExpr)
    case yieldExpr: YieldExpr           => visitYieldExpr(yieldExpr)
    case yeetExpr: YeetExpr             => visitYeetExpr(yeetExpr)
    case letExpr: LetExpr               => visitLetExpr(letExpr)
    case underscoreExpr: UnderscoreExpr => visitUnderscoreExpr(underscoreExpr)
  }

  private def visitType(typ: Type): Ast = {
    Ast(typeRefNode(typ, code(typ), typeFullNameForType(typ)))
  }

  // Module =
  //  Attr* Visibility?
  //  'mod' Name (ItemList | ';')
  private def visitModule(module: Module): Seq[Ast] = {
    module.itemList match {
      case None =>
        // It's a forward declaration for which we don't have an AST use.
        Nil
      case Some(itemList) =>
        // NB: two same-named `mod foo {}` declarations in the same file are not valid, e.g.
        // ```
        // mod foo {...}
        // ...
        // mod foo {...}
        // ```
        // So we don't need to disambiguate its occurrence like in other languages.
        val namespaceBlock = moduleNamespaceBlockNode(module)
        methodAstParentStack.push(namespaceBlock)
        val itemAsts = withLexicalTypeScope {
          withMacroDefinitionScope {
            itemList.item.foreach(bindImportsForItem)
            itemList.item.flatMap(visitItem)
          }
        }
        methodAstParentStack.pop()
        Ast(namespaceBlock).withChildren(itemAsts) :: Nil
    }
  }

  private case class RustUseImport(importedEntity: String, importedAs: String, isWildcard: Boolean)

  private def bindImportsForItem(item: Item): Unit = item match {
    case useItem: Use             => importsForUseTree(useItem.useTree).foreach(bindUseImport)
    case externCrate: ExternCrate => bindUseImport(importForExternCrate(externCrate))
    case _                        =>
  }

  private def bindUseImport(importInfo: RustUseImport): Unit = {
    if (importInfo.isWildcard) {
      bindWildcardImport(importInfo.importedEntity)
    } else {
      bindImportAlias(importInfo.importedAs, importInfo.importedEntity)
    }
  }

  // Use =
  //  Attr* Visibility? 'use' UseTree ';'
  private def visitUse(useItem: Use): Seq[Ast] = {
    val imports = importsForUseTree(useItem.useTree)
    imports.foreach(bindUseImport)
    imports.map { importInfo =>
      val importNode = newImportNode(code(useItem), importInfo.importedEntity, importInfo.importedAs, useItem)
        .isExplicit(true)
        .isWildcard(importInfo.isWildcard)
      Ast(importNode)
    }
  }

  private def importsForUseTree(useTree: UseTree, prefixParts: Seq[String] = Nil): Seq[RustUseImport] = {
    val currentParts = prefixParts ++ useTree.path.map(pathParts).getOrElse(Nil)

    useTree.useTreeList match {
      case Some(useTreeList) =>
        useTreeList.useTree.flatMap(importsForUseTree(_, currentParts))
      case None if useTree.starToken.isDefined =>
        Seq(RustUseImport(currentParts.mkString(RustFullNames.PathSep), "*", isWildcard = true))
      case None =>
        val importedEntityParts =
          if (currentParts.lastOption.contains("self")) currentParts.dropRight(1) else currentParts
        val importedEntity = importedEntityParts.mkString(RustFullNames.PathSep)
        val importedAs     = renamedImport(useTree).getOrElse(importedEntityParts.lastOption.getOrElse(importedEntity))
        Seq(RustUseImport(importedEntity, importedAs, isWildcard = false))
    }
  }

  private def renamedImport(useTree: UseTree): Option[String] = {
    useTree.rename.flatMap { rename =>
      rename.name.map(code).orElse(rename.underscoreToken.map(_ => "_"))
    }
  }

  // ExternCrate =
  //  Attr* Visibility? 'extern' 'crate' NameRef Rename? ';'
  private def visitExternCrate(externCrate: ExternCrate): Ast = {
    val importInfo = importForExternCrate(externCrate)
    bindUseImport(importInfo)
    val importNode = newImportNode(code(externCrate), importInfo.importedEntity, importInfo.importedAs, externCrate)
      .isExplicit(true)
      .isWildcard(false)
    Ast(importNode)
  }

  private def importForExternCrate(externCrate: ExternCrate): RustUseImport = {
    val importedEntity = code(externCrate.nameRef)
    val importedAs = externCrate.rename
      .flatMap(rename => rename.name.map(code).orElse(rename.underscoreToken.map(_ => "_")))
      .getOrElse(importedEntity)
    RustUseImport(importedEntity, importedAs, isWildcard = false)
  }

  private def pathParts(path: Path): Seq[String] = {
    path.path.map(pathParts).getOrElse(Nil) :+ code(path.pathSegment)
  }

  // ExternBlock =
  //  Attr* 'unsafe'? Abi ExternItemList
  private def visitExternBlock(externBlock: ExternBlock): Seq[Ast] = {
    externBlock.externItemList.externItem.flatMap(visitExternItem)
  }

  private def visitExternItem(externItem: ExternItem): Seq[Ast] = externItem match {
    case fn: Fn               => visitFn(fn) :: Nil
    case macroCall: MacroCall => visitMacroCall(macroCall) :: Nil
    case static: Static       => visitStatic(static)
    case typeAlias: TypeAlias => visitTypeAlias(typeAlias) :: Nil
  }

  private def visitAssocItem(assocItem: AssocItem): Seq[Ast] = assocItem match {
    case const: Const         => visitConst(const)
    case fn: Fn               => visitFn(fn) :: Nil
    case macroCall: MacroCall => visitMacroCall(macroCall) :: Nil
    case typeAlias: TypeAlias => visitTypeAlias(typeAlias) :: Nil
  }

  // Impl =
  //  Attr* Visibility? 'default'? 'unsafe'? 'impl' GenericParamList?
  //  'const'? '!'? Type ('for' Type)? WhereClause? AssocItemList
  private def visitImpl(implItem: Impl): Seq[Ast] = {
    implItem.typ.lastOption match {
      case Some(targetType) =>
        val targetName     = nameForImplTargetType(targetType)
        val targetFullName = Option(typeFullNameForType(targetType)).filter(_ != Defines.Any)
        val typeDecl       = typeDeclForNamedItem(implItem, targetName, fullName = targetFullName)

        methodAstParentStack.push(typeDecl)
        try {
          implItem.assocItemList.assocItem.flatMap(visitAssocItem)
        } finally {
          methodAstParentStack.pop()
        }
      case None =>
        notHandledYet(implItem) :: Nil
    }
  }

  private def nameForImplTargetType(typ: Type): String = typ match {
    case pathType: PathType =>
      pathType.path.pathSegment.nameRef.map(code).getOrElse(code(pathType.path.pathSegment))
    case other =>
      code(other)
  }

  // Const =
  //  Attr* Visibility?
  //  'default'?
  //  'const' (Name | '_') GenericParamList? ':' Type
  //  ('=' body:Expr)?
  //  WhereClause? ';'
  private def visitConst(const: Const): Seq[Ast] = {
    val typeFullName = typeFullNameForType(const.typ)
    const.name.flatMap(_.identToken) match {
      case Some(identToken) =>
        const.expr match {
          case Some(rhsExpr) => lowerIdentifierDecl(identToken, rhsExpr, typeFullName, code(const))
          case None =>
            val lhsName = code(identToken)
            bindLexicalType(lhsName, typeFullName)
            Seq(Ast(localNode(identToken, lhsName, lhsName, typeFullName)))
        }
      case None =>
        const.underscoreToken match {
          case Some(underscoreToken) => lowerAnonymousConst(underscoreToken, const.expr, typeFullName, code(const))
          case None                  => notHandledYet(const) :: Nil
        }
    }
  }

  private def lowerAnonymousConst(
    underscoreToken: UnderscoreToken,
    rhsExpr: Option[Expr],
    typeFullName: String,
    declCode: String
  ): Seq[Ast] = {
    val lhsName  = code(underscoreToken)
    val local    = localNode(underscoreToken, lhsName, lhsName, typeFullName)
    val localAst = Ast(local)
    val assignmentAst = rhsExpr.map { rhs =>
      val ident  = identifierNode(underscoreToken, lhsName, lhsName, typeFullName)
      val lhsAst = Ast(ident).withRefEdge(ident, local)
      val rhsAst = visitIdentifierDeclRhs(rhs, typeFullName)
      callAst(assignmentNode(underscoreToken, declCode, knownType(typeFullName)), Seq(lhsAst, rhsAst))
    }
    localAst +: assignmentAst.toSeq
  }

  // Static =
  //  Attr* Visibility? 'unsafe'? 'safe'? 'static' 'mut'? Name ':' Type ('=' Expr)? ';'
  private def visitStatic(static: Static): Seq[Ast] = {
    static.name.identToken match {
      case Some(identToken) =>
        val typeFullName = typeFullNameForType(static.typ)
        static.expr match {
          case Some(rhsExpr) => lowerIdentifierDecl(identToken, rhsExpr, typeFullName, code(static))
          case None =>
            val lhsName = code(identToken)
            bindLexicalType(lhsName, typeFullName)
            Seq(Ast(localNode(identToken, lhsName, lhsName, typeFullName)))
        }
      case None => notHandledYet(static) :: Nil
    }
  }

  // TypeAlias =
  //  Attr* Visibility? 'default'? 'type' Name GenericParamList? (':' TypeBoundList?)?
  //  WhereClause? ('=' Type)? ';'
  private def visitTypeAlias(typeAlias: TypeAlias): Ast = {
    val aliasTypeFullName = typeAlias.typ.map(typeFullNameForType)
    Ast(typeDeclForNamedItem(typeAlias, code(typeAlias.name), alias = aliasTypeFullName))
  }

  // LetStmt =
  //  Attr* 'super'? 'let' Pat (':' Type)?
  //  '=' initializer:Expr?
  //  LetElse?
  //  ';'
  private def visitLetStmt(letStmt: LetStmt): Seq[Ast] = {
    letStmt.pat match {
      case identPat: IdentPat =>
        identPat.name.identToken match {
          case Some(identToken) =>
            val typeFullName = typeFullNameForLetStmt(letStmt, identPat)
            letStmt.expr match {
              case Some(rhsExpr) => lowerIdentifierDecl(identToken, rhsExpr, typeFullName, code(letStmt))
              case None =>
                val lhsName = code(identToken)
                val local   = localNode(identToken, lhsName, lhsName, typeFullName)
                bindLexicalType(lhsName, typeFullName)
                Ast(local) :: Nil
            }
          case None => lowerPatternDecl(identPat, letStmt.expr, code(letStmt))
        }
      case pat => lowerPatternDecl(pat, letStmt.expr, code(letStmt))
    }
  }

  private def typeFullNameForLetStmt(letStmt: LetStmt, identPat: IdentPat): String = {
    letStmt.typ match {
      case Some(typ) =>
        val annotatedTypeFullName = typeFullNameForType(typ)
        Option
          .when(annotatedTypeFullName.contains("_")) {
            letStmt.expr.map(typeFullNameForExpr).filter(isConcreteInferredType)
          }
          .flatten
          .getOrElse(annotatedTypeFullName)
      case None =>
        letStmt.expr.flatMap(knownExprTypeFullName).getOrElse(typeFullNameForIdentPat(identPat))
    }
  }

  private def isConcreteInferredType(typeFullName: String): Boolean = {
    typeFullName != Defines.Any && !typeFullName.contains("_")
  }

  // Creates:
  // - LOCAL (lhsToken) with given typeFullName
  // - CALL (assignment) for lhsToken = rhsExpr
  private def lowerIdentifierDecl(
    lhsToken: IdentToken,
    rhsExpr: Expr,
    typeFullName: String,
    declCode: String
  ): Seq[Ast] = {
    val lhsName = code(lhsToken)

    val local = localNode(lhsToken, lhsName, code(lhsToken), typeFullName)
    val ident = identifierNode(lhsToken, lhsName, code(lhsToken), typeFullName)

    val lhsAst = Ast(ident).withRefEdge(ident, local)
    val rhsAst = visitIdentifierDeclRhs(rhsExpr, typeFullName)
    bindLexicalType(lhsName, typeFullName)
    val localAst      = Ast(local)
    val assignmentAst = callAst(assignmentNode(lhsToken, declCode, knownType(typeFullName)), Seq(lhsAst, rhsAst))

    Seq(localAst, assignmentAst)
  }

  private def lowerPatternDecl(pat: Pat, rhsExpr: Option[Expr], declCode: String): Seq[Ast] = {
    val rhsAst    = rhsExpr.map(visitExpr)
    val localAsts = localAstsForPattern(pat)
    val assignmentAsts = rhsExpr.map { rhs =>
      val lhsAst = Ast(literalNode(pat, code(pat), Defines.Any))
      callAst(assignmentNode(pat, declCode), Seq(lhsAst, rhsAst.getOrElse(visitExpr(rhs))))
    }.toSeq
    localAsts ++ assignmentAsts
  }

  private def localAstsForPattern(pat: Pat): Seq[Ast] = {
    uniquePatternBindings(pat).flatMap { identPat =>
      identPat.name.identToken.map { identToken =>
        val name         = code(identToken)
        val typeFullName = typeFullNameForIdentPat(identPat)
        bindLexicalType(name, typeFullName)
        Ast(localNode(identToken, name, name, typeFullName))
      }
    }
  }

  private def uniquePatternBindings(pat: Pat): Seq[IdentPat] = {
    patternBindings(pat)
      .foldLeft((Set.empty[String], Vector.empty[IdentPat])) { case ((seenNames, bindings), identPat) =>
        identPat.name.identToken.map(code) match {
          case Some(name) if !seenNames.contains(name) => (seenNames + name, bindings :+ identPat)
          case _                                       => (seenNames, bindings)
        }
      }
      ._2
  }

  private def patternBindings(pat: Pat): Seq[IdentPat] = pat match {
    case identPat: IdentPat =>
      identPat.name.identToken.map(_ => identPat).toSeq ++ identPat.pat.toSeq.flatMap(patternBindings)
    case tuplePat: TuplePat =>
      tuplePat.pat.flatMap(patternBindings)
    case tupleStructPat: TupleStructPat =>
      tupleStructPat.pat.flatMap(patternBindings)
    case recordPat: RecordPat =>
      recordPat.recordPatFieldList.recordPatField.flatMap(field => patternBindings(field.pat))
    case slicePat: SlicePat =>
      slicePat.pat.flatMap(patternBindings)
    case refPat: RefPat =>
      patternBindings(refPat.pat)
    case parenPat: ParenPat =>
      patternBindings(parenPat.pat)
    case derefPat: DerefPat =>
      patternBindings(derefPat.pat)
    case boxPat: BoxPat =>
      patternBindings(boxPat.pat)
    case orPat: OrPat =>
      orPat.pat.flatMap(patternBindings)
    case _ =>
      Nil
  }

  private def visitIdentifierDeclRhs(rhsExpr: Expr, typeFullName: String): Ast = {
    val rhsAst = visitExpr(rhsExpr)
    rhsExpr match {
      case _: MacroExpr if typeFullName != Defines.Any =>
        rhsAst.root match {
          case Some(call: NewCall) if call.typeFullName == Defines.Any => call.typeFullName(typeFullName)
          case _                                                       =>
        }
      case _ =>
    }
    rhsAst
  }

  // LetExpr =
  //  'let' Pat '=' Expr
  private def visitLetExpr(letExpr: LetExpr): Ast = {
    val typeFullName = Option(typeFullNameForExpr(letExpr))
      .filter(_ != Defines.Any)
      .getOrElse("bool")
    val callNode = operatorCallNode(letExpr, code(letExpr), RustOperators.matches, Some(typeFullName))
    val pattern  = Ast(literalNode(letExpr.pat, code(letExpr.pat), Defines.Any))
    val rhs      = visitExpr(letExpr.expr)
    callAst(callNode, Seq(pattern, rhs))
  }

  // Name =
  //  '#ident' | 'self'
  private def visitName(name: Name): Ast = {
    name.identToken match {
      case None             => notHandledYet(name)
      case Some(identToken) => Ast()
    }
  }

  private def visitLiteral(lit: Literal): Ast = {
    val typeFullName = typeFullNameForLiteral(lit)
    Ast(literalNode(lit, code(lit), typeFullName))
  }

  extension (lit: Literal) {
    protected def value: Option[RustToken] =
      lit.intNumberToken
        .orElse(lit.floatNumberToken)
        .orElse(lit.stringToken)
        .orElse(lit.byteStringToken)
        .orElse(lit.cStringToken)
        .orElse(lit.charToken)
        .orElse(lit.byteToken)
        .orElse(lit.trueKwToken)
        .orElse(lit.falseKwToken)
  }

  // Fn =
  // Attr* Visibility?
  // 'default'? 'const'? 'async'? 'gen'? 'unsafe'? 'safe'? Abi?
  // 'fn' Name GenericParamList? ParamList RetType? WhereClause?
  // (body:BlockExpr | ';')
  private def visitFn(fn: Fn): Ast = {
    val method          = methodNode(node = fn, name = code(fn.name))
    val retTypeFullName = fn.retType.map(_.typ).map(typeFullNameForType).getOrElse("()")
    val methodRet       = methodReturnNode(fn, retTypeFullName)
    val methodMods      = Seq[NewModifier]()

    methodAstParentStack.push(method)
    val (paramAsts, bodyAst) = withFreshLexicalTypeScope {
      val paramAsts = visitParamList(fn.paramList, allowAnonymousTypeParams = fn.blockExpr.isEmpty)
      val bodyAst   = fn.blockExpr.map(lowerFnBody).getOrElse(blockAst(blockNode(fn)))
      (paramAsts, bodyAst)
    }
    methodAstParentStack.pop()

    methodAst(method = method, parameters = paramAsts, body = bodyAst, methodReturn = methodRet, modifiers = methodMods)
  }

  // Creates:
  // BLOCK {
  //   <stmts>
  //   RETURN (expr) // if (expr) exists
  // }
  private def lowerFnBody(blockExpr: BlockExpr): Ast = {
    withLexicalTypeScope {
      withMacroDefinitionScope {
        val stmtAsts   = blockExpr.stmtList.stmt.flatMap(visitStmt)
        val retExprAst = blockExpr.stmtList.expr.map(lowerReturnExpr).toList
        Ast(blockNode(blockExpr)).withChildren(stmtAsts ++ retExprAst)
      }
    }
  }

  // Creates:
  // RETURN expr
  private def lowerReturnExpr(expr: Expr): Ast = {
    val exprAst = visitExpr(expr)
    val ret     = returnNode(expr, code(expr))
    returnAst(ret, Seq(exprAst))
  }

  // BlockExpr =
  //  Attr* Label? (TryBlockModifier | 'unsafe' | ('async' 'move'?) | ('gen' 'move'?) | 'const') StmtList
  private def visitBlockExpr(blockExpr: BlockExpr): Ast = {
    withLexicalTypeScope {
      withMacroDefinitionScope {
        val stmts = visitStmtList(blockExpr.stmtList)
        val block = blockNode(blockExpr)
        Ast(block).withChildren(stmts)
      }
    }
  }

  // ReturnExpr =
  //  Attr* 'return' Expr?
  private def visitReturnExpr(returnExpr: ReturnExpr): Ast = {
    val ret     = returnNode(returnExpr, code(returnExpr))
    val exprAst = returnExpr.expr.map(visitExpr)
    returnAst(ret, exprAst.toList)
  }

  // CallExpr =
  //  Attr* Expr ArgList
  private def visitCallExpr(callExpr: CallExpr): Ast = {
    viewExprAsPathSegments(callExpr.expr) match {
      case Some(segments) =>
        val name           = segments.last
        val methodFullName = methodFullNameForCallExpr(callExpr, segments)
        val typeFullName   = typeFullNameForExpr(callExpr)
        val dispatch       = DispatchTypes.STATIC_DISPATCH
        val call =
          callNode(callExpr, code(callExpr), name, methodFullName, dispatch, None, Some(typeFullName))
        val args = callExpr.argList.expr.map(visitExpr)
        callAst(call, args)
      case None =>
        val name         = code(callExpr.expr)
        val typeFullName = typeFullNameForExpr(callExpr)
        val call =
          callNode(
            callExpr,
            code(callExpr),
            name,
            Defines.DynamicCallUnknownFullName,
            DispatchTypes.DYNAMIC_DISPATCH,
            None,
            Some(typeFullName)
          )
        val calleeAst = visitExpr(callExpr.expr)
        val args      = callExpr.argList.expr.map(visitExpr)
        callAst(call, args, base = Some(calleeAst))
    }
  }

  private def visitMacroExpr(macroExpr: MacroExpr): Ast = {
    visitMacroCall(macroExpr.macroCall, Some(macroExpr))
  }

  private def visitMacroCall(macroCall: MacroCall, expressionNode: Option[MacroExpr] = None): Ast = {
    viewPathAsSegments(macroCall.path).filter(_.nonEmpty) match {
      case Some(segments) =>
        val anchor         = expressionNode.getOrElse(macroCall)
        val name           = s"${segments.last}!"
        val methodFullName = methodFullNameForMacroCall(segments)
        val expansion      = Option.when(segments.size == 1)(simpleMacroCallExpansion(segments.last, macroCall)).flatten
        val typeFullName = anchor.typeFullName
          .orElse(expansion.map(_.typeFullName).filter(_ != Defines.Any))
          .getOrElse(Defines.Any)
        val dispatch = DispatchTypes.STATIC_DISPATCH
        val call =
          callNode(anchor, code(anchor), name, methodFullName, dispatch, None, Some(typeFullName))
        callAst(call, macroArgumentAsts(macroCall.tokenTree) ++ expansion.map(_.ast).toSeq)
      case None => notHandledYet(macroCall)
    }
  }

  private def macroArgumentAsts(
    tokenTree: TokenTree,
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Seq[Ast] = {
    splitMacroArguments(tokenTree).flatMap(macroArgumentTokenAsts(_, substitution))
  }

  private def splitMacroArguments(tokenTree: TokenTree): Seq[Seq[RustNode]] = {
    splitMacroTokenGroups(stripOuterDelimiters(tokenTree.children.map(createRustNode)))
  }

  private def splitMacroTokenGroups(tokens: Seq[RustNode]): Seq[Seq[RustNode]] = {
    tokens.zipWithIndex
      .foldLeft(Vector(Vector.empty[RustNode])) {
        case (groups, (token: CommaToken, index)) if !isMacroRepetitionSeparator(tokens, index) =>
          groups :+ Vector.empty[RustNode]
        case (groups, (token, _)) =>
          groups.init :+ (groups.last :+ token)
      }
      .filter(_.nonEmpty)
  }

  private def isMacroRepetitionSeparator(tokens: Seq[RustNode], index: Int): Boolean = {
    tokens.lift(index - 2).exists(_.isInstanceOf[DollarToken]) &&
    tokens.lift(index - 1).exists(_.isInstanceOf[TokenTree]) &&
    tokens.lift(index + 1).exists(isMacroRepetitionOperator)
  }

  private def isMacroRepetitionOperator(node: RustNode): Boolean = {
    node.isInstanceOf[StarToken] || node.isInstanceOf[PlusToken] || node.isInstanceOf[QuestionToken]
  }

  private def macroArgumentTokenAsts(tokens: Seq[RustNode], substitution: MacroSubstitution): Seq[Ast] = {
    simpleExpansionsFromTokenGroup(tokens, substitution).map(_.ast)
  }

  private def stripOuterDelimiters(tokens: Seq[RustNode]): Seq[RustNode] = {
    tokens match {
      case opening +: rest if isOpeningDelimiter(opening) && rest.lastOption.exists(isClosingDelimiter) =>
        rest.dropRight(1)
      case other => other
    }
  }

  private def isOpeningDelimiter(node: RustNode): Boolean = {
    node.isInstanceOf[LParenToken] || node.isInstanceOf[LBrackToken] || node.isInstanceOf[LCurlyToken]
  }

  private def isClosingDelimiter(node: RustNode): Boolean = {
    node.isInstanceOf[RParenToken] || node.isInstanceOf[RBrackToken] || node.isInstanceOf[RCurlyToken]
  }

  private def macroLiteralTypeFullName(token: RustToken): Option[String] = token match {
    case _: IntNumberToken   => Some("i32")
    case _: FloatNumberToken => Some("f64")
    case _: StringToken      => Some("&str")
    case _: ByteStringToken  => Some("&[u8]")
    case _: CStringToken     => Some("&CStr")
    case _: CharToken        => Some("char")
    case _: ByteToken        => Some("u8")
    case _: TrueKwToken      => Some("bool")
    case _: FalseKwToken     => Some("bool")
    case _                   => None
  }

  // MacroRules =
  //  Attr* Visibility? 'macro_rules' '!' Name TokenTree
  private def visitMacroRules(macroRules: MacroRules): Ast = {
    val rules = simpleMacroRules(macroRules)
    bindMacroDefinition(code(macroRules.name), rules)
    visitMacroDefinition(macroRules, code(macroRules.name), simpleMacroExpansions(rules))
  }

  // MacroDef =
  //  Attr* Visibility? 'macro' Name TokenTree*
  private def visitMacroDef(macroDef: MacroDef): Ast = {
    val rules = simpleMacroDefRules(macroDef)
    bindMacroDefinition(code(macroDef.name), rules)
    visitMacroDefinition(macroDef, code(macroDef.name), simpleMacroExpansions(rules))
  }

  private case class SimpleMacroRule(patternTokens: Seq[RustNode], body: TokenTree)
  private case class SimpleMacroExpansion(
    node: RustNode,
    ast: Ast,
    typeFullName: String,
    sourceCode: String,
    childAsts: Seq[Ast] = Nil,
    statementLike: Boolean = false
  ) {
    def asts: Seq[Ast]           = if (childAsts.nonEmpty) childAsts else Seq(ast)
    def isStatementLike: Boolean = statementLike || childAsts.nonEmpty || typeFullName == "!"
  }
  private case class MacroSubstitution(single: Map[String, Seq[RustNode]], repeated: Map[String, Seq[Seq[RustNode]]])
  private case class SimpleMacroLetDeclaration(
    identToken: IdentToken,
    typeTokens: Seq[RustNode],
    rhsTokens: Seq[RustNode]
  )
  private case class SimpleMacroIfParts(
    conditionTokens: Seq[RustNode],
    thenTree: TokenTree,
    elseToken: Option[ElseKwToken],
    elseTree: Option[TokenTree]
  )

  private object MacroSubstitution {
    val empty: MacroSubstitution = MacroSubstitution(Map.empty, Map.empty)
  }

  private def visitMacroDefinition(node: RustNode, name: String, expansions: Seq[SimpleMacroExpansion]): Ast = {
    val macroName = s"$name!"
    val method    = methodNode(node, macroName)
    val methodRet = methodReturnNode(node, macroReturnTypeFullName(expansions))
    val bodyAst   = macroDefinitionBodyAst(node, expansions)
    methodAst(method = method, parameters = Nil, body = bodyAst, methodReturn = methodRet, modifiers = Nil)
  }

  private def macroReturnTypeFullName(expansions: Seq[SimpleMacroExpansion]): String = {
    expansions.map(_.typeFullName).distinct match {
      case Seq(typeFullName) => typeFullName
      case _                 => Defines.Any
    }
  }

  private def macroDefinitionBodyAst(node: RustNode, expansions: Seq[SimpleMacroExpansion]): Ast = {
    Ast(blockNode(node)).withChildren {
      expansions.map { expansion =>
        returnAst(returnNode(expansion.node, expansion.sourceCode), Seq(expansion.ast))
      }
    }
  }

  private def simpleMacroDefRules(macroDef: MacroDef): Seq[SimpleMacroRule] = {
    macroDef.tokenTree.lastOption.toSeq.map { body =>
      val patternTokens = macroDef.tokenTree
        .dropRight(1)
        .headOption
        .map(tokenTree => stripOuterDelimiters(tokenTree.children.map(createRustNode)))
        .getOrElse(Nil)
      SimpleMacroRule(patternTokens, body)
    }
  }

  private def simpleMacroRules(macroRules: MacroRules): Seq[SimpleMacroRule] = {
    val topLevelTokens = stripOuterDelimiters(macroRules.tokenTree.children.map(createRustNode))
    macroRuleArms(topLevelTokens)
  }

  private def simpleMacroExpansions(rules: Seq[SimpleMacroRule]): Seq[SimpleMacroExpansion] = {
    rules.flatMap(rule => simpleExpansionFromTokenTree(rule.body))
  }

  private def macroRuleArms(tokens: Seq[RustNode]): Seq[SimpleMacroRule] = {
    @tailrec
    def loop(remaining: Seq[RustNode], acc: Vector[SimpleMacroRule]): Vector[SimpleMacroRule] = {
      remaining.dropWhile(isMacroRuleSeparator) match {
        case Nil => acc
        case (pattern: TokenTree) +: rest =>
          macroRuleArrowWidth(rest) match {
            case Some(arrowWidth) =>
              rest.drop(arrowWidth) match {
                case (body: TokenTree) +: tail =>
                  val patternTokens = stripOuterDelimiters(pattern.children.map(createRustNode))
                  loop(tail, acc :+ SimpleMacroRule(patternTokens, body))
                case _ => Vector.empty
              }
            case None => Vector.empty
          }
        case _ => Vector.empty
      }
    }

    loop(tokens, Vector.empty)
  }

  private def isMacroRuleSeparator(node: RustNode): Boolean = {
    node.isInstanceOf[CommaToken] || node.isInstanceOf[SemicolonToken]
  }

  private def macroRuleArrowWidth(tokens: Seq[RustNode]): Option[Int] = {
    tokens match {
      case (_: FatArrowToken) +: _               => Some(1)
      case (_: EqToken) +: (_: RAngleToken) +: _ => Some(2)
      case _                                     => None
    }
  }

  private def simpleExpansionFromTokenTree(
    tokenTree: TokenTree,
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[SimpleMacroExpansion] = {
    simpleBlockExpansion(tokenTree, substitution).orElse {
      val bodyTokens =
        stripOuterDelimiters(tokenTree.children.map(createRustNode)).filterNot(_.isInstanceOf[SemicolonToken])
      simpleExpansionFromTokens(bodyTokens, substitution).orElse {
        bodyTokens match {
          case Seq(tokenTree: TokenTree) => simpleExpansionFromTokenTree(tokenTree, substitution)
          case _                         => None
        }
      }
    }
  }

  private def simpleBlockExpansion(
    tokenTree: TokenTree,
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    val tokens = tokenTree.children.map(createRustNode)
    Option
      .when(tokens.headOption.exists(_.isInstanceOf[LCurlyToken])) {
        withLexicalTypeScope {
          val bodyTokens = stripOuterDelimiters(tokens)
          val groups     = splitMacroStatementTokenGroups(bodyTokens)
          val expansions = groups.map(simpleExpansionFromTokens(_, substitution))
          Option
            .when(expansions.forall(_.isDefined)) {
              val concreteExpansions = expansions.flatten
              val isSingleNestedTokenTree = bodyTokens match {
                case Seq(_: TokenTree) => true
                case _                 => false
              }
              Option
                .when(
                  bodyTokens.exists(_.isInstanceOf[SemicolonToken]) ||
                    (!isSingleNestedTokenTree && concreteExpansions.exists(_.isStatementLike))
                ) {
                  val typeFullName =
                    if (concreteExpansions.lastOption.exists(_.typeFullName == "!")) "!"
                    else if (bodyTokens.lastOption.exists(_.isInstanceOf[SemicolonToken])) "()"
                    else concreteExpansions.lastOption.map(_.typeFullName).getOrElse(Defines.Any)
                  SimpleMacroExpansion(
                    tokenTree,
                    Ast(blockNode(tokenTree)).withChildren(concreteExpansions.flatMap(_.asts)),
                    typeFullName,
                    code(tokenTree)
                  )
                }
            }
            .flatten
        }
      }
      .flatten
  }

  private def splitMacroStatementTokenGroups(tokens: Seq[RustNode]): Seq[Seq[RustNode]] = {
    tokens.zipWithIndex
      .foldLeft(Vector(Vector.empty[RustNode])) {
        case (groups, (token: SemicolonToken, index)) if !isMacroRepetitionSeparator(tokens, index) =>
          groups :+ Vector.empty[RustNode]
        case (groups, (token, _)) =>
          groups.init :+ (groups.last :+ token)
      }
      .filter(_.nonEmpty)
  }

  private def simpleExpansionFromTokens(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[SimpleMacroExpansion] = {
    simpleIfStatementExpansion(tokens, substitution)
      .orElse(simpleReturnStatementExpansion(tokens, substitution))
      .orElse(simpleLetStatementExpansion(tokens, substitution))
      .orElse(substitutedMetavariableExpansion(tokens, substitution))
      .orElse {
        tokens match {
          case Seq(token: IdentToken)                      => Some(simpleIdentifierExpansion(token))
          case Seq(dollar: DollarToken, token: IdentToken) => Some(simpleMetavariableExpansion(dollar, token))
          case Seq(token: RustToken)                       => simpleLiteralExpansion(token)
          case Seq(tokenTree: TokenTree) =>
            simpleTupleExpansion(tokenTree, substitution)
              .orElse(simpleArrayExpansion(tokenTree, substitution))
              .orElse(simpleExpansionFromTokenTree(tokenTree, substitution))
          case _ =>
            simpleMacroRepetitionExpansion(tokens, substitution)
              .orElse(simpleCallExpressionExpansion(tokens, substitution))
              .orElse(simpleBinaryExpressionExpansion(tokens, substitution))
              .orElse(simplePrefixExpressionExpansion(tokens, substitution))
        }
      }
  }

  private def simpleIfStatementExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens match {
      case (ifToken: IfKwToken) +: tail =>
        simpleIfParts(tail).flatMap { parts =>
          for {
            condition     <- simpleExpansionFromTokens(parts.conditionTokens, substitution)
            thenExpansion <- simpleExpansionFromTokenTree(parts.thenTree, substitution)
            elseExpansion <- simpleOptionalElseExpansion(parts.elseTree, substitution)
          } yield {
            val sourceCode = codeForMacroTokens(tokens)
            val ifNode     = controlStructureNode(ifToken, ControlStructureTypes.IF, sourceCode)
            val thenAst    = simpleIfBranchAst(parts.thenTree, thenExpansion)
            val elseAst    = parts.elseToken.map(simpleElseExpansionAst(_, parts.elseTree, elseExpansion))
            SimpleMacroExpansion(
              ifToken,
              ifThenElseAst(ifNode, Some(condition.ast), thenAst, elseAst),
              simpleIfTypeFullName(thenExpansion, elseExpansion),
              sourceCode,
              statementLike = parts.elseTree.isEmpty
            )
          }
        }
      case _ => None
    }
  }

  private def simpleIfParts(tokens: Seq[RustNode]): Option[SimpleMacroIfParts] = {
    val thenIndex = tokens.indexWhere(isCurlyTokenTree)
    Option
      .when(thenIndex > 0) {
        val conditionTokens = tokens.take(thenIndex)
        val thenTree        = tokens(thenIndex).asInstanceOf[TokenTree]
        tokens.drop(thenIndex + 1) match {
          case Nil =>
            Some(SimpleMacroIfParts(conditionTokens, thenTree, None, None))
          case (elseToken: ElseKwToken) +: (elseTree: TokenTree) +: Nil if isCurlyTokenTree(elseTree) =>
            Some(SimpleMacroIfParts(conditionTokens, thenTree, Some(elseToken), Some(elseTree)))
          case _ =>
            None
        }
      }
      .flatten
  }

  private def simpleOptionalElseExpansion(
    elseTree: Option[TokenTree],
    substitution: MacroSubstitution
  ): Option[Option[SimpleMacroExpansion]] = {
    elseTree match {
      case Some(tokenTree) => simpleExpansionFromTokenTree(tokenTree, substitution).map(Some(_))
      case None            => Some(None)
    }
  }

  private def simpleIfBranchAst(branchTree: TokenTree, branchExpansion: SimpleMacroExpansion): Ast = {
    branchExpansion.ast.root match {
      case Some(_: NewBlock) => branchExpansion.ast
      case _                 => Ast(blockNode(branchTree)).withChildren(branchExpansion.asts)
    }
  }

  private def simpleElseExpansionAst(
    elseToken: ElseKwToken,
    elseTree: Option[TokenTree],
    elseExpansion: Option[SimpleMacroExpansion]
  ): Ast = {
    val elseNode = controlStructureNode(elseToken, ControlStructureTypes.ELSE, "else")
    val elseBody = for {
      tree      <- elseTree
      expansion <- elseExpansion
    } yield simpleIfBranchAst(tree, expansion)
    elseBody.map(body => Ast(elseNode).withChild(body)).getOrElse(Ast(elseNode))
  }

  private def simpleIfTypeFullName(
    thenExpansion: SimpleMacroExpansion,
    elseExpansion: Option[SimpleMacroExpansion]
  ): String = {
    elseExpansion match {
      case Some(elseExpansion) if thenExpansion.typeFullName == elseExpansion.typeFullName =>
        thenExpansion.typeFullName
      case Some(_) => Defines.Any
      case None    => "()"
    }
  }

  private def isCurlyTokenTree(node: RustNode): Boolean = node match {
    case tokenTree: TokenTree =>
      tokenTree.children.map(createRustNode).headOption.exists(_.isInstanceOf[LCurlyToken])
    case _ => false
  }

  private def simpleReturnStatementExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens match {
      case (returnToken: ReturnKwToken) +: exprTokens =>
        val sourceCode = codeForMacroTokens(tokens)
        val exprExpansion = Option
          .when(exprTokens.nonEmpty)(simpleExpansionFromTokens(exprTokens, substitution))
          .flatten
        Option.when(exprTokens.isEmpty || exprExpansion.isDefined) {
          SimpleMacroExpansion(
            returnToken,
            returnAst(returnNode(returnToken, sourceCode), exprExpansion.map(_.ast).toSeq),
            "!",
            sourceCode
          )
        }
      case _ => None
    }
  }

  private def simpleLetStatementExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    simpleLetDeclaration(tokens).flatMap { declaration =>
      simpleExpansionFromTokens(declaration.rhsTokens, substitution).map { rhs =>
        val name         = code(declaration.identToken)
        val typeFullName = macroLetTypeFullName(declaration.typeTokens, substitution, rhs.typeFullName)
        val local        = localNode(declaration.identToken, name, name, typeFullName)
        val ident        = identifierNode(declaration.identToken, name, name, typeFullName)
        val lhsAst       = Ast(ident).withRefEdge(ident, local)
        val assignmentAst = callAst(
          assignmentNode(declaration.identToken, codeForMacroTokens(tokens), knownType(typeFullName)),
          Seq(lhsAst, rhs.ast)
        )

        bindLexicalType(name, typeFullName)
        SimpleMacroExpansion(
          declaration.identToken,
          assignmentAst,
          "()",
          codeForMacroTokens(tokens),
          childAsts = Seq(Ast(local), assignmentAst)
        )
      }
    }
  }

  private def simpleLetDeclaration(tokens: Seq[RustNode]): Option[SimpleMacroLetDeclaration] = {
    val afterLet = tokens match {
      case (_: LetKwToken) +: (_: MutKwToken) +: tail => Some(tail)
      case (_: LetKwToken) +: tail                    => Some(tail)
      case _                                          => None
    }

    afterLet.flatMap {
      case (identToken: IdentToken) +: tail => simpleLetDeclarationTail(identToken, tail)
      case _                                => None
    }
  }

  private def simpleLetDeclarationTail(
    identToken: IdentToken,
    tail: Seq[RustNode]
  ): Option[SimpleMacroLetDeclaration] = {
    tail match {
      case (_: EqToken) +: rhsTokens if rhsTokens.nonEmpty =>
        Some(SimpleMacroLetDeclaration(identToken, Nil, rhsTokens))
      case (_: ColonToken) +: typedTail =>
        val assignmentIndex = typedTail.indexWhere(_.isInstanceOf[EqToken])
        Option.when(assignmentIndex > 0 && assignmentIndex < typedTail.size - 1) {
          SimpleMacroLetDeclaration(identToken, typedTail.take(assignmentIndex), typedTail.drop(assignmentIndex + 1))
        }
      case _ => None
    }
  }

  private def macroLetTypeFullName(
    typeTokens: Seq[RustNode],
    substitution: MacroSubstitution,
    rhsTypeFullName: String
  ): String = {
    if (typeTokens.isEmpty) {
      rhsTypeFullName
    } else {
      val annotatedType = codeForMacroTokens(substitutedMacroTypeTokens(typeTokens, substitution)).trim
      annotatedType match {
        case "" | "_"                                     => rhsTypeFullName
        case typeFullName if typeFullName.startsWith("$") => Defines.Any
        case typeFullName                                 => typeFullName
      }
    }
  }

  private def substitutedMacroTypeTokens(tokens: Seq[RustNode], substitution: MacroSubstitution): Seq[RustNode] = {
    tokens match {
      case Seq(_: DollarToken, identToken: IdentToken) => substitution.single.getOrElse(code(identToken), tokens)
      case _                                           => tokens
    }
  }

  private def simpleExpansionsFromTokenGroup(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Seq[SimpleMacroExpansion] = {
    simpleExpansionsFromTokenGroupOption(tokens, substitution).getOrElse(Nil)
  }

  private def simpleExpansionsFromTokenGroupOption(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[Seq[SimpleMacroExpansion]] = {
    simpleMacroRepetitionExpansions(tokens, substitution)
      .orElse(simpleExpansionFromTokens(tokens, substitution).map(Seq(_)))
  }

  private def simpleMacroCallExpansion(name: String, macroCall: MacroCall): Option[SimpleMacroExpansion] = {
    val argumentGroups = splitMacroArguments(macroCall.tokenTree)
    lookupMacroDefinition(name).flatMap { rules =>
      rules.iterator
        .map { rule =>
          macroSubstitutionForRule(rule, argumentGroups).flatMap(simpleExpansionFromTokenTree(rule.body, _))
        }
        .collectFirst { case Some(expansion) => expansion }
    }
  }

  private def macroSubstitutionForRule(
    rule: SimpleMacroRule,
    argumentGroups: Seq[Seq[RustNode]]
  ): Option[MacroSubstitution] = {
    if (rule.patternTokens.isEmpty) {
      Option.when(argumentGroups.isEmpty)(MacroSubstitution.empty)
    } else {
      macroPatternAlternatives(rule.patternTokens).iterator
        .map(bindMacroPatternAlternative(_, argumentGroups))
        .collectFirst { case Some(substitution) =>
          substitution
        }
    }
  }

  private def bindMacroPatternAlternative(
    patternTokens: Seq[RustNode],
    argumentGroups: Seq[Seq[RustNode]]
  ): Option[MacroSubstitution] = {
    bindMacroRepetitionPattern(patternTokens, argumentGroups).orElse {
      val patternGroups = splitMacroTokenGroups(patternTokens)
      Option
        .when(patternGroups.size == argumentGroups.size)(patternGroups.zip(argumentGroups))
        .flatMap { groups =>
          groups.foldLeft(Option(MacroSubstitution.empty)) {
            case (Some(substitution), (patternGroup, argumentGroup)) =>
              bindMacroPatternGroup(patternGroup, argumentGroup, substitution)
            case (None, _) => None
          }
        }
    }
  }

  private def macroPatternAlternatives(tokens: Seq[RustNode]): Seq[Seq[RustNode]] = {
    tokens match {
      case Nil => Seq(Nil)
      case (_: DollarToken) +: (tokenTree: TokenTree) +: (_: QuestionToken) +: tail =>
        val tailAlternatives = macroPatternAlternatives(tail)
        literalOptionalMacroPatternTokens(tokenTree) match {
          case Some(optionalTokens) =>
            tailAlternatives ++ tailAlternatives.map(optionalTokens ++ _)
          case None =>
            tailAlternatives.map(tokens.take(3) ++ _)
        }
      case head +: tail =>
        macroPatternAlternatives(tail).map(head +: _)
    }
  }

  private def literalOptionalMacroPatternTokens(tokenTree: TokenTree): Option[Seq[RustNode]] = {
    val bodyTokens = stripOuterDelimiters(tokenTree.children.map(createRustNode))
    Option.when(macroPatternVariables(bodyTokens).isEmpty && macroMetavariableNames(bodyTokens).isEmpty)(bodyTokens)
  }

  private def bindMacroRepetitionPattern(
    patternTokens: Seq[RustNode],
    argumentGroups: Seq[Seq[RustNode]]
  ): Option[MacroSubstitution] = {
    patternTokens match {
      case (_: DollarToken) +: (tokenTree: TokenTree) +: rest if rest.lastOption.exists(isMacroRepetitionOperator) =>
        val bodyTokens             = stripOuterDelimiters(tokenTree.children.map(createRustNode))
        val separatorTokens        = rest.dropRight(1)
        val repeatedArgumentGroups = splitMacroRepetitionArgumentGroups(argumentGroups, separatorTokens)
        val names                  = macroPatternVariables(bodyTokens).map(_.name).distinct
        val countMatchesOperator = rest.lastOption match {
          case Some(_: PlusToken)     => repeatedArgumentGroups.nonEmpty
          case Some(_: QuestionToken) => repeatedArgumentGroups.size <= 1
          case _                      => true
        }
        Option
          .when(names.nonEmpty && countMatchesOperator) {
            val substitutions =
              repeatedArgumentGroups.map(bindMacroPatternGroup(bodyTokens, _, MacroSubstitution.empty))
            Option.when(substitutions.forall(_.isDefined)) {
              val concreteSubstitutions = substitutions.flatten
              val repeated = names.map { name =>
                name -> concreteSubstitutions.flatMap(_.single.get(name))
              }.toMap
              MacroSubstitution(Map.empty, repeated)
            }
          }
          .flatten
      case _ => None
    }
  }

  private def splitMacroRepetitionArgumentGroups(
    argumentGroups: Seq[Seq[RustNode]],
    separatorTokens: Seq[RustNode]
  ): Seq[Seq[RustNode]] = {
    if (separatorTokens.isEmpty || separatorTokens.forall(_.isInstanceOf[CommaToken])) {
      argumentGroups
    } else {
      argumentGroups.flatMap(splitMacroTokensOnSeparator(_, separatorTokens))
    }
  }

  private def splitMacroTokensOnSeparator(tokens: Seq[RustNode], separatorTokens: Seq[RustNode]): Seq[Seq[RustNode]] = {
    @tailrec
    def loop(
      remaining: Seq[RustNode],
      current: Vector[RustNode],
      acc: Vector[Vector[RustNode]]
    ): Vector[Vector[RustNode]] = {
      remaining match {
        case Nil => acc :+ current
        case _ if macroTokensStartWith(remaining, separatorTokens) =>
          loop(remaining.drop(separatorTokens.size), Vector.empty, acc :+ current)
        case head +: tail =>
          loop(tail, current :+ head, acc)
      }
    }

    loop(tokens, Vector.empty, Vector.empty).filter(_.nonEmpty)
  }

  private def macroTokensStartWith(tokens: Seq[RustNode], prefix: Seq[RustNode]): Boolean = {
    prefix.nonEmpty &&
    tokens.sizeIs >= prefix.size &&
    macroTokensEquivalent(tokens.take(prefix.size), prefix)
  }

  private def bindMacroPatternGroup(
    patternTokens: Seq[RustNode],
    argumentTokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[MacroSubstitution] = {
    macroPatternVariables(patternTokens) match {
      case Nil =>
        Option.when(macroTokensEquivalent(patternTokens, argumentTokens))(substitution)
      case variable :: Nil =>
        bindSinglePatternVariable(variable, patternTokens, argumentTokens, substitution)
      case variables =>
        bindMultiplePatternVariables(variables, patternTokens, argumentTokens, substitution)
    }
  }

  private case class MacroPatternVariable(name: String, start: Int, end: Int)

  private def macroPatternVariables(tokens: Seq[RustNode]): Seq[MacroPatternVariable] = {
    @tailrec
    def loop(index: Int, acc: Vector[MacroPatternVariable]): Vector[MacroPatternVariable] = {
      tokens.lift(index) match {
        case None => acc
        case Some(_: DollarToken)
            if tokens.lift(index + 1).exists(_.isInstanceOf[IdentToken]) &&
              tokens.lift(index + 2).exists(_.isInstanceOf[ColonToken]) &&
              tokens.lift(index + 3).exists(isMacroFragmentSpecifier) =>
          val identToken = tokens(index + 1).asInstanceOf[IdentToken]
          loop(index + 4, acc :+ MacroPatternVariable(code(identToken), index, index + 4))
        case Some(_) => loop(index + 1, acc)
      }
    }

    loop(0, Vector.empty)
  }

  private def isMacroFragmentSpecifier(node: RustNode): Boolean = {
    node.isInstanceOf[IdentToken]
  }

  private def bindSinglePatternVariable(
    variable: MacroPatternVariable,
    patternTokens: Seq[RustNode],
    argumentTokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[MacroSubstitution] = {
    val prefix = patternTokens.take(variable.start)
    val suffix = patternTokens.drop(variable.end)
    Option
      .when(
        argumentTokens.size >= prefix.size + suffix.size &&
          macroTokensEquivalent(prefix, argumentTokens.take(prefix.size)) &&
          macroTokensEquivalent(suffix, argumentTokens.takeRight(suffix.size))
      ) {
        val valueTokens = argumentTokens.slice(prefix.size, argumentTokens.size - suffix.size)
        addSingleSubstitution(substitution, variable.name, valueTokens)
      }
      .flatten
  }

  private def bindMultiplePatternVariables(
    variables: Seq[MacroPatternVariable],
    patternTokens: Seq[RustNode],
    argumentTokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[MacroSubstitution] = {
    val prefix = patternTokens.take(variables.head.start)
    if (!macroTokensEquivalent(prefix, argumentTokens.take(prefix.size))) {
      None
    } else {
      val initial = Option((substitution, prefix.size))
      variables.zipWithIndex
        .foldLeft(initial) {
          case (Some((currentSubstitution, argumentStart)), (variable, index)) =>
            val nextVariable = variables.lift(index + 1)
            val separator = nextVariable match {
              case Some(next) => patternTokens.slice(variable.end, next.start)
              case None       => patternTokens.drop(variable.end)
            }
            val valueEnd = nextVariable match {
              case Some(_) => findMacroSeparator(argumentTokens, separator, argumentStart)
              case None =>
                Option.when(macroTokensEquivalent(separator, argumentTokens.takeRight(separator.size))) {
                  argumentTokens.size - separator.size
                }
            }
            valueEnd.flatMap { end =>
              val valueTokens = argumentTokens.slice(argumentStart, end)
              addSingleSubstitution(currentSubstitution, variable.name, valueTokens).map { nextSubstitution =>
                val nextArgumentStart = end + separator.size
                (nextSubstitution, nextArgumentStart)
              }
            }
          case (None, _) => None
        }
        .flatMap { case (substitution, consumed) =>
          Option.when(consumed == argumentTokens.size)(substitution)
        }
    }
  }

  private def findMacroSeparator(tokens: Seq[RustNode], separator: Seq[RustNode], from: Int): Option[Int] = {
    Option
      .when(separator.nonEmpty) {
        (from to (tokens.size - separator.size)).find { index =>
          macroTokensEquivalent(separator, tokens.slice(index, index + separator.size))
        }
      }
      .flatten
  }

  private def addSingleSubstitution(
    substitution: MacroSubstitution,
    name: String,
    tokens: Seq[RustNode]
  ): Option[MacroSubstitution] = {
    substitution.single.get(name) match {
      case Some(existing) if macroTokensEquivalent(existing, tokens) => Some(substitution)
      case Some(_)                                                   => None
      case None => Some(substitution.copy(single = substitution.single.updated(name, tokens)))
    }
  }

  private def substitutedMetavariableExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens match {
      case Seq(_: DollarToken, identToken: IdentToken) =>
        substitution.single.get(code(identToken)).flatMap(simpleExpansionFromTokens(_))
      case _ => None
    }
  }

  private def repeatedMacroSubstitutions(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[Seq[MacroSubstitution]] = {
    val names = macroMetavariableNames(tokens).filter(substitution.repeated.contains).distinct
    val sizes = names.map(name => substitution.repeated(name).size).distinct
    sizes match {
      case Seq(size) =>
        Some {
          (0 until size).map { index =>
            val single = names.flatMap { name =>
              substitution.repeated(name).lift(index).map(name -> _)
            }.toMap
            substitution.copy(single = substitution.single ++ single)
          }
        }
      case _ => None
    }
  }

  private def macroMetavariableNames(tokens: Seq[RustNode]): Seq[String] = {
    tokens
      .sliding(2)
      .collect { case Seq(_: DollarToken, identToken: IdentToken) =>
        code(identToken)
      }
      .toSeq
  }

  private def macroTokensEquivalent(left: Seq[RustNode], right: Seq[RustNode]): Boolean = {
    left.size == right.size && left.zip(right).forall { case (leftToken, rightToken) =>
      leftToken.getClass == rightToken.getClass && code(leftToken) == code(rightToken)
    }
  }

  private def simpleIdentifierExpansion(token: IdentToken): SimpleMacroExpansion = {
    val name         = code(token)
    val typeFullName = lookupLexicalType(name).getOrElse(Defines.Any)
    SimpleMacroExpansion(token, Ast(identifierNode(token, name, name, typeFullName)), typeFullName, name)
  }

  private def simpleLiteralExpansion(token: RustToken): Option[SimpleMacroExpansion] = {
    macroLiteralTypeFullName(token).map { typeFullName =>
      val sourceCode = code(token)
      SimpleMacroExpansion(token, Ast(literalNode(token, sourceCode, typeFullName)), typeFullName, sourceCode)
    }
  }

  private def simpleMetavariableExpansion(dollar: DollarToken, token: IdentToken): SimpleMacroExpansion = {
    val name         = code(token)
    val sourceCode   = codeForMacroTokens(Seq(dollar, token))
    val typeFullName = lookupLexicalType(name).getOrElse(Defines.Any)
    SimpleMacroExpansion(dollar, Ast(identifierNode(dollar, name, sourceCode, typeFullName)), typeFullName, sourceCode)
  }

  private def simpleMacroRepetitionExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    simpleMacroRepetitionExpansions(tokens, substitution).filter(_.size == 1).flatMap(_.headOption).map { expansion =>
      expansion.copy(node = tokens.head, typeFullName = Defines.Any, sourceCode = codeForMacroTokens(tokens))
    }
  }

  private def simpleMacroRepetitionExpansions(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[Seq[SimpleMacroExpansion]] = {
    tokens match {
      case (_: DollarToken) +: (tokenTree: TokenTree) +: rest if rest.lastOption.exists(isMacroRepetitionOperator) =>
        val bodyTokens =
          stripOuterDelimiters(tokenTree.children.map(createRustNode)).filterNot(_.isInstanceOf[SemicolonToken])
        repeatedMacroSubstitutions(bodyTokens, substitution)
          .map { repeatedSubstitutions =>
            val groups = splitMacroTokenGroups(bodyTokens)
            repeatedSubstitutions.flatMap { repeatedSubstitution =>
              groups.flatMap(simpleExpansionsFromTokenGroup(_, repeatedSubstitution))
            }
          }
          .orElse {
            val groups     = splitMacroTokenGroups(bodyTokens)
            val expansions = groups.map(simpleExpansionFromTokens(_, substitution))
            Option.when(expansions.nonEmpty && expansions.forall(_.isDefined)) {
              expansions.flatten.map(_.copy(typeFullName = Defines.Any))
            }
          }
      case _ => None
    }
  }

  private def simpleCallExpressionExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens.lastOption.collect { case tokenTree: TokenTree => tokenTree }.filter(isParenthesizedTokenTree).flatMap {
      argTree =>
        val calleeTokens = tokens.dropRight(1)
        macroPathSegments(calleeTokens).map { segments =>
          val sourceCode     = codeForMacroTokens(tokens)
          val name           = segments.last
          val methodFullName = methodFullNameForMacroTokenCall(segments)
          val typeFullName   = Defines.Any
          val call =
            callNode(argTree, sourceCode, name, methodFullName, DispatchTypes.STATIC_DISPATCH, None, Some(typeFullName))
          val args = macroArgumentAsts(argTree, substitution)
          SimpleMacroExpansion(argTree, callAst(call, args), typeFullName, sourceCode)
        }
    }
  }

  private def simpleBinaryExpressionExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    findBinaryOperator(tokens).flatMap { operator =>
      for {
        lhs <- simpleExpansionFromTokens(tokens.take(operator.index), substitution)
        rhs <- simpleExpansionFromTokens(tokens.drop(operator.index + operator.width), substitution)
      } yield {
        val sourceCode   = codeForMacroTokens(tokens)
        val typeFullName = binaryMacroExpressionTypeFullName(operator.name, lhs, rhs)
        val callNode     = operatorCallNode(operator.node, sourceCode, operator.name, Some(typeFullName))
        SimpleMacroExpansion(operator.node, callAst(callNode, Seq(lhs.ast, rhs.ast)), typeFullName, sourceCode)
      }
    }
  }

  private def simplePrefixExpressionExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens match {
      case (opToken: RustToken) +: operandTokens =>
        for {
          opName  <- prefixOperatorNameForToken(opToken)
          operand <- simpleExpansionFromTokens(operandTokens, substitution)
        } yield {
          val sourceCode   = codeForMacroTokens(tokens)
          val typeFullName = prefixMacroExpressionTypeFullName(opToken, operand.typeFullName)
          val callNode     = operatorCallNode(opToken, sourceCode, opName, Some(typeFullName))
          SimpleMacroExpansion(opToken, callAst(callNode, Seq(operand.ast)), typeFullName, sourceCode)
        }
      case _ => None
    }
  }

  private def simpleTupleExpansion(
    tokenTree: TokenTree,
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[SimpleMacroExpansion] = {
    val tokens = tokenTree.children.map(createRustNode)
    tokens.headOption.collect { case _: LParenToken => () }.flatMap { _ =>
      val bodyTokens = stripOuterDelimiters(tokens).filterNot(_.isInstanceOf[SemicolonToken])
      val groups     = splitMacroTokenGroups(bodyTokens)
      Option
        .when(groups.sizeIs > 1 || groups.exists(isMacroRepetitionTokens))(groups)
        .flatMap { groups =>
          val expansions = groups.map(simpleExpansionsFromTokenGroupOption(_, substitution))
          Option.when(expansions.forall(_.isDefined))(expansions.flatten.flatten)
        }
        .map { expansions =>
          val typeFullName =
            Option
              .when(groups.exists(isMacroRepetitionTokens) || expansions.exists(_.typeFullName == Defines.Any))(
                Defines.Any
              )
              .getOrElse(s"(${expansions.map(_.typeFullName).mkString(", ")})")
          val callNode = operatorCallNode(tokenTree, code(tokenTree), RustOperators.tupleLiteral, Some(typeFullName))
          SimpleMacroExpansion(tokenTree, callAst(callNode, expansions.map(_.ast)), typeFullName, code(tokenTree))
        }
    }
  }

  private def isMacroRepetitionTokens(tokens: Seq[RustNode]): Boolean = {
    tokens match {
      case (_: DollarToken) +: (_: TokenTree) +: rest => rest.lastOption.exists(isMacroRepetitionOperator)
      case _                                          => false
    }
  }

  private def simpleArrayExpansion(
    tokenTree: TokenTree,
    substitution: MacroSubstitution = MacroSubstitution.empty
  ): Option[SimpleMacroExpansion] = {
    val tokens = tokenTree.children.map(createRustNode)
    tokens.headOption.collect { case _: LBrackToken => () }.flatMap { _ =>
      val bodyTokens     = stripOuterDelimiters(tokens)
      val semicolonIndex = bodyTokens.indexWhere(_.isInstanceOf[SemicolonToken])
      if (semicolonIndex >= 0) {
        simpleRepeatArrayExpansion(tokenTree, bodyTokens, semicolonIndex, substitution)
      } else {
        simpleArrayInitializerExpansion(tokenTree, bodyTokens, substitution)
      }
    }
  }

  private def simpleArrayInitializerExpansion(
    tokenTree: TokenTree,
    bodyTokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    val groups = splitMacroTokenGroups(bodyTokens)
    val expansions = groups match {
      case Seq() => Some(Seq.empty[SimpleMacroExpansion])
      case _ =>
        val parsed = groups.map(simpleExpansionsFromTokenGroupOption(_, substitution))
        Option.when(parsed.forall(_.isDefined))(parsed.flatten.flatten)
    }

    expansions.map { expansions =>
      val typeFullName = homogeneousArrayTypeFullName(expansions)
      val callNode     = operatorCallNode(tokenTree, code(tokenTree), Operators.arrayInitializer, Some(typeFullName))
      SimpleMacroExpansion(tokenTree, callAst(callNode, expansions.map(_.ast)), typeFullName, code(tokenTree))
    }
  }

  private def simpleRepeatArrayExpansion(
    tokenTree: TokenTree,
    bodyTokens: Seq[RustNode],
    semicolonIndex: Int,
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    val valueTokens = bodyTokens.take(semicolonIndex)
    val countTokens = bodyTokens.drop(semicolonIndex + 1)
    for {
      value <- simpleExpansionFromTokens(valueTokens, substitution)
      count <- simpleArrayRepeatCountExpansion(countTokens, substitution)
    } yield {
      val typeFullName =
        Option
          .when(value.typeFullName != Defines.Any)(s"[${value.typeFullName}; ${codeForMacroTokens(countTokens)}]")
          .getOrElse(Defines.Any)
      val callNode = operatorCallNode(tokenTree, code(tokenTree), RustOperators.repeatInArray, Some(typeFullName))
      SimpleMacroExpansion(tokenTree, callAst(callNode, Seq(value.ast, count.ast)), typeFullName, code(tokenTree))
    }
  }

  private def simpleArrayRepeatCountExpansion(
    tokens: Seq[RustNode],
    substitution: MacroSubstitution
  ): Option[SimpleMacroExpansion] = {
    tokens match {
      case Seq(token: IntNumberToken) =>
        val sourceCode = code(token)
        Some(SimpleMacroExpansion(token, Ast(literalNode(token, sourceCode, "usize")), "usize", sourceCode))
      case _ =>
        simpleExpansionFromTokens(tokens, substitution)
    }
  }

  private def homogeneousArrayTypeFullName(expansions: Seq[SimpleMacroExpansion]): String = {
    val elementTypes = expansions.map(_.typeFullName).distinct
    elementTypes match {
      case Seq(elementType) if elementType != Defines.Any => s"[$elementType; ${expansions.size}]"
      case _                                              => Defines.Any
    }
  }

  private def codeForMacroTokens(tokens: Seq[RustNode]): String = {
    (for {
      start <- tokens.flatMap(_.startOffset).minOption
      end   <- tokens.flatMap(_.endOffset).maxOption
      if start <= end
    } yield String(parseResult.contentBytes.slice(start, end), java.nio.charset.StandardCharsets.UTF_8))
      .getOrElse(tokens.map(code).mkString(" "))
  }

  private def isParenthesizedTokenTree(tokenTree: TokenTree): Boolean = {
    tokenTree.children.map(createRustNode).headOption.exists(_.isInstanceOf[LParenToken])
  }

  private def macroPathSegments(tokens: Seq[RustNode]): Option[Seq[String]] = {
    @tailrec
    def loop(remaining: Seq[RustNode], expectingName: Boolean, acc: Vector[String]): Option[Vector[String]] = {
      remaining match {
        case Nil => Option.when(!expectingName && acc.nonEmpty)(acc)
        case head +: tail if expectingName =>
          nameForMacroPathToken(head) match {
            case Some(name) => loop(tail, expectingName = false, acc :+ name)
            case None       => None
          }
        case (_: Colon2Token) +: tail                   => loop(tail, expectingName = true, acc)
        case (_: ColonToken) +: (_: ColonToken) +: tail => loop(tail, expectingName = true, acc)
        case _                                          => None
      }
    }

    loop(normalizeMacroPathTokens(tokens), expectingName = true, Vector.empty).map(_.toSeq)
  }

  private def normalizeMacroPathTokens(tokens: Seq[RustNode]): Seq[RustNode] = {
    tokens match {
      case (_: DollarToken) +: (crateToken: CrateKwToken) +: rest => crateToken +: rest
      case (_: DollarToken) +: (crateToken: IdentToken) +: rest if code(crateToken) == "crate" =>
        crateToken +: rest
      case other => other
    }
  }

  private def nameForMacroPathToken(token: RustNode): Option[String] = token match {
    case token: IdentToken      => Some(code(token))
    case token: SelfKwToken     => Some(code(token))
    case token: SuperKwToken    => Some(code(token))
    case token: CrateKwToken    => Some(code(token))
    case token: SelfTypeKwToken => Some(code(token))
    case _                      => None
  }

  private def methodFullNameForMacroTokenCall(segments: Seq[String]): String = {
    importedMacroTokenFullName(segments).getOrElse {
      segments match {
        case name :: Nil => combineRustFullName(Defines.UnresolvedNamespace, name)
        case names       => names.mkString(RustFullNames.PathSep)
      }
    }
  }

  private def importedMacroTokenFullName(segments: Seq[String]): Option[String] = {
    segments match {
      case alias +: rest =>
        lookupImportAlias(alias)
          .map(importedEntity => (importedEntity +: rest).mkString(RustFullNames.PathSep))
          .orElse(lookupWildcardImport(segments))
      case _ => None
    }
  }

  private case class MacroBinaryOperator(index: Int, width: Int, name: String, precedence: Int, node: RustNode)

  private def findBinaryOperator(tokens: Seq[RustNode]): Option[MacroBinaryOperator] = {
    val twoTokenOperators = tokens
      .sliding(2)
      .zipWithIndex
      .flatMap { case (pair, index) =>
        pair match {
          case Seq(first, second) if index > 0 && index + 2 < tokens.size =>
            compositeBinaryOperatorName(first, second)
              .flatMap(name => binaryOperatorPrecedence(name).map(MacroBinaryOperator(index, 2, name, _, first)))
          case _ => None
        }
      }
      .toSeq
    val compositeTokenIndexes =
      twoTokenOperators.flatMap(operator => operator.index until operator.index + operator.width).toSet
    val singleTokenOperators = tokens.zipWithIndex.flatMap { case (token, index) =>
      Option
        .when(index > 0 && index < tokens.size - 1 && !compositeTokenIndexes.contains(index))(token)
        .flatMap(binaryOperatorNameForToken)
        .flatMap(name => binaryOperatorPrecedence(name).map(MacroBinaryOperator(index, 1, name, _, token)))
    }

    (singleTokenOperators ++ twoTokenOperators)
      .sortBy(operator => (operator.precedence, -operator.index, -operator.width))
      .headOption
  }

  private def binaryOperatorNameForToken(token: RustNode): Option[String] = token match {
    case _: Pipe2Token     => Some(Operators.logicalOr)
    case _: Amp2Token      => Some(Operators.logicalAnd)
    case _: Eq2Token       => Some(Operators.equals)
    case _: NeqToken       => Some(Operators.notEquals)
    case _: LteqToken      => Some(Operators.lessEqualsThan)
    case _: GteqToken      => Some(Operators.greaterEqualsThan)
    case _: LAngleToken    => Some(Operators.lessThan)
    case _: RAngleToken    => Some(Operators.greaterThan)
    case _: PlusToken      => Some(Operators.addition)
    case _: StarToken      => Some(Operators.multiplication)
    case _: MinusToken     => Some(Operators.subtraction)
    case _: SlashToken     => Some(Operators.division)
    case _: PercentToken   => Some(Operators.modulo)
    case _: ShlToken       => Some(Operators.shiftLeft)
    case _: ShrToken       => Some(Operators.arithmeticShiftRight)
    case _: CaretToken     => Some(Operators.xor)
    case _: PipeToken      => Some(Operators.or)
    case _: AmpToken       => Some(Operators.and)
    case _: EqToken        => Some(Operators.assignment)
    case _: PluseqToken    => Some(Operators.assignmentPlus)
    case _: SlasheqToken   => Some(Operators.assignmentDivision)
    case _: StareqToken    => Some(Operators.assignmentMultiplication)
    case _: PercenteqToken => Some(Operators.assignmentModulo)
    case _: ShreqToken     => Some(Operators.assignmentArithmeticShiftRight)
    case _: ShleqToken     => Some(Operators.assignmentShiftLeft)
    case _: MinuseqToken   => Some(Operators.assignmentMinus)
    case _: PipeeqToken    => Some(Operators.assignmentOr)
    case _: AmpeqToken     => Some(Operators.assignmentAnd)
    case _: CareteqToken   => Some(Operators.assignmentXor)
    case _                 => None
  }

  private def compositeBinaryOperatorName(first: RustNode, second: RustNode): Option[String] = {
    (first, second) match {
      case (_: PipeToken, _: PipeToken)     => Some(Operators.logicalOr)
      case (_: AmpToken, _: AmpToken)       => Some(Operators.logicalAnd)
      case (_: EqToken, _: EqToken)         => Some(Operators.equals)
      case (_: BangToken, _: EqToken)       => Some(Operators.notEquals)
      case (_: LAngleToken, _: EqToken)     => Some(Operators.lessEqualsThan)
      case (_: RAngleToken, _: EqToken)     => Some(Operators.greaterEqualsThan)
      case (_: LAngleToken, _: LAngleToken) => Some(Operators.shiftLeft)
      case (_: RAngleToken, _: RAngleToken) => Some(Operators.arithmeticShiftRight)
      case (_: PlusToken, _: EqToken)       => Some(Operators.assignmentPlus)
      case (_: SlashToken, _: EqToken)      => Some(Operators.assignmentDivision)
      case (_: StarToken, _: EqToken)       => Some(Operators.assignmentMultiplication)
      case (_: PercentToken, _: EqToken)    => Some(Operators.assignmentModulo)
      case (_: MinusToken, _: EqToken)      => Some(Operators.assignmentMinus)
      case (_: PipeToken, _: EqToken)       => Some(Operators.assignmentOr)
      case (_: AmpToken, _: EqToken)        => Some(Operators.assignmentAnd)
      case (_: CaretToken, _: EqToken)      => Some(Operators.assignmentXor)
      case _                                => None
    }
  }

  private def binaryOperatorPrecedence(operatorName: String): Option[Int] = {
    operatorName match {
      case Operators.assignment | Operators.assignmentPlus | Operators.assignmentDivision |
          Operators.assignmentMultiplication | Operators.assignmentModulo | Operators.assignmentArithmeticShiftRight |
          Operators.assignmentShiftLeft | Operators.assignmentMinus | Operators.assignmentOr | Operators.assignmentAnd |
          Operators.assignmentXor =>
        Some(1)
      case Operators.logicalOr  => Some(2)
      case Operators.logicalAnd => Some(3)
      case Operators.equals | Operators.notEquals | Operators.lessEqualsThan | Operators.greaterEqualsThan |
          Operators.lessThan | Operators.greaterThan =>
        Some(4)
      case Operators.or                                         => Some(5)
      case Operators.xor                                        => Some(6)
      case Operators.and                                        => Some(7)
      case Operators.shiftLeft | Operators.arithmeticShiftRight => Some(8)
      case Operators.addition | Operators.subtraction           => Some(9)
      case Operators.multiplication | Operators.division | Operators.modulo =>
        Some(10)
      case _ => None
    }
  }

  private def prefixOperatorNameForToken(token: RustNode): Option[String] = token match {
    case _: MinusToken => Some(Operators.minus)
    case _: BangToken  => Some(Operators.logicalNot)
    case _: StarToken  => Some(Operators.indirection)
    case _             => None
  }

  private def binaryMacroExpressionTypeFullName(
    operatorName: String,
    lhs: SimpleMacroExpansion,
    rhs: SimpleMacroExpansion
  ): String = {
    operatorName match {
      case Operators.logicalOr | Operators.logicalAnd | Operators.equals | Operators.notEquals |
          Operators.lessEqualsThan | Operators.greaterEqualsThan | Operators.lessThan | Operators.greaterThan =>
        "bool"
      case Operators.assignment | Operators.assignmentPlus | Operators.assignmentDivision |
          Operators.assignmentMultiplication | Operators.assignmentModulo | Operators.assignmentArithmeticShiftRight |
          Operators.assignmentShiftLeft | Operators.assignmentMinus | Operators.assignmentOr | Operators.assignmentAnd |
          Operators.assignmentXor =>
        lhs.typeFullName
      case _ if lhs.typeFullName == rhs.typeFullName && lhs.typeFullName != Defines.Any =>
        lhs.typeFullName
      case _ => Defines.Any
    }
  }

  private def prefixMacroExpressionTypeFullName(token: RustNode, operandTypeFullName: String): String = {
    token match {
      case _: BangToken => "bool"
      case _: MinusToken =>
        Option.when(operandTypeFullName != Defines.Any)(operandTypeFullName).getOrElse(Defines.Any)
      case _: StarToken =>
        dereferencedTypeFullName(operandTypeFullName).getOrElse(Defines.Any)
      case _ => Defines.Any
    }
  }

  private def dereferencedTypeFullName(typeFullName: String): Option[String] = {
    typeFullName match {
      case t if t.startsWith("&mut ")   => Some(t.stripPrefix("&mut "))
      case t if t.startsWith("&")       => Some(t.stripPrefix("&"))
      case t if t.startsWith("*const ") => Some(t.stripPrefix("*const "))
      case t if t.startsWith("*mut ")   => Some(t.stripPrefix("*mut "))
      case _                            => None
    }
  }

  // FormatArgsExpr =
  //  'builtin' '#' 'format_args' '(' Expr (',' FormatArgsArg)* ','? ')'
  private def visitFormatArgsExpr(formatArgsExpr: FormatArgsExpr): Ast = {
    val name           = "format_args!"
    val methodFullName = combineRustFullName(Defines.UnresolvedNamespace, name)
    val typeFullName   = typeFullNameForExpr(formatArgsExpr)
    val call =
      callNode(
        formatArgsExpr,
        code(formatArgsExpr),
        name,
        methodFullName,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(typeFullName)
      )
    val args = visitExpr(formatArgsExpr.expr) +: formatArgsExpr.formatArgsArg.map(arg => visitExpr(arg.expr))
    callAst(call, args)
  }

  // OffsetOfExpr =
  //  'builtin' '#' 'offset_of' '(' Type ',' NameRef ('.' NameRef)* ')'
  private def visitOffsetOfExpr(offsetOfExpr: OffsetOfExpr): Ast = {
    val name           = "offset_of!"
    val methodFullName = combineRustFullName(Defines.UnresolvedNamespace, name)
    val typeFullName = Option(typeFullNameForExpr(offsetOfExpr))
      .filter(_ != Defines.Any)
      .getOrElse("usize")
    val call =
      callNode(
        offsetOfExpr,
        code(offsetOfExpr),
        name,
        methodFullName,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Some(typeFullName)
      )
    val targetTypeAst = Ast(
      typeRefNode(offsetOfExpr.typ, code(offsetOfExpr.typ), typeFullNameForType(offsetOfExpr.typ))
    )
    val fieldAsts = offsetOfExpr.nameRef.map(field => Ast(fieldIdentifierNode(field, code(field), code(field))))
    callAst(call, targetTypeAst +: fieldAsts)
  }

  // AsmExpr =
  //  'builtin' '#' ('asm' | 'global_asm' | 'naked_asm') '(' Expr (',' AsmPiece)* ','? ')'
  private def visitAsmExpr(asmExpr: AsmExpr): Ast = {
    val name           = asmExpr.name
    val methodFullName = combineRustFullName(Defines.UnresolvedNamespace, name)
    val typeFullName   = typeFullNameForExpr(asmExpr)
    val call =
      callNode(asmExpr, code(asmExpr), name, methodFullName, DispatchTypes.STATIC_DISPATCH, None, Some(typeFullName))
    val templateAsts = asmExpr.expr.map(visitExpr)
    val operandAsts  = asmExpr.asmPiece.flatMap(visitAsmPiece)
    callAst(call, templateAsts ++ operandAsts)
  }

  extension (asmExpr: AsmExpr) {
    protected def name: String = {
      if (asmExpr.globalAsmKwToken.isDefined) {
        "global_asm!"
      } else if (asmExpr.nakedAsmKwToken.isDefined) {
        "naked_asm!"
      } else {
        "asm!"
      }
    }
  }

  private def visitAsmPiece(asmPiece: AsmPiece): Seq[Ast] = asmPiece match {
    case operand: AsmOperandNamed => visitAsmOperand(operand.asmOperand)
    case _: AsmClobberAbi         => Nil
    case _: AsmOptions            => Nil
  }

  private def visitAsmOperand(asmOperand: AsmOperand): Seq[Ast] = asmOperand match {
    case const: AsmConst           => Seq(visitExpr(const.expr))
    case label: AsmLabel           => Seq(visitBlockExpr(label.blockExpr))
    case regOperand: AsmRegOperand => regOperand.asmOperandExpr.expr.map(visitExpr)
    case sym: AsmSym               => Seq(pathIdentifierAst(sym.path))
  }

  private def pathIdentifierAst(path: Path): Ast = {
    val name = viewPathAsSegments(path).flatMap(_.lastOption).getOrElse(code(path.pathSegment))
    Ast(identifierNode(path, name, code(path), typeFullNameForPath(path)))
  }

  // UnderscoreExpr =
  //  '_'
  private def visitUnderscoreExpr(underscoreExpr: UnderscoreExpr): Ast = {
    Ast(literalNode(underscoreExpr, code(underscoreExpr), Defines.Any))
  }

  private def viewExprAsPathSegments(expr: Expr): Option[Seq[String]] = expr match {
    case pathExpr: PathExpr => viewPathAsSegments(pathExpr.path)
    case _                  => None
  }

  private def knownType(typeFullName: String): Option[String] = {
    Option(typeFullName).filter(_ != Defines.Any)
  }

  private def viewPathAsSegments(path: Path): Option[Seq[String]] = {
    path.pathSegment.segmentName.map { segment =>
      path.path match {
        case Some(qualifier) => viewPathAsSegments(qualifier).getOrElse(Nil) :+ segment
        case None            => segment :: Nil
      }
    }
  }

  // ArgList =
  //  '(' args:(Expr (',' Expr)* ','?)? ')'
  private def visitArgList(argList: ArgList): Ast = {
    notHandledYet(argList)
  }

  // StmtList =
  //  '{'
  //    Attr*
  //    statements:Stmt*
  //    tail_expr:Expr?
  //  '}'
  private def visitStmtList(stmtList: StmtList): Seq[Ast] = {
    val stmtAsts    = stmtList.stmt.flatMap(visitStmt)
    val tailExprAst = stmtList.expr.map(visitExpr).toList
    stmtAsts ++ tailExprAst
  }

  // ParamList =
  //  '('(
  //    SelfParam
  //  | (SelfParam ',')? (Param (',' Param)* ','?)?
  //  )')'
  // | '|' (Param (',' Param)* ','?)? '|'
  private def visitParamList(paramList: ParamList, allowAnonymousTypeParams: Boolean = false): Seq[Ast] = {
    val receiverAsts = paramList.selfParam.map(visitSelfParam).toSeq
    receiverAsts ++ visitParamSeq(paramList.param, receiverAsts.size + 1, allowAnonymousTypeParams)
  }

  // ClosureExpr =
  //  Attr* 'static'? 'async'? 'move'? ParamList RetType? Expr
  private def visitClosureExpr(closureExpr: ClosureExpr): Ast = {
    val closureName = nextClosureName()
    val method      = methodNode(node = closureExpr, name = closureName)

    val retTypeFullName = closureExpr.retType.map(_.typ).map(typeFullNameForType).getOrElse {
      typeFullNameForExpr(closureExpr.expr)
    }
    val methodRet  = methodReturnNode(closureExpr, retTypeFullName)
    val methodMods = Seq(modifierNode(closureExpr, ModifierTypes.LAMBDA))

    methodAstParentStack.push(method)
    val (paramAsts, bodyAst) = withLexicalTypeScope {
      withMacroDefinitionScope {
        val paramAsts = visitClosureParamList(closureExpr.paramList)
        val bodyAst   = lowerClosureBody(closureExpr)
        (paramAsts, bodyAst)
      }
    }
    methodAstParentStack.pop()

    Ast.storeInDiffGraph(
      methodAst(
        method = method,
        parameters = paramAsts,
        body = bodyAst,
        methodReturn = methodRet,
        modifiers = methodMods
      ),
      diffGraph
    )

    val methodRefTypeFullName = Option
      .when(typeFullNameForExpr(closureExpr) != Defines.Any) {
        typeFullNameForExpr(closureExpr)
      }
      .getOrElse(method.fullName)
    Ast(methodRefNode(closureExpr, code(closureExpr), method.fullName, methodRefTypeFullName))
  }

  private def visitClosureParamList(paramList: ParamList): Seq[Ast] = {
    visitParamSeq(paramList.param)
  }

  private def visitSelfParam(selfParam: SelfParam): Ast = {
    parameterAst(selfParam, code(selfParam.name), code(selfParam), 1, typeFullNameForSelfParam(selfParam))
  }

  private def typeFullNameForSelfParam(selfParam: SelfParam): String = {
    selfParam.typeFullName
      .orElse(selfParam.typ.map(typeFullNameForType))
      .getOrElse {
        if (selfParam.ampToken.isDefined) {
          val mut = Option.when(selfParam.mutKwToken.isDefined)("mut ").getOrElse("")
          s"&${mut}Self"
        } else {
          "Self"
        }
      }
  }

  private def visitParamSeq(
    params: Seq[Param],
    startIndex: Int = 1,
    allowAnonymousTypeParams: Boolean = false
  ): Seq[Ast] = {
    params
      .foldLeft((startIndex, Vector.empty[Ast])) { case ((nextIndex, acc), param) =>
        val paramAsts = visitParam(param, nextIndex, allowAnonymousTypeParams)
        (nextIndex + paramAsts.size, acc ++ paramAsts)
      }
      ._2
  }

  private def visitParam(param: Param, startIndex: Int, allowAnonymousTypeParams: Boolean): Seq[Ast] = {
    val explicitTypeFullName = param.typ.map(typeFullNameForType)
    if (param.dot3Token.isDefined) {
      val name = s"<param>$startIndex"
      Seq(parameterAst(param, name, s"$name...", startIndex, Defines.Any, isVariadic = true))
    } else {
      param.pat match {
        case Some(identPat: IdentPat) if identPat.pat.isEmpty =>
          identPat.name.identToken match {
            case Some(identToken) =>
              if (allowAnonymousTypeParams && param.colonToken.isEmpty && explicitTypeFullName.isEmpty) {
                val name = s"<param>$startIndex"
                Seq(parameterAst(param, name, code(param), startIndex, code(identToken)))
              } else {
                val typeFullName = explicitTypeFullName.getOrElse(typeFullNameForIdentPat(identPat))
                Seq(parameterAst(param, code(identToken), code(param), startIndex, typeFullName))
              }
            case None =>
              Nil
          }
        case Some(wildcardPat: WildcardPat) =>
          Seq(parameterAst(wildcardPat, "_", code(param), startIndex, explicitTypeFullName.getOrElse(Defines.Any)))
        case Some(pat) =>
          uniquePatternBindings(pat).zipWithIndex.flatMap { case (identPat, idx) =>
            identPat.name.identToken.map { identToken =>
              val typeFullName = Option(typeFullNameForIdentPat(identPat))
                .filter(_ != Defines.Any)
                .getOrElse(Defines.Any)
              parameterAst(identPat, code(identToken), code(identPat), startIndex + idx, typeFullName)
            }
          }
        case None =>
          param.typ.map { typ =>
            val name = s"<param>$startIndex"
            parameterAst(typ, name, code(param), startIndex, explicitTypeFullName.getOrElse(Defines.Any))
          }.toSeq
      }
    }
  }

  private def parameterAst(
    node: RustNode,
    name: String,
    paramCode: String,
    index: Int,
    typeFullName: String,
    isVariadic: Boolean = false
  ): Ast = {
    val paramNode = parameterInNode(
      node = node,
      name = name,
      code = paramCode,
      index = index,
      isVariadic = isVariadic,
      evaluationStrategy = EvaluationStrategies.BY_SHARING,
      typeFullName = typeFullName
    )
    bindLexicalType(name, typeFullName)
    Ast(paramNode)
  }

  private def lowerClosureBody(closureExpr: ClosureExpr): Ast = {
    closureExpr.expr match {
      case blockExpr: BlockExpr => lowerFnBody(blockExpr)
      case expr                 => Ast(blockNode(closureExpr)).withChild(lowerReturnExpr(expr))
    }
  }

  // Param =
  //  Attr* (
  //    Pat (':' Type)?
  //  | Type
  //  | '...'
  //  )
  private def visitParam(param: Param): Ast = {
    visitParam(param, 1, allowAnonymousTypeParams = false).headOption.getOrElse(notHandledYet(param))
  }

  // PathExpr =
  //  Attr* Path
  private def visitPathExpr(pathExpr: PathExpr): Ast = {
    visitPath(pathExpr.path)
  }

  // Path =
  //  (qualifier:Path '::')? segment:PathSegment
  private def visitPath(path: Path): Ast = {
    lowerPathAsFieldAccess(path)
  }

  private def lowerPathAsFieldAccess(path: Path): Ast = {
    val lhs = path.path.map(lowerPathAsFieldAccess)

    val name         = code(path.pathSegment)
    val typeFullName = typeFullNameForPath(path)

    lhs match {
      case None      => Ast(identifierNode(path.pathSegment, name, code(path), typeFullName))
      case Some(lhs) => fieldAccessAst(path, path.pathSegment, lhs, code(path), name, typeFullName)
    }
  }

  // PathSegment =
  //  '::'? NameRef
  // | NameRef GenericArgList?
  // | NameRef ParenthesizedArgList RetType?
  // | NameRef ReturnTypeSyntax
  // | TypeAnchor
  private def visitPathSegment(pathSegment: PathSegment): Ast = {
    pathSegment.nameRef match {
      case Some(nameRef) => visitNameRef(nameRef)
      case None =>
        pathSegment.typeAnchor match {
          case Some(typeAnchor) =>
            Ast(typeRefNode(typeAnchor, code(typeAnchor), typeFullNameForType(typeAnchor.typ)))
          case None => notHandledYet(pathSegment)
        }
    }
  }

  extension (pathSegment: PathSegment) {
    private def segmentName: Option[String] = {
      pathSegment.nameRef.map(code).orElse(pathSegment.typeAnchor.map(code))
    }
  }

  // NameRef =
  //  '#ident' | '@int_number' | 'self' | 'super' | 'crate' | 'Self'
  private def visitNameRef(nameRef: NameRef): Ast = {
    nameRef.name match {
      case Some(name) =>
        val typeFullName = typeFullNameForNameRef(nameRef)
        Ast(identifierNode(nameRef, name, code(nameRef), typeFullName))
      case None => notHandledYet(nameRef)
    }
  }

  extension (nameRef: NameRef) {
    private def name: Option[String] = {
      nameRef.identToken
        .orElse(nameRef.intNumberToken)
        .orElse(nameRef.selfKwToken)
        .orElse(nameRef.superKwToken)
        .orElse(nameRef.crateKwToken)
        .orElse(nameRef.selfTypeKwToken)
        .map(code)
    }
  }

  // BinExpr =
  //  Attr*
  //  lhs:Expr
  //  op:(
  //    '||' | '&&'
  //  | '==' | '!=' | '<=' | '>=' | '<' | '>'
  //  | '+' | '*' | '-' | '/' | '%' | '<<' | '>>' | '^' | '|' | '&'
  //  | '=' | '+=' | '/=' | '*=' | '%=' | '>>=' | '<<=' | '-=' | '|=' | '&=' | '^='
  //  )
  //  rhs:Expr
  private def visitBinExpr(binExpr: BinExpr): Ast = {
    operatorNameFor(binExpr) match {
      case Some(opName) =>
        val typeFullName = typeFullNameForExpr(binExpr)
        val callNode     = operatorCallNode(binExpr, code(binExpr), opName, Some(typeFullName))
        val lhsRhs       = binExpr.expr.map(visitExpr)
        callAst(callNode, lhsRhs)
      case None => notHandledYet(binExpr)
    }
  }

  extension (binExpr: BinExpr) {
    protected def op: Option[RustToken] =
      binExpr.pipe2Token
        .orElse(binExpr.amp2Token)
        .orElse(binExpr.eq2Token)
        .orElse(binExpr.neqToken)
        .orElse(binExpr.lteqToken)
        .orElse(binExpr.gteqToken)
        .orElse(binExpr.lAngleToken)
        .orElse(binExpr.rAngleToken)
        .orElse(binExpr.plusToken)
        .orElse(binExpr.starToken)
        .orElse(binExpr.minusToken)
        .orElse(binExpr.slashToken)
        .orElse(binExpr.percentToken)
        .orElse(binExpr.shlToken)
        .orElse(binExpr.shrToken)
        .orElse(binExpr.caretToken)
        .orElse(binExpr.pipeToken)
        .orElse(binExpr.ampToken)
        .orElse(binExpr.eqToken)
        .orElse(binExpr.pluseqToken)
        .orElse(binExpr.slasheqToken)
        .orElse(binExpr.stareqToken)
        .orElse(binExpr.percenteqToken)
        .orElse(binExpr.shreqToken)
        .orElse(binExpr.shleqToken)
        .orElse(binExpr.minuseqToken)
        .orElse(binExpr.pipeeqToken)
        .orElse(binExpr.ampeqToken)
        .orElse(binExpr.careteqToken)
  }

  // PrefixExpr =
  //  Attr* op:('-' | '!' | '*') Expr
  private def visitPrefixExpr(prefixExpr: PrefixExpr): Ast = {
    operatorNameFor(prefixExpr) match {
      case Some(opName) =>
        val typeFullName = typeFullNameForExpr(prefixExpr)
        val callNode     = operatorCallNode(prefixExpr, code(prefixExpr), opName, Some(typeFullName))
        val exprAst      = visitExpr(prefixExpr.expr)
        callAst(callNode, Seq(exprAst))
      case None => notHandledYet(prefixExpr)
    }
  }

  extension (prefixExpr: PrefixExpr) {
    protected def op: Option[RustToken] =
      prefixExpr.minusToken
        .orElse(prefixExpr.bangToken)
        .orElse(prefixExpr.starToken)
  }

  // RefExpr =
  //  Attr* '&' 'raw'? ('const' | 'mut')? Expr
  private def visitRefExpr(refExpr: RefExpr): Ast = {
    val typeFullName = typeFullNameForExpr(refExpr)
    val callNode     = operatorCallNode(refExpr, code(refExpr), Operators.addressOf, Some(typeFullName))
    val exprAst      = visitExpr(refExpr.expr)
    callAst(callNode, Seq(exprAst))
  }

  // RangeExpr =
  //  Attr* Expr? ('..' | '..=') Expr?
  private def visitRangeExpr(rangeExpr: RangeExpr): Ast = {
    val typeFullName = typeFullNameForExpr(rangeExpr)
    val callNode     = operatorCallNode(rangeExpr, code(rangeExpr), Operators.range, Some(typeFullName))
    val argAsts      = rangeExpr.expr.map(visitExpr)
    callAst(callNode, argAsts)
  }

  // RecordExpr =
  //  Path RecordExprFieldList
  private def visitRecordExpr(recordExpr: RecordExpr): Ast = {
    val typeFullName = typeFullNameForExpr(recordExpr)
    val callNode     = operatorCallNode(recordExpr, code(recordExpr), Operators.alloc, Some(typeFullName))
    callAst(callNode, visitRecordExprFieldList(recordExpr.recordExprFieldList))
  }

  // RecordExprFieldList =
  //  '{' fields:(RecordExprField (',' RecordExprField)* ','?)? ('..' Expr)? '}'
  private def visitRecordExprFieldList(recordExprFieldList: RecordExprFieldList): Seq[Ast] = {
    val fieldAsts  = recordExprFieldList.recordExprField.map(visitRecordExprField)
    val updateAsts = recordExprFieldList.expr.map(visitExpr).toSeq
    fieldAsts ++ updateAsts
  }

  // RecordExprField =
  //  Attr* (NameRef ':')? Expr
  private def visitRecordExprField(recordExprField: RecordExprField): Ast = {
    recordExprField.nameRef match {
      case Some(nameRef) =>
        val typeFullName = typeFullNameForNameRef(nameRef, allowLexicalFallback = false)
        val lhs          = Ast(identifierNode(nameRef, code(nameRef), code(nameRef), typeFullName))
        val rhs          = visitExpr(recordExprField.expr)
        callAst(assignmentNode(recordExprField, code(recordExprField), knownType(typeFullName)), Seq(lhs, rhs))
      case None =>
        visitExpr(recordExprField.expr)
    }
  }

  // IfExpr =
  //  Attr* 'if' condition:Expr then_branch:BlockExpr
  //  ('else' else_branch:(IfExpr | BlockExpr))?
  private def visitIfExpr(ifExpr: IfExpr): Ast = {
    val ifNode       = controlStructureNode(ifExpr, ControlStructureTypes.IF, code(ifExpr))
    val conditionAst = visitExpr(ifExpr.expr)
    val thenAst      = visitBlockExpr(ifExpr.thenBranch)
    val elseAst      = ifExpr.elseBranch.map(visitElseBranch)

    ifThenElseAst(ifNode, Some(conditionAst), thenAst, elseAst)
  }

  private def visitElseBranch(elseBranch: IfExpr | BlockExpr): Ast = {
    val elseNode = controlStructureNode(elseBranch, ControlStructureTypes.ELSE, "else")
    val bodyAst  = visitExpr(elseBranch)
    Ast(elseNode).withChild(bodyAst)
  }

  // MatchExpr =
  //  Attr* 'match' Expr MatchArmList
  private def visitMatchExpr(matchExpr: MatchExpr): Ast = {
    val switchNode   = controlStructureNode(matchExpr, ControlStructureTypes.SWITCH, code(matchExpr))
    val conditionAst = visitExpr(matchExpr.expr)
    val switchBlock  = Ast(blockNode(matchExpr.matchArmList)).withChildren(visitMatchArmList(matchExpr.matchArmList))
    switchAst(switchNode, conditionAst, Seq(switchBlock))
  }

  // MatchArmList =
  //  '{' MatchArm* '}'
  private def visitMatchArmList(matchArmList: MatchArmList): Seq[Ast] = {
    matchArmList.matchArm.flatMap(visitMatchArm)
  }

  // MatchArm =
  //  Attr* Pat MatchGuard? '=>' Expr ','?
  private def visitMatchArm(matchArm: MatchArm): Seq[Ast] = {
    val jumpTarget = matchArm.pat match {
      case _: WildcardPat => jumpTargetNode(matchArm, "default", "default")
      case pat            => jumpTargetNode(matchArm, "case", s"case ${code(pat)}")
    }
    val guardAsts = matchArm.matchGuard.map(_.expr).map(visitExpr).toSeq
    Seq(Ast(jumpTarget)) ++ guardAsts :+ visitExpr(matchArm.expr)
  }

  extension (ifExpr: IfExpr) {
    protected def thenBranch: BlockExpr = {
      ifExpr.blockExpr.head
    }

    protected def elseBranch: Option[IfExpr | BlockExpr] = {
      if (ifExpr.ifExpr.isDefined) {
        ifExpr.ifExpr
      } else if (ifExpr.blockExpr.sizeIs > 1) {
        Some(ifExpr.blockExpr.last)
      } else
        None
    }
  }

  // CastExpr =
  //  Attr* Expr 'as' Type
  private def visitCastExpr(castExpr: CastExpr): Ast = {
    val typeFullName = typeFullNameForType(castExpr.typ)
    val castNode     = operatorCallNode(castExpr, code(castExpr), Operators.cast, Some(typeFullName))
    val typeRefAst   = Ast(typeRefNode(castExpr.typ, code(castExpr.typ), typeFullName))
    val exprAst      = visitExpr(castExpr.expr)

    callAst(castNode, Seq(typeRefAst, exprAst))
  }

  // WhileExpr =
  //  Attr* Label? 'while' condition:Expr
  //  loop_body:BlockExpr
  private def visitWhileExpr(whileExpr: WhileExpr): Ast = {
    val whileNode    = controlStructureNode(whileExpr, ControlStructureTypes.WHILE, code(whileExpr))
    val conditionAst = visitExpr(whileExpr.expr)
    val bodyAst      = visitBlockExpr(whileExpr.blockExpr)

    whileBodyAst(whileNode, conditionAst, bodyAst)
  }

  // LoopExpr =
  //  Attr* Label? 'loop'
  //  loop_body:BlockExpr
  private def visitLoopExpr(loopExpr: LoopExpr): Ast = {
    val loopNode     = controlStructureNode(loopExpr, ControlStructureTypes.WHILE, code(loopExpr))
    val conditionAst = Ast(literalNode(loopExpr.loopKwToken, "true", "bool"))
    val bodyAst      = visitBlockExpr(loopExpr.blockExpr)

    whileBodyAst(loopNode, conditionAst, bodyAst)
  }

  // ForExpr =
  //  Attr* Label? 'for' Pat 'in' iterable:Expr
  //  loop_body:BlockExpr
  private def visitForExpr(forExpr: ForExpr): Ast = {
    val iterableAst = visitExpr(forExpr.expr)
    withLexicalTypeScope {
      val forNode     = controlStructureNode(forExpr, ControlStructureTypes.FOR, code(forExpr))
      val localAsts   = visitForLoopPat(forExpr.pat, forExpr.expr)
      val loopBodyAst = visitBlockExpr(forExpr.blockExpr)
      forAst(forNode, localAsts, Nil, Seq(iterableAst), Nil, loopBodyAst)
    }
  }

  private def visitForLoopPat(pat: Pat, iterableExpr: Expr): Seq[Ast] = pat match {
    case identPat: IdentPat =>
      identPat.name.identToken match {
        case Some(identToken) =>
          val name         = code(identToken)
          val typeFullName = typeFullNameForIdentPat(identPat)
          val inferredType =
            if (typeFullName == Defines.Any) typeFullNameForForLoopValue(iterableExpr) else typeFullName
          Seq(Ast(localNode(identToken, name, name, inferredType)))
        case None => localAstsForPattern(identPat)
      }
    case _ => localAstsForPattern(pat)
  }

  private def typeFullNameForForLoopValue(iterableExpr: Expr): String = iterableExpr match {
    case rangeExpr: RangeExpr =>
      rangeExpr.expr.map(typeFullNameForExpr).find(_ != Defines.Any).getOrElse(Defines.Any)
    case _ => Defines.Any
  }

  // ContinueExpr =
  //  Attr* 'continue' Lifetime?
  private def visitContinueExpr(continueExpr: ContinueExpr): Ast = {
    Ast(controlStructureNode(continueExpr, ControlStructureTypes.CONTINUE, code(continueExpr)))
  }

  // BreakExpr =
  //  Attr* 'break' Lifetime? Expr?
  private def visitBreakExpr(breakExpr: BreakExpr): Ast = {
    val breakNode = controlStructureNode(breakExpr, ControlStructureTypes.BREAK, code(breakExpr))
    controlStructureAst(breakNode, None, breakExpr.expr.toSeq.map(visitExpr))
  }

  // BecomeExpr =
  //  Attr* 'become' Expr
  private def visitBecomeExpr(becomeExpr: BecomeExpr): Ast = {
    val typeFullName = typeFullNameForExpr(becomeExpr)
    val callNode     = operatorCallNode(becomeExpr, code(becomeExpr), RustOperators.become, Some(typeFullName))
    callAst(callNode, Seq(visitExpr(becomeExpr.expr)))
  }

  // IndexExpr =
  //  Attr* base:Expr '[' index:Expr ']'
  private def visitIndexExpr(indexExpr: IndexExpr): Ast = {
    val typeFullName = typeFullNameForExpr(indexExpr)
    val callNode     = operatorCallNode(indexExpr, code(indexExpr), Operators.indexAccess, Some(typeFullName))
    val baseAst      = visitExpr(indexExpr.base)
    val indexAst     = visitExpr(indexExpr.index)
    callAst(callNode, Seq(baseAst, indexAst))
  }

  extension (indexExpr: IndexExpr) {
    protected def base: Expr  = indexExpr.expr.head
    protected def index: Expr = indexExpr.expr.last
  }

  // TupleExpr =
  //  Attr* '(' (Expr (',' Expr)* ','?)? ')'
  private def visitTupleExpr(tupleExpr: TupleExpr): Ast = {
    if (tupleExpr.expr.isEmpty) {
      Ast(literalNode(tupleExpr, code(tupleExpr), "()"))
    } else {
      val typeFullName = typeFullNameForTupleExpr(tupleExpr)
      val callNode     = operatorCallNode(tupleExpr, code(tupleExpr), RustOperators.tupleLiteral, Some(typeFullName))
      val argAsts      = tupleExpr.expr.map(visitExpr)
      callAst(callNode, argAsts)
    }
  }

  // ArrayExpr =
  //  Attr* '[' (
  //    (Expr (',' Expr)* ','?)?
  //  | Expr ';' Expr
  //  )']'
  private def visitArrayExpr(arrayExpr: ArrayExpr): Ast = {
    val typeFullName = typeFullNameForExpr(arrayExpr)
    val isRepeatForm = arrayExpr.semicolonToken.isDefined
    val operator     = if (isRepeatForm) RustOperators.repeatInArray else Operators.arrayInitializer
    val callNode     = operatorCallNode(arrayExpr, code(arrayExpr), operator, Some(typeFullName))

    callAst(callNode, arrayExpr.expr.map(visitExpr))
  }

  // FieldExpr =
  //  Attr* Expr '.' NameRef
  private def visitFieldExpr(fieldExpr: FieldExpr): Ast = {
    val baseAst      = visitExpr(fieldExpr.expr)
    val typeFullName = typeFullNameForExpr(fieldExpr)
    val nameRef      = fieldExpr.nameRef
    val fieldName    = code(nameRef)
    fieldAccessAst(fieldExpr, nameRef, baseAst, code(fieldExpr), fieldName, typeFullName)
  }

  // MethodCallExpr =
  //  Attr* receiver:Expr '.' NameRef GenericArgList? ArgList
  private def visitMethodCallExpr(methodCallExpr: MethodCallExpr): Ast = {
    val methodName     = code(methodCallExpr.nameRef)
    val methodFullName = methodFullNameForMethodCallExpr(methodCallExpr)
    val typeFullName   = typeFullNameForExpr(methodCallExpr)
    val dispatch =
      if (methodFullName == Defines.DynamicCallUnknownFullName || isDynDispatchReceiver(methodCallExpr.expr)) {
        DispatchTypes.DYNAMIC_DISPATCH
      } else DispatchTypes.STATIC_DISPATCH
    val call =
      callNode(methodCallExpr, code(methodCallExpr), methodName, methodFullName, dispatch, None, Some(typeFullName))
    val receiverAst = visitExpr(methodCallExpr.expr)
    val args        = methodCallExpr.argList.expr.map(visitExpr)
    callAst(call, args, base = Some(receiverAst))
  }

  private def isDynDispatchReceiver(expr: Expr): Boolean = {
    isDynTypeName(typeFullNameForExpr(expr))
  }

  private def isDynTypeName(typeFullName: String): Boolean = {
    val normalized = typeFullName.trim
      .stripPrefix("&mut ")
      .stripPrefix("&")
      .stripPrefix("*const ")
      .stripPrefix("*mut ")
      .trim
    normalized.startsWith("dyn ") || normalized.contains("<dyn ") || normalized.contains(", dyn ")
  }

  // AwaitExpr =
  //  Attr* Expr '.' 'await'
  private def visitAwaitExpr(awaitExpr: AwaitExpr): Ast = {
    val typeFullName = typeFullNameForExpr(awaitExpr)
    val callNode     = operatorCallNode(awaitExpr, code(awaitExpr), RustOperators.await, Some(typeFullName))
    callAst(callNode, Seq(visitExpr(awaitExpr.expr)))
  }

  // TryExpr =
  //  Attr* Expr '?'
  private def visitTryExpr(tryExpr: TryExpr): Ast = {
    val typeFullName = typeFullNameForExpr(tryExpr)
    val callNode     = operatorCallNode(tryExpr, code(tryExpr), RustOperators.tryPropagate, Some(typeFullName))
    callAst(callNode, Seq(visitExpr(tryExpr.expr)))
  }

  // YieldExpr =
  //  Attr* 'yield' Expr?
  private def visitYieldExpr(yieldExpr: YieldExpr): Ast = {
    val typeFullName = typeFullNameForExpr(yieldExpr)
    val callNode     = operatorCallNode(yieldExpr, code(yieldExpr), RustOperators.yieldValue, Some(typeFullName))
    callAst(callNode, yieldExpr.expr.map(visitExpr).toSeq)
  }

  // YeetExpr =
  //  Attr* 'do' 'yeet' Expr?
  private def visitYeetExpr(yeetExpr: YeetExpr): Ast = {
    val typeFullName = typeFullNameForExpr(yeetExpr)
    val callNode     = operatorCallNode(yeetExpr, code(yeetExpr), RustOperators.yeet, Some(typeFullName))
    callAst(callNode, yeetExpr.expr.map(visitExpr).toSeq)
  }

  // Struct =
  //  Attr* Visibility?
  //  'struct' Name GenericParamList? (
  //    WhereClause? (RecordFieldList | ';')
  //  | TupleFieldList WhereClause? ';'
  //  )
  private def visitStruct(struct: Struct): Ast = {
    (struct.recordFieldList, struct.tupleFieldList) match {
      case (Some(recordFieldList), _) =>
        Ast(typeDeclForStruct(struct)).withChildren(visitRecordFieldList(recordFieldList))
      case (None, Some(tupleFieldList)) =>
        Ast(typeDeclForStruct(struct)).withChildren(visitTupleFieldList(tupleFieldList))
      case (None, None) =>
        Ast(typeDeclForStruct(struct))
    }
  }

  // Union =
  //  Attr* Visibility? 'union' Name GenericParamList? WhereClause? RecordFieldList
  private def visitUnion(union: Union): Ast = {
    Ast(typeDeclForNamedItem(union, code(union.name))).withChildren(visitRecordFieldList(union.recordFieldList))
  }

  // Enum =
  //  Attr* Visibility? 'enum' Name GenericParamList? WhereClause? VariantList
  private def visitEnum(enumItem: Enum): Ast = {
    val typeDecl = typeDeclForNamedItem(enumItem, code(enumItem.name))
    Ast(typeDecl).withChildren(enumItem.variantList.variant.map(visitVariant(_, typeDecl.fullName)))
  }

  // Variant =
  //  Attr* Visibility? Name FieldList? ('=' ConstArg)?
  private def visitVariant(variant: Variant, enumTypeFullName: String): Ast = {
    Ast(memberNode(variant, code(variant.name), code(variant), enumTypeFullName))
  }

  // Trait =
  //  Attr* Visibility? 'unsafe'? 'auto'? ImplRestriction? 'trait' Name
  //  GenericParamList? (':' TypeBoundList?)? WhereClause? (AssocItemList | '=' TypeBoundList ';' | ';')
  private def visitTrait(traitItem: Trait): Ast = {
    val typeDecl = typeDeclForNamedItem(traitItem, code(traitItem.name))
    methodAstParentStack.push(typeDecl)
    val assocItemAsts = traitItem.assocItemList.toSeq.flatMap(_.assocItem.flatMap(visitAssocItem))
    methodAstParentStack.pop()
    Ast(typeDecl).withChildren(assocItemAsts)
  }

  // RecordFieldList =
  // '{' fields:(RecordField (',' RecordField)* ','?)? '}'
  private def visitRecordFieldList(recordFieldList: RecordFieldList): Seq[Ast] = {
    recordFieldList.recordField.map(visitRecordField)
  }

  // RecordField =
  //  Attr* Visibility? 'unsafe'?
  //  Name ':' Type ('=' default_val:ConstArg)?
  private def visitRecordField(recordField: RecordField): Ast = {
    Ast(memberNode(recordField, code(recordField.name), code(recordField), typeFullNameForType(recordField.typ)))
  }

  // TupleFieldList =
  //  '(' fields:(TupleField (',' TupleField)* ','?)? ')'
  private def visitTupleFieldList(tupleFieldList: TupleFieldList): Seq[Ast] = {
    tupleFieldList.tupleField.zipWithIndex.map { case (tupleField, index) => visitTupleField(tupleField, index) }
  }

  // TupleField =
  //  Attr* Visibility?
  //  Type
  private def visitTupleField(tupleField: TupleField, index: Int): Ast = {
    Ast(memberNode(tupleField, index.toString, code(tupleField), typeFullNameForType(tupleField.typ)))
  }

}
