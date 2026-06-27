package io.joern.rust2cpg.astcreation

import io.joern.rust2cpg.parser.RustNodeSyntax
import io.joern.rust2cpg.parser.RustNodeSyntax.RustNode
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.PropertyNames
import io.shiftleft.codepropertygraph.generated.nodes.{NewMethod, NewNamespaceBlock}
import io.shiftleft.semanticcpg.language.types.structure.NamespaceTraversal

// Computes rust-style full names, e.g. `crate::module::item`.
// rust_ast_gen-provided methodFullName/typeFullName are always preferred.
// The fallback is to rely on the current enclosing scope.
trait RustFullNames { this: AstCreator =>
  import RustFullNames.PathSep

  protected def combineRustFullName(parent: String, child: String): String = {
    s"$parent$PathSep$child"
  }

  private def crateName: String = {
    parseResult.crateName.getOrElse(Defines.UnresolvedNamespace)
  }

  protected def rustNamespaceFullName: String = parseResult.modulePath match {
    case Some(path) => combineRustFullName(crateName, path)
    case None       => crateName
  }

  protected def composeRustFullName(name: String): String = {
    combineRustFullName(rustParentFullName, name)
  }

  private def rustParentFullName: String = {
    // We don't want to have the "global method"'s fullname propagated since that
    // would not match rust_ast_gen's full names.
    val parent = methodAstParentStack.find {
      case method: NewMethod if method.name == NamespaceTraversal.globalNamespaceName => false
      case _                                                                          => true
    }.get
    // When the parent is a namespace_block, we don't want its fullName since it's
    // file-prefix in order to be unique, but rather its name. For other nodes,
    // we do want the fullName.
    parent match {
      case ns: NewNamespaceBlock => ns.name
      case other                 => other.properties(PropertyNames.FullName).toString
    }
  }

  protected def methodFullNameForCallExpr(callExpr: RustNodeSyntax.CallExpr, segments: Seq[String]): String = {
    callExpr.methodFullName.getOrElse {
      importedFullName(segments).getOrElse {
        segments match {
          case name :: Nil => combineRustFullName(Defines.UnresolvedNamespace, name)
          case names       => names.mkString(PathSep)
        }
      }
    }
  }

  private def importedFullName(segments: Seq[String]): Option[String] = {
    explicitImportedFullName(segments).orElse(lookupWildcardImport(segments))
  }

  private def explicitImportedFullName(segments: Seq[String]): Option[String] = {
    segments match {
      case alias +: rest =>
        lookupImportAlias(alias).map { importedEntity =>
          (importedEntity +: rest).mkString(PathSep)
        }
      case _ => None
    }
  }

  protected def methodFullNameForMethodCallExpr(methodCallExpr: RustNodeSyntax.MethodCallExpr): String = {
    methodCallExpr.methodFullName
      .orElse(localReceiverMethodFullName(methodCallExpr))
      .getOrElse(Defines.DynamicCallUnknownFullName)
  }

  private def localReceiverMethodFullName(methodCallExpr: RustNodeSyntax.MethodCallExpr): Option[String] = {
    val receiverTypeFullName = canonicalReceiverTypeFullName(typeFullNameForExpr(methodCallExpr.expr))
    val methodName           = code(methodCallExpr.nameRef)
    Option
      .when(isLocalTypeFullName(receiverTypeFullName) && methodName.nonEmpty) {
        combineRustFullName(receiverTypeFullName, methodName)
      }
  }

  private def canonicalReceiverTypeFullName(typeFullName: String): String = {
    typeFullName.trim
      .stripPrefix("&mut ")
      .stripPrefix("&")
      .stripPrefix("*const ")
      .stripPrefix("*mut ")
      .trim
      .takeWhile(_ != '<')
  }

  private def isLocalTypeFullName(typeFullName: String): Boolean = {
    crateName != Defines.UnresolvedNamespace && typeFullName.startsWith(s"$crateName$PathSep")
  }

  protected def methodFullNameForMacroCall(segments: Seq[String]): String = {
    importedFullName(segments)
      .map(addMacroBang)
      .getOrElse {
        val macroName = s"${segments.last}!"
        segments match {
          case _ :: Nil => combineRustFullName(Defines.UnresolvedNamespace, macroName)
          case names    => names.init.appended(macroName).mkString(PathSep)
        }
      }
  }

