package io.joern.pysrc2cpg.parser

import io.joern.pythonparser.{ast => py}
import io.joern.pythonparser.ast.AttributeProvider

import java.nio.file.{Files, Path, Paths}
import java.util.regex.Pattern
import scala.collection.mutable
import scala.util.Try

object PyAstJsonParser {

  case class ParsedModule(module: py.Module, relFileName: String, source: String, fullPath: String)

  private case class JsonAttributeProvider(
    lineno: Int,
    col_offset: Int,
    input_offset: Int,
    end_lineno: Int,
    end_col_offset: Int,
    end_input_offset: Int
  ) extends AttributeProvider

  private case class ArgWithDefault(arg: py.Arg, default: Option[py.iexpr])

  private val fallbackAttributeProvider = JsonAttributeProvider(1, 1, 0, 1, 1, 0)

  def sourcePath(jsonPath: Path): Option[Path] = {
    Try {
      Paths.get(ujson.read(Files.readString(jsonPath))("path").str)
    }.toOption
  }

  def parseFile(jsonPath: Path, inputRoot: Path): ParsedModule = {
    parseDocument(ujson.read(Files.readString(jsonPath)), inputRoot, jsonPath)
  }

  def parseDocument(json: ujson.Value, inputRoot: Path, jsonPath: Path): ParsedModule = {
    val fullPath = json("path").str
    val root     = json("root")
    val source   = text(root).getOrElse("")
    ParsedModule(parseModule(root), relativeFileName(fullPath, inputRoot, jsonPath), source, fullPath)
  }

  private def parseModule(node: ujson.Value): py.Module = {
    expectKind(node, "Module")
    py.Module(coll(children(node, "body").map(parseStmt)), mutable.ArrayBuffer.empty)
  }