  private def addMacroBang(fullName: String): String = {
    if (fullName.endsWith("!")) fullName else s"$fullName!"
  }

  protected def typeFullNameForType(typ: RustNodeSyntax.Type): String = {
    typ match {
      case pathType: RustNodeSyntax.PathType =>
        typeFullNameForPath(pathType.path, allowLexicalFallback = false, allowImportFallback = true)
      case refType: RustNodeSyntax.RefType =>
        val mut = Option.when(refType.mutKwToken.isDefined)("mut ").getOrElse("")
        s"&$mut${typeFullNameForType(refType.typ)}"
      case ptrType: RustNodeSyntax.PtrType =>
        val qualifier = if (ptrType.constKwToken.isDefined) "const " else "mut "
        s"*$qualifier${typeFullNameForType(ptrType.typ)}"
      case sliceType: RustNodeSyntax.SliceType =>
        s"[${typeFullNameForType(sliceType.typ)}]"
      case arrayType: RustNodeSyntax.ArrayType =>
        s"[${typeFullNameForType(arrayType.typ)}; ${text(arrayType.constArg).getOrElse("")}]"
      case tupleType: RustNodeSyntax.TupleType =>
        s"(${tupleType.typ.map(typeFullNameForType).mkString(", ")})"
      case parenType: RustNodeSyntax.ParenType =>
        typeFullNameForType(parenType.typ)
      case _: RustNodeSyntax.NeverType =>
        "!"
      case fnPtrType: RustNodeSyntax.FnPtrType =>
        typeFullNameForFnPtrType(fnPtrType)
      case dynTraitType: RustNodeSyntax.DynTraitType =>
        val dynPrefix = Option.when(dynTraitType.dynKwToken.isDefined)("dyn ").getOrElse("")
        s"$dynPrefix${typeFullNameForTypeBoundList(dynTraitType.typeBoundList)}"
      case forType: RustNodeSyntax.ForType =>
        s"${code(forType.forBinder)} ${typeFullNameForType(forType.typ)}"
      case implTraitType: RustNodeSyntax.ImplTraitType =>
        s"impl ${typeFullNameForTypeBoundList(implTraitType.typeBoundList)}"

      // TODO: the following are not handled yet.
      case io.joern.rust2cpg.parser.RustNodeSyntax.InferType(_) =>
        // TODO(rust_ast_gen): is this typeFullName missing on purpose or by accident?
        //  This corresponds to `_` in something like `let x: Vec<_>`. We currently don't have a
        //  typeFullName for it, but we have for `x`.
        text(typ).getOrElse(Defines.Any)
      case io.joern.rust2cpg.parser.RustNodeSyntax.MacroType(_) =>
        // TODO: pending macroExpansion from rust_ast_gen.
        text(typ).getOrElse(Defines.Any)
    }
  }

  protected def typeFullNameForNameRef(
    nameRef: RustNodeSyntax.NameRef,
    allowLexicalFallback: Boolean = true
  ): String = {
    nameRef.typeFullName
      .filter(_ != Defines.Any)
      .orElse {
        Option.when(allowLexicalFallback)(text(nameRef).flatMap(lookupLexicalType)).flatten
      }
      .getOrElse(Defines.Any)
  }

  protected def typeFullNameForPath(
    path: RustNodeSyntax.Path,
    allowLexicalFallback: Boolean = true,
    allowImportFallback: Boolean = false
  ): String = {
    // In a path, only the leaf (NameRef) has typeFullName set (by rust_ast_gen).
    val astgenType = path.pathSegment.nameRef.flatMap(_.typeFullName).filter(_ != Defines.Any)
    val importType = Option.when(allowImportFallback)(pathSegments(path).flatMap(importedTypeFullName(_, path))).flatten
    val baseType = importType
      .filter(_ => astgenType.isEmpty || astgenType.exists(isRawPathType(path, _)))
      .orElse(astgenType)
      .orElse(importType)
      .orElse {
        Option
          .when(allowLexicalFallback && path.path.isEmpty)(
            path.pathSegment.nameRef.flatMap(ref => text(ref).flatMap(lookupLexicalType))
          )
          .flatten
      }
      .getOrElse(Defines.Any)
    path.pathSegment.genericArgList match {
      case Some(genericArgList) => typeFullNameWithGenericArgs(baseType, genericArgList)
      case None =>
        path.pathSegment.parenthesizedArgList match {
          case Some(argList) => typeFullNameWithParenthesizedArgs(baseType, argList, path.pathSegment.retType)
          case None          => baseType
        }
    }
  }