  private def parseStmt(node: ujson.Value): py.istmt = {
    kind(node) match {
      case "FunctionDef" =>
        py.FunctionDef(
          strProp(node, "name"),
          parseArguments(child(node, "args")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "decorator_list").map(parseExpr)),
          childOpt(node, "returns").map(parseExpr),
          optStrProp(node, "type_comment"),
          coll(children(node, "type_params").map(parseTypeParam)),
          attr(node)
        )
      case "AsyncFunctionDef" =>
        py.AsyncFunctionDef(
          strProp(node, "name"),
          parseArguments(child(node, "args")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "decorator_list").map(parseExpr)),
          childOpt(node, "returns").map(parseExpr),
          optStrProp(node, "type_comment"),
          coll(children(node, "type_params").map(parseTypeParam)),
          attr(node)
        )
      case "ClassDef" =>
        py.ClassDef(
          strProp(node, "name"),
          coll(children(node, "bases").map(parseExpr)),
          coll(children(node, "keywords").map(parseKeyword)),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "decorator_list").map(parseExpr)),
          coll(children(node, "type_params").map(parseTypeParam)),
          attr(node)
        )
      case "Return" =>
        py.Return(childOpt(node, "value").map(parseExpr), attr(node))
      case "Delete" =>
        py.Delete(coll(children(node, "targets").map(parseExpr)), attr(node))
      case "Assign" =>
        py.Assign(
          coll(children(node, "targets").map(parseExpr)),
          parseExpr(child(node, "value")),
          optStrProp(node, "type_comment"),
          attr(node)
        )
      case "TypeAlias" =>
        py.TypeAlias(
          parseExpr(child(node, "name")),
          coll(children(node, "type_params").map(parseTypeParam)),
          parseExpr(child(node, "value")),
          attr(node)
        )
      case "AugAssign" =>
        py.AugAssign(
          parseExpr(child(node, "target")),
          parseOperator(strProp(node, "op")),
          parseExpr(child(node, "value")),
          attr(node)
        )
      case "AnnAssign" =>
        py.AnnAssign(
          parseExpr(child(node, "target")),
          parseExpr(child(node, "annotation")),
          childOpt(node, "value").map(parseExpr),
          boolProp(node, "simple", default = false),
          attr(node)
        )
      case "For" =>
        py.For(
          parseExpr(child(node, "target")),
          parseExpr(child(node, "iter")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "orelse").map(parseStmt)),
          optStrProp(node, "type_comment"),
          attr(node)
        )
      case "AsyncFor" =>
        py.AsyncFor(
          parseExpr(child(node, "target")),
          parseExpr(child(node, "iter")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "orelse").map(parseStmt)),
          optStrProp(node, "type_comment"),
          attr(node)
        )
      case "While" =>
        py.While(
          parseExpr(child(node, "test")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "orelse").map(parseStmt)),
          attr(node)
        )
      case "If" =>
        py.If(
          parseExpr(child(node, "test")),
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "orelse").map(parseStmt)),
          attr(node)
        )
      case "With" =>
        py.With(
          coll(children(node, "items").map(parseWithItem)),
          coll(children(node, "body").map(parseStmt)),
          optStrProp(node, "type_comment"),
          attr(node)
        )
      case "AsyncWith" =>
        py.AsyncWith(
          coll(children(node, "items").map(parseWithItem)),
          coll(children(node, "body").map(parseStmt)),
          optStrProp(node, "type_comment"),
          attr(node)
        )
      case "Match" =>
        py.Match(parseExpr(child(node, "subject")), coll(children(node, "cases").map(parseMatchCase)), attr(node))
      case "Raise" =>
        py.Raise(childOpt(node, "exc").map(parseExpr), childOpt(node, "cause").map(parseExpr), attr(node))
      case "Try" | "TryStar" =>
        py.Try(
          coll(children(node, "body").map(parseStmt)),
          coll(children(node, "handlers").map(parseExceptHandler)),
          coll(children(node, "orelse").map(parseStmt)),
          coll(children(node, "finalbody").map(parseStmt)),
          attr(node)
        )
      case "Assert" =>
        py.Assert(parseExpr(child(node, "test")), childOpt(node, "msg").map(parseExpr), attr(node))
      case "Import" =>
        py.Import(coll(children(node, "names").map(parseAlias)), attr(node))
      case "ImportFrom" =>
        py.ImportFrom(
          optStrProp(node, "module"),
          coll(children(node, "names").map(parseAlias)),
          intProp(node, "level", 0),
          attr(node)
        )
      case "Global" =>
        py.Global(coll(stringArrayProp(node, "names")), attr(node))
      case "Nonlocal" =>
        py.Nonlocal(coll(stringArrayProp(node, "names")), attr(node))
      case "Expr" =>
        py.Expr(parseExpr(child(node, "value")), attr(node))
      case "Pass" =>
        py.Pass(attr(node))
      case "Break" =>
        py.Break(attr(node))
      case "Continue" =>
        py.Continue(attr(node))
      case other =>
        unsupported(node, s"statement kind '$other'")
    }
  }

  private def parseExpr(node: ujson.Value): py.iexpr = {
    kind(node) match {
      case "BoolOp" =>
        py.BoolOp(parseBoolOp(strProp(node, "op")), coll(children(node, "values").map(parseExpr)), attr(node))
      case "NamedExpr" =>
        py.NamedExpr(parseExpr(child(node, "target")), parseExpr(child(node, "value")), attr(node))
      case "BinOp" =>
        py.BinOp(
          parseExpr(child(node, "left")),
          parseOperator(strProp(node, "op")),
          parseExpr(child(node, "right")),
          attr(node)
        )
      case "UnaryOp" =>
        py.UnaryOp(parseUnaryOp(strProp(node, "op")), parseExpr(child(node, "operand")), attr(node))
      case "Lambda" =>
        py.Lambda(parseArguments(child(node, "args")), parseExpr(child(node, "body")), attr(node))
      case "IfExp" =>
        py.IfExp(
          parseExpr(child(node, "test")),
          parseExpr(child(node, "body")),
          parseExpr(child(node, "orelse")),
          attr(node)
        )
      case "Dict" =>
        py.Dict(
          coll(children(node, "keys").map(parseOptionalDictKey)),
          coll(children(node, "values").map(parseExpr)),
          attr(node)
        )
      case "Set" =>
        py.Set(coll(children(node, "elts").map(parseExpr)), attr(node))
      case "ListComp" =>
        py.ListComp(
          parseExpr(child(node, "elt")),
          coll(children(node, "generators").map(parseComprehension)),
          attr(node)
        )
      case "SetComp" =>
        py.SetComp(
          parseExpr(child(node, "elt")),
          coll(children(node, "generators").map(parseComprehension)),
          attr(node)
        )
      case "DictComp" =>
        py.DictComp(
          parseExpr(child(node, "key")),
          parseExpr(child(node, "value")),
          coll(children(node, "generators").map(parseComprehension)),
          attr(node)
        )
      case "GeneratorExp" =>
        py.GeneratorExp(
          parseExpr(child(node, "elt")),
          coll(children(node, "generators").map(parseComprehension)),
          attr(node)
        )
      case "Await" =>
        py.Await(parseExpr(child(node, "value")), attr(node))
      case "Yield" =>
        py.Yield(childOpt(node, "value").map(parseExpr), attr(node))
      case "YieldFrom" =>
        py.YieldFrom(parseExpr(child(node, "value")), attr(node))
      case "Compare" =>
        py.Compare(
          parseExpr(child(node, "left")),
          coll(stringArrayProp(node, "ops").map(parseCompOp)),
          coll(children(node, "comparators").map(parseExpr)),
          attr(node)
        )
      case "Call" =>
        py.Call(
          parseExpr(child(node, "func")),
          coll(children(node, "args").map(parseExpr)),
          coll(children(node, "keywords").map(parseKeyword)),
          attr(node)
        )
      case "FormattedValue" =>
        parseFormattedValue(node)
      case "JoinedStr" | "JoinedString" =>
        val (quote, prefix, _) = stringParts(node)
        py.JoinedString(coll(parseJoinedStringValues(node)), quote, prefix, attr(node))
      case "Constant" =>
        parseStringExpressionList(node).getOrElse(py.Constant(parseConstant(node), attr(node)))
      case "Attribute" =>
        py.Attribute(parseExpr(child(node, "value")), strProp(node, "attr"), attr(node))
      case "Subscript" =>
        py.Subscript(parseExpr(child(node, "value")), parseExpr(child(node, "slice")), attr(node))
      case "Starred" =>
        py.Starred(parseExpr(child(node, "value")), attr(node))
      case "Name" =>
        py.Name(strProp(node, "id"), attr(node))
      case "List" =>
        py.List(coll(children(node, "elts").map(parseExpr)), attr(node))
      case "Tuple" =>
        py.Tuple(coll(children(node, "elts").map(parseExpr)), attr(node))
      case "Slice" =>
        py.Slice(
          childOpt(node, "lower").map(parseExpr),
          childOpt(node, "upper").map(parseExpr),
          childOpt(node, "step").map(parseExpr),
          attr(node)
        )
      case other =>
        unsupported(node, s"expression kind '$other'")
    }
  }

  private def parseJoinedStringValues(node: ujson.Value): Seq[py.iexpr] = {
    val values    = children(node, "values")
    val rawFValue = text(node)
    val out       = mutable.ArrayBuffer.empty[py.iexpr]
    var index     = 0

    while (index < values.length) {
      val current = values(index)
      val next    = values.lift(index + 1)
      (joinedStringConstantValue(current), next.filter(value => kind(value) == "FormattedValue")) match {
        case (Some(segment), Some(formatted)) =>
          splitDebugFStringPrefix(segment, formatted, rawFValue) match {
            case Some((literalPrefix, defaultDebugConversion)) =>
              if (literalPrefix.nonEmpty) {
                out += joinedStringConstant(current, literalPrefix)
              }
              out += parseFormattedValue(formatted, equalSign = true, defaultDebugConversion = defaultDebugConversion)
              index += 2
            case None =>
              out += joinedStringConstant(current, segment)
              index += 1
          }
        case (Some(segment), _) =>
          out += joinedStringConstant(current, segment)
          index += 1
        case _ =>
          out += parseExpr(current)
          index += 1
      }
    }

    out.toSeq
  }

  private def parseFormattedValue(
    node: ujson.Value,
    equalSign: Boolean = false,
    defaultDebugConversion: Boolean = false
  ): py.FormattedValue = {
    py.FormattedValue(
      parseExpr(child(node, "value")),
      if (defaultDebugConversion) -1 else parseConversion(optStrProp(node, "conversion")),
      childOpt(node, "format_spec").map(formatSpecText),
      equalSign,
      attr(node)
    )
  }

  private def joinedStringConstant(node: ujson.Value, value: String): py.Constant = {
    py.Constant(py.StringConstant(value, "", ""), attr(node))
  }

  private def joinedStringConstantValue(node: ujson.Value): Option[String] = {
    Option.when(kind(node) == "Constant" && optStrProp(node, "value_kind").contains("Str")) {
      strProp(node, "value")
    }
  }

  private def splitDebugFStringPrefix(
    segment: String,
    formatted: ujson.Value,
    rawFStringText: Option[String]
  ): Option[(String, Boolean)] = {
    text(child(formatted, "value")).flatMap { expressionText =>
      val equalsIndex = segment.lastIndexOf('=')
      Option
        .when(equalsIndex >= 0) {
          val beforeEquals    = segment.take(equalsIndex)
          val expressionEnd   = beforeEquals.lastIndexWhere(!_.isWhitespace) + 1
          val expressionStart = expressionEnd - expressionText.length
          val suffixIsExpression =
            expressionStart >= 0 && beforeEquals.slice(expressionStart, expressionEnd) == expressionText
          val trailingEqualsSpace   = segment.drop(equalsIndex + 1).forall(_.isWhitespace)
          val rawContainsDebugField = rawFStringText.exists(isDebugField(_, expressionText))

          Option.when(suffixIsExpression && trailingEqualsSpace && rawContainsDebugField) {
            val literalPrefix = beforeEquals.take(expressionStart)
            val defaultDebugConversion =
              optStrProp(formatted, "conversion").contains("Repr") &&
                childOpt(formatted, "format_spec").isEmpty &&
                rawFStringText.exists(isDefaultDebugField(_, expressionText))
            literalPrefix -> defaultDebugConversion
          }
        }
        .flatten
    }
  }

  private def isDebugField(rawFStringText: String, expressionText: String): Boolean = {
    val expression = Pattern.quote(expressionText)
    rawFStringText.matches(s"""(?s).*\\{\\s*$expression\\s*=.*""")
  }

  private def isDefaultDebugField(rawFStringText: String, expressionText: String): Boolean = {
    val expression = Pattern.quote(expressionText)
    rawFStringText.matches(s"""(?s).*\\{\\s*$expression\\s*=\\s*\\}.*""")
  }

  private def formatSpecText(node: ujson.Value): String = {
    kind(node) match {
      case "JoinedStr" | "JoinedString" =>
        children(node, "values").map(formatSpecPartText).mkString
      case "Constant" if optStrProp(node, "value_kind").contains("Str") =>
        strProp(node, "value")
      case _ =>
        new io.joern.pythonparser.AstPrinter("").print(parseExpr(node))
    }
  }

  private def formatSpecPartText(node: ujson.Value): String = {
    joinedStringConstantValue(node).getOrElse(new io.joern.pythonparser.AstPrinter("").print(parseExpr(node)))
  }

  private def parseOptionalDictKey(node: ujson.Value): Option[py.iexpr] = {
    kind(node) match {
      case "DictUnpack" => None
      case _            => Some(parseExpr(node))
    }
  }

  private def parseArguments(node: ujson.Value): py.Arguments = {
    expectKind(node, "Arguments")

    val posOnly     = children(node, "posonlyargs").map(parseArgWithDefault)
    val normal      = children(node, "args").map(parseArgWithDefault)
    val keywordOnly = children(node, "kwonlyargs").map(parseArgWithDefault)
    val defaults    = (posOnly ++ normal).flatMap(_.default)

    py.Arguments(
      coll(posOnly.map(_.arg)),
      coll(normal.map(_.arg)),
      childOpt(node, "vararg").map(parseArg),
      coll(keywordOnly.map(_.arg)),
      coll(keywordOnly.map(_.default)),
      childOpt(node, "kwarg").map(parseArg),
      coll(defaults)
    )
  }

  private def parseArgWithDefault(node: ujson.Value): ArgWithDefault = {
    kind(node) match {
      case "ArgWithDefault" => ArgWithDefault(parseArg(child(node, "def")), childOpt(node, "default").map(parseExpr))
      case "Arg"            => ArgWithDefault(parseArg(node), None)
      case other            => unsupported(node, s"argument kind '$other'")
    }
  }

  private def parseArg(node: ujson.Value): py.Arg = {
    expectKind(node, "Arg")
    py.Arg(
      strProp(node, "arg"),
      childOpt(node, "annotation").map(parseExpr),
      optStrProp(node, "type_comment"),
      attr(node)
    )
  }

  private def parseKeyword(node: ujson.Value): py.Keyword = {
    expectKind(node, "Keyword")
    py.Keyword(optStrProp(node, "arg"), parseExpr(child(node, "value")), attr(node))
  }

  private def parseAlias(node: ujson.Value): py.Alias = {
    expectKind(node, "Alias")
    py.Alias(strProp(node, "name"), optStrProp(node, "asname"))
  }

  private def parseWithItem(node: ujson.Value): py.Withitem = {
    expectKind(node, "WithItem")
    py.Withitem(parseExpr(child(node, "context_expr")), childOpt(node, "optional_vars").map(parseExpr))
  }

  private def parseMatchCase(node: ujson.Value): py.MatchCase = {
    expectKind(node, "MatchCase")
    py.MatchCase(
      parsePattern(child(node, "pattern")),
      childOpt(node, "guard").map(parseExpr),
      coll(children(node, "body").map(parseStmt))
    )
  }

  private def parsePattern(node: ujson.Value): py.ipattern = {
    kind(node) match {
      case "MatchValue" =>
        py.MatchValue(parseExpr(child(node, "value")), attr(node))
      case "MatchSingleton" =>
        py.MatchSingleton(parseConstant(node), attr(node))
      case "MatchSequence" =>
        py.MatchSequence(coll(children(node, "patterns").map(parsePattern)), attr(node))
      case "MatchMapping" =>
        py.MatchMapping(
          coll(children(node, "keys").map(parseExpr)),
          coll(children(node, "patterns").map(parsePattern)),
          optStrProp(node, "rest"),
          attr(node)
        )
      case "MatchClass" =>
        py.MatchClass(
          parseExpr(child(node, "cls")),
          coll(children(node, "patterns").map(parsePattern)),
          coll(stringArrayProp(node, "kwd_attrs")),
          coll(children(node, "kwd_patterns").map(parsePattern)),
          attr(node)
        )
      case "MatchStar" =>
        py.MatchStar(optStrProp(node, "name"), attr(node))
      case "MatchAs" =>
        py.MatchAs(childOpt(node, "pattern").map(parsePattern), optStrProp(node, "name"), attr(node))
      case "MatchOr" =>
        py.MatchOr(coll(children(node, "patterns").map(parsePattern)), attr(node))
      case other =>
        unsupported(node, s"pattern kind '$other'")
    }
  }

  private def parseComprehension(node: ujson.Value): py.Comprehension = {
    expectKind(node, "Comprehension")
    py.Comprehension(
      parseExpr(child(node, "target")),
      parseExpr(child(node, "iter")),
      coll(children(node, "ifs").map(parseExpr)),
      boolProp(node, "is_async", default = false)
    )
  }

  private def parseExceptHandler(node: ujson.Value): py.ExceptHandler = {
    expectKind(node, "ExceptHandler")
    py.ExceptHandler(
      childOpt(node, "type").map(parseExpr),
      optStrProp(node, "name"),
      coll(children(node, "body").map(parseStmt)),
      attr(node)
    )
  }

  private def parseTypeParam(node: ujson.Value): py.itypeParam = {
    kind(node) match {
      case "TypeVar" =>
        py.TypeVar(strProp(node, "name"), childOpt(node, "bound").map(parseExpr), attr(node))
      case "ParamSpec" =>
        py.ParamSpec(strProp(node, "name"), attr(node))
      case "TypeVarTuple" =>
        py.TypeVarTuple(strProp(node, "name"), attr(node))
      case other =>
        unsupported(node, s"type parameter kind '$other'")
    }
  }

  private def parseConstant(node: ujson.Value): py.iconstant = {
    strProp(node, "value_kind") match {
      case "None" =>
        py.NoneConstant
      case "Bool" =>
        py.BoolConstant(prop(node, "value").exists(_.bool))
      case "Str" =>
        val (quote, prefix, rawValue) = stringParts(node)
        py.StringConstant(rawValue, quote, prefix)
      case "Bytes" =>
        val (quote, prefix, rawValue) = stringParts(node)
        py.StringConstant(rawValue, quote, prefix)
      case "Int" =>
        py.IntConstant(text(node).getOrElse(strProp(node, "value")))
      case "Float" =>
        py.FloatConstant(text(node).getOrElse(strProp(node, "value")))
      case "Complex" =>
        py.ImaginaryConstant(text(node).getOrElse("0j").stripSuffix("j").stripSuffix("J"))
      case "Ellipsis" =>
        py.EllipsisConstant
      case other =>
        unsupported(node, s"constant kind '$other'")
    }
  }

  private def parseStringExpressionList(node: ujson.Value): Option[py.StringExpList] = {
    Option
      .when(optStrProp(node, "value_kind").contains("Str")) {
        text(node).flatMap(adjacentStringLiteralParts)
      }
      .flatten
      .filter(_.size >= 2)
      .map { parts =>
        val expressions = parts.map { case StringLiteralPart(prefix, quote, value) =>
          py.Constant(py.StringConstant(value, quote, prefix), attr(node)): py.iexpr
        }
        py.StringExpList(coll(expressions), attr(node))
      }
  }

  private def parseBoolOp(value: String): py.iboolop = value match {
    case "And" => py.And
    case "Or"  => py.Or
    case other => throw new UnsupportedOperationException(s"Unsupported Python bool operator '$other'")
  }

  private def parseOperator(value: String): py.ioperator = value match {
    case "Add"      => py.Add
    case "Sub"      => py.Sub
    case "Mult"     => py.Mult
    case "MatMult"  => py.MatMult
    case "Div"      => py.Div
    case "Mod"      => py.Mod
    case "Pow"      => py.Pow
    case "LShift"   => py.LShift
    case "RShift"   => py.RShift
    case "BitOr"    => py.BitOr
    case "BitXor"   => py.BitXor
    case "BitAnd"   => py.BitAnd
    case "FloorDiv" => py.FloorDiv
    case other      => throw new UnsupportedOperationException(s"Unsupported Python operator '$other'")
  }

  private def parseUnaryOp(value: String): py.iunaryop = value match {
    case "Invert" => py.Invert
    case "Not"    => py.Not
    case "UAdd"   => py.UAdd
    case "USub"   => py.USub
    case other    => throw new UnsupportedOperationException(s"Unsupported Python unary operator '$other'")
  }

  private def parseCompOp(value: String): py.icompop = value match {
    case "Eq"    => py.Eq
    case "NotEq" => py.NotEq
    case "Lt"    => py.Lt
    case "LtE"   => py.LtE
    case "Gt"    => py.Gt
    case "GtE"   => py.GtE
    case "Is"    => py.Is
    case "IsNot" => py.IsNot
    case "In"    => py.In
    case "NotIn" => py.NotIn
    case other   => throw new UnsupportedOperationException(s"Unsupported Python comparison operator '$other'")
  }

  private def parseConversion(value: Option[String]): Int = value match {
    case None | Some("None") => -1
    case Some("Str")         => 115
    case Some("Repr")        => 114
    case Some("Ascii")       => 97
    case Some(other) => throw new UnsupportedOperationException(s"Unsupported Python f-string conversion '$other'")
  }

  private def attr(node: ujson.Value): AttributeProvider = {
    node.obj.get("range") match {
      case Some(range) =>
        JsonAttributeProvider(
          intField(range, "start_line"),
          intField(range, "start_column"),
          intField(range, "start_offset"),
          intField(range, "end_line"),
          intField(range, "end_column"),
          intField(range, "end_offset")
        )
      case None =>
        fallbackAttributeProvider
    }
  }

  private def relativeFileName(fullPath: String, inputRoot: Path, jsonPath: Path): String = {
    val root = inputRoot.toAbsolutePath.normalize()
    val path = Try(Paths.get(fullPath).toAbsolutePath.normalize()).toOption

    path match {
      case Some(value) if Files.isRegularFile(root) && value == root =>
        root.getFileName.toString
      case Some(value) if value.startsWith(root) && value != root =>
        root.relativize(value).toString
      case Some(value) =>
        value.getFileName.toString
      case None =>
        jsonPath.getFileName.toString.stripSuffix(".json")
    }
  }

  private def stringParts(node: ujson.Value): (String, String, String) = {
    val rawText    = text(node).getOrElse(prop(node, "value").map(_.render()).getOrElse(""))
    val quoteStart = rawText.indexWhere(ch => ch == '\'' || ch == '"')
    if (quoteStart < 0) {
      ("\"", "", prop(node, "value").map(_.str).getOrElse(rawText))
    } else {
      val prefix         = rawText.take(quoteStart)
      val quoteChar      = rawText.charAt(quoteStart)
      val tripleQuote    = List.fill(3)(quoteChar).mkString
      val remaining      = rawText.drop(quoteStart)
      val quote          = if (remaining.startsWith(tripleQuote)) tripleQuote else quoteChar.toString
      val valueStart     = (quoteStart + quote.length).min(rawText.length)
      val valueEnd       = (rawText.length - quote.length).max(valueStart)
      val rawInnerString = rawText.substring(valueStart, valueEnd)
      (quote, prefix, rawInnerString)
    }
  }

  private final case class StringLiteralPart(prefix: String, quote: String, value: String)

  private def adjacentStringLiteralParts(rawText: String): Option[Seq[StringLiteralPart]] = {
    val parts = mutable.ArrayBuffer.empty[StringLiteralPart]
    var index = 0
    while (index < rawText.length) {
      index = skipWhitespace(rawText, index)
      if (index >= rawText.length) {
        return Some(parts.toSeq)
      }

      readStringLiteralPart(rawText, index) match {
        case Some((part, nextIndex)) =>
          if (part.prefix.toLowerCase.contains('f')) {
            return None
          }
          parts += part
          index = nextIndex
        case None =>
          return None
      }
    }
    Some(parts.toSeq)
  }

  private def readStringLiteralPart(rawText: String, start: Int): Option[(StringLiteralPart, Int)] = {
    var index = start
    while (index < rawText.length && isStringPrefixChar(rawText.charAt(index))) {
      index += 1
    }
    if (index >= rawText.length || !isQuote(rawText.charAt(index))) {
      return None
    }

    val quoteChar = rawText.charAt(index)
    val tripleQuote = index + 2 < rawText.length &&
      rawText.charAt(index + 1) == quoteChar &&
      rawText.charAt(index + 2) == quoteChar
    val quoteLength  = if (tripleQuote) 3 else 1
    val contentStart = index + quoteLength
    var cursor       = contentStart
    var closed       = false

    while (cursor < rawText.length && !closed) {
      if (rawText.charAt(cursor) == '\\') {
        cursor = (cursor + 2).min(rawText.length)
      } else if (startsWithQuote(rawText, cursor, quoteChar, quoteLength)) {
        closed = true
      } else {
        cursor += 1
      }
    }

    Option.when(closed) {
      val contentEnd = cursor
      val end        = cursor + quoteLength
      val prefix     = rawText.substring(start, index)
      val quote      = rawText.substring(index, contentStart)
      val value      = rawText.substring(contentStart, contentEnd)
      StringLiteralPart(prefix, quote, value) -> end
    }
  }

  private def skipWhitespace(value: String, index: Int): Int = {
    var cursor = index
    while (cursor < value.length && value.charAt(cursor).isWhitespace) {
      cursor += 1
    }
    cursor
  }

  private def isStringPrefixChar(ch: Char): Boolean = {
    ch match {
      case 'r' | 'R' | 'u' | 'U' | 'b' | 'B' | 'f' | 'F' => true
      case _                                             => false
    }
  }

  private def isQuote(ch: Char): Boolean = ch == '\'' || ch == '"'

  private def startsWithQuote(value: String, index: Int, quoteChar: Char, quoteLength: Int): Boolean = {
    index + quoteLength <= value.length &&
    value.substring(index, index + quoteLength).forall(_ == quoteChar)
  }

  private def expectKind(node: ujson.Value, expected: String): Unit = {
    val actual = kind(node)
    if (actual != expected) {
      unsupported(node, s"kind '$actual', expected '$expected'")
    }
  }

  private def kind(node: ujson.Value): String = node("kind").str

  private def text(node: ujson.Value): Option[String] = node.obj.get("text").map(_.str)

  private def children(node: ujson.Value, name: String): Seq[ujson.Value] = {
    node.obj.get("children").flatMap(_.obj.get(name)).map(_.arr.toSeq).getOrElse(Seq.empty)
  }

  private def child(node: ujson.Value, name: String): ujson.Value = {
    childOpt(node, name).getOrElse(unsupported(node, s"missing child '$name'"))
  }

  private def childOpt(node: ujson.Value, name: String): Option[ujson.Value] = children(node, name).headOption

  private def prop(node: ujson.Value, name: String): Option[ujson.Value] = {
    node.obj.get("properties").flatMap(_.obj.get(name))
  }

  private def strProp(node: ujson.Value, name: String): String = {
    optStrProp(node, name).getOrElse(unsupported(node, s"missing string property '$name'"))
  }

  private def optStrProp(node: ujson.Value, name: String): Option[String] = {
    prop(node, name).collect { case value if !value.isNull => value.str }
  }

  private def intProp(node: ujson.Value, name: String, default: Int): Int = {
    prop(node, name).map(_.num.toInt).getOrElse(default)
  }

  private def boolProp(node: ujson.Value, name: String, default: Boolean): Boolean = {
    prop(node, name).map(_.bool).getOrElse(default)
  }

  private def stringArrayProp(node: ujson.Value, name: String): Seq[String] = {
    prop(node, name).map(_.arr.map(_.str).toSeq).getOrElse(Seq.empty)
  }

  private def intField(node: ujson.Value, name: String): Int = node(name).num.toInt

  private def coll[T](values: Iterable[T]): mutable.Seq[T] = mutable.ArrayBuffer.from(values)

  private def unsupported[A](node: ujson.Value, message: String): A = {
    val where = text(node).map(value => s" near '$value'").getOrElse("")
    throw new UnsupportedOperationException(s"Unsupported pyastgen JSON $message$where")
  }
}