  private def isRawPathType(path: RustNodeSyntax.Path, typeFullName: String): Boolean = {
    text(path).exists { rawPath =>
      typeFullName == rawPath || rawPath.split(PathSep).contains(typeFullName)
    }
  }

  private def pathSegments(path: RustNodeSyntax.Path): Option[Seq[String]] = {
    path.pathSegment.nameRef.flatMap(text).orElse(path.pathSegment.typeAnchor.flatMap(text)).map { segment =>
      path.path match {
        case Some(qualifier) => pathSegments(qualifier).getOrElse(Nil) :+ segment
        case None            => segment :: Nil
      }
    }
  }

  private def importedTypeFullName(segments: Seq[String], path: RustNodeSyntax.Path): Option[String] = {
    explicitImportedFullName(segments).orElse {
      Option.when(path.pathSegment.genericArgList.isEmpty)(lookupWildcardImport(segments)).flatten
    }
  }

  private def typeFullNameWithGenericArgs(baseType: String, genericArgList: RustNodeSyntax.GenericArgList): String = {
    val base = baseType.takeWhile(_ != '<')
    val args = genericArgList.genericArg.map(typeFullNameForGenericArg)
    s"$base<${args.mkString(", ")}>"
  }

  private def typeFullNameWithParenthesizedArgs(
    baseType: String,
    argList: RustNodeSyntax.ParenthesizedArgList,
    retType: Option[RustNodeSyntax.RetType]
  ): String = {
    val args = argList.typeArg.map(typeArg => typeFullNameForType(typeArg.typ))
    val ret  = retType.map(retType => s" -> ${typeFullNameForType(retType.typ)}").getOrElse("")
    s"$baseType(${args.mkString(", ")})$ret"
  }

  private def typeFullNameForGenericArg(genericArg: RustNodeSyntax.GenericArg): String = {
    genericArg match {
      case typeArg: RustNodeSyntax.TypeArg =>
        typeFullNameForType(typeArg.typ)
      case assocTypeArg: RustNodeSyntax.AssocTypeArg =>
        assocTypeArg.typ match {
          case Some(typ) => s"${code(assocTypeArg.nameRef)} = ${typeFullNameForType(typ)}"
          case None      => text(assocTypeArg).getOrElse(Defines.Any)
        }
      case _ =>
        text(genericArg).getOrElse(Defines.Any)
    }
  }

  private def typeFullNameForFnPtrType(fnPtrType: RustNodeSyntax.FnPtrType): String = {
    val params = fnPtrType.paramList.param.map { param =>
      param.typ.map(typeFullNameForType).getOrElse(text(param).getOrElse(Defines.Any))
    }
    val ret = fnPtrType.retType.map(retType => s" -> ${typeFullNameForType(retType.typ)}").getOrElse("")
    s"fn(${params.mkString(", ")})$ret"
  }

  private def typeFullNameForTypeBoundList(typeBoundList: RustNodeSyntax.TypeBoundList): String = {
    typeBoundList.typeBound.map(typeFullNameForTypeBound).mkString(" + ")
  }

  private def typeFullNameForTypeBound(typeBound: RustNodeSyntax.TypeBound): String = {
    val prefix = Seq(
      Option.when(typeBound.tildeToken.isDefined)("~"),
      Option.when(typeBound.constKwToken.isDefined)("const "),
      Option.when(typeBound.asyncKwToken.isDefined)("async "),
      typeBound.forBinder.map(forBinder => s"${code(forBinder)} ")
    ).flatten.mkString
    val bound = typeBound.typ
      .map(typeFullNameForType)
      .orElse(typeBound.lifetime.map(code))
      .getOrElse(text(typeBound).getOrElse(Defines.Any))
    val maybeBound = if (typeBound.questionToken.isDefined) s"?$bound" else bound
    s"$prefix$maybeBound"
  }

  protected def typeFullNameForExpr(expr: RustNodeSyntax.Expr): String = {
    knownExprTypeFullName(expr)
      .orElse(expr.typeFullName)
      .filter(_ != Defines.Any)
      .orElse {
        expr match {
          case pathExpr: RustNodeSyntax.PathExpr =>
            Some(typeFullNameForPath(pathExpr.path))
          case indexExpr: RustNodeSyntax.IndexExpr =>
            indexedElementType(typeFullNameForExpr(indexExpr.base))
          case binExpr: RustNodeSyntax.BinExpr if isAssignmentExpr(binExpr) =>
            binExpr.expr.headOption.map(typeFullNameForExpr).filter(_ != Defines.Any)
          case _ =>
            None
        }
      }
      .getOrElse(Defines.Any)
  }

  protected def knownExprTypeFullName(expr: RustNodeSyntax.Expr): Option[String] = {
    expr match {
      case callExpr: RustNodeSyntax.CallExpr => knownCallExprReturnType(callExpr)
      case _                                 => None
    }
  }

  private def knownCallExprReturnType(callExpr: RustNodeSyntax.CallExpr): Option[String] = {
    val segments = callExpr.expr match {
      case pathExpr: RustNodeSyntax.PathExpr => pathSegments(pathExpr.path)
      case _                                 => None
    }
    (callExpr.methodFullName, segments) match {
      case (Some("core::convert::From<T>::from"), Some(path)) if path.takeRight(2) == Seq("String", "from") =>
        Some("alloc::string::String")
      case _ => None
    }
  }

  private def isAssignmentExpr(binExpr: RustNodeSyntax.BinExpr): Boolean = {
    binExpr.eqToken.isDefined ||
    binExpr.pluseqToken.isDefined ||
    binExpr.slasheqToken.isDefined ||
    binExpr.stareqToken.isDefined ||
    binExpr.percenteqToken.isDefined ||
    binExpr.shreqToken.isDefined ||
    binExpr.shleqToken.isDefined ||
    binExpr.minuseqToken.isDefined ||
    binExpr.pipeeqToken.isDefined ||
    binExpr.ampeqToken.isDefined ||
    binExpr.careteqToken.isDefined
  }

  private def indexedElementType(containerType: String): Option[String] = {
    val typeName = stripReference(containerType.trim)
    if (typeName.startsWith("Vec<") && typeName.endsWith(">")) {
      Some(typeName.substring("Vec<".length, typeName.length - 1))
    } else if (typeName.startsWith("[") && typeName.endsWith("]")) {
      val inner = typeName.substring(1, typeName.length - 1)
      Some(inner.takeWhile(_ != ';').trim)
    } else {
      None
    }
  }

  private def stripReference(typeName: String): String = {
    typeName
      .stripPrefix("&mut ")
      .stripPrefix("&")
      .trim
  }

  protected def typeFullNameForTupleExpr(tupleExpr: RustNodeSyntax.TupleExpr): String = {
    tupleExpr.typeFullName.getOrElse {
      val childTypes = tupleExpr.expr.map(typeFullNameForExpr)
      s"(${childTypes.mkString(", ")})"
    }
  }

  protected def typeFullNameForLiteral(lit: RustNodeSyntax.Literal): String = {
    lit.typeFullName.orElse(lit.value.map(typeFullNameForLiteralToken)).getOrElse(Defines.Any)
  }

  protected def typeFullNameForIdentPat(identPat: RustNodeSyntax.IdentPat): String = {
    identPat.typeFullName.getOrElse(Defines.Any)
  }

  private def typeFullNameForLiteralToken(tok: RustNodeSyntax.RustToken): String = tok match {
    case _: RustNodeSyntax.IntNumberToken   => "i32"
    case _: RustNodeSyntax.FloatNumberToken => "f64"
    case _: RustNodeSyntax.StringToken      => "&str"
    case _: RustNodeSyntax.ByteStringToken  => "&[u8]"
    case _: RustNodeSyntax.CStringToken     => "&CStr"
    case _: RustNodeSyntax.CharToken        => "char"
    case _: RustNodeSyntax.ByteToken        => "u8"
    case _: RustNodeSyntax.TrueKwToken      => "bool"
    case _: RustNodeSyntax.FalseKwToken     => "bool"
    case _                                  => Defines.Any
  }

}

object RustFullNames {
  val PathSep = "::"
}
