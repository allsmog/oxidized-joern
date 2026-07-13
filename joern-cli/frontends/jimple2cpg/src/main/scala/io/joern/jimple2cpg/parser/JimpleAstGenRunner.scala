package io.joern.jimple2cpg.parser

import io.joern.jimple2cpg.Config
import io.joern.jimple2cpg.util.ProgramHandlingUtil.ClassFile
import io.joern.x2cpg.astgen.AstGenRunner
import io.joern.x2cpg.astgen.AstGenRunner.{AstGenProgramMetaData, AstGenRunnerResult}
import io.shiftleft.semanticcpg.utils.ExternalCommand
import org.slf4j.LoggerFactory

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths}
import scala.jdk.CollectionConverters.*
import scala.util.Try

object JimpleAstGenRunner {

  final case class JimpleClassReference(internalName: String, fullyQualifiedName: String)

  final case class JimpleFieldInfo(
    name: String,
    descriptor: String,
    typeName: Option[String],
    accessFlags: Int,
    accessFlagsText: List[String],
    signature: Option[String],
    constantValue: Option[String]
  )

  final case class JimpleExceptionHandlerInfo(
    startPc: Int,
    endPc: Int,
    handlerPc: Int,
    catchType: Option[JimpleClassReference]
  )

  final case class JimpleLineNumberInfo(startPc: Int, lineNumber: Int)

  final case class JimpleResolvedConstantPoolInfo(
    tag: String,
    classReference: Option[JimpleClassReference],
    name: Option[String],
    descriptor: Option[String],
    fieldType: Option[String],
    parameterTypes: List[String],
    returnType: Option[String],
    value: Option[String],
    referenceKind: Option[Int],
    referenceKindText: Option[String],
    referenceIndex: Option[Int],
    bootstrapMethodAttrIndex: Option[Int]
  )

  final case class JimpleBytecodeOperandInfo(
    name: String,
    kind: String,
    value: Int,
    resolved: Option[JimpleResolvedConstantPoolInfo]
  )

  final case class JimpleBytecodeInstructionInfo(
    offset: Long,
    opcode: Int,
    mnemonic: String,
    operands: List[JimpleBytecodeOperandInfo]
  )

  final case class JimpleMethodBodyIrInfo(
    offset: Long,
    operation: String,
    code: String,
    result: Option[String],
    target: Option[String],
    methodFullName: Option[String],
    signature: Option[String],
    dispatchType: Option[String],
    receiver: Option[String],
    targets: List[Long],
    arguments: List[String],
    bootstrapArguments: List[String]
  )

  final case class JimpleLocalVariableInfo(
    startPc: Int,
    length: Int,
    name: String,
    descriptor: String,
    typeName: Option[String],
    signature: Option[String],
    index: Int
  )

  final case class JimpleMethodCodeInfo(
    maxStack: Int,
    maxLocals: Int,
    bytecodeLength: Long,
    instructions: List[JimpleBytecodeInstructionInfo],
    bodyIr: List[JimpleMethodBodyIrInfo],
    exceptionTable: List[JimpleExceptionHandlerInfo],
    lineNumbers: List[JimpleLineNumberInfo],
    localVariables: List[JimpleLocalVariableInfo]
  )

  final case class JimpleMethodInfo(
    name: String,
    descriptor: String,
    parameterTypes: List[String],
    returnType: Option[String],
    accessFlags: Int,
    accessFlagsText: List[String],
    signature: Option[String],
    exceptions: List[JimpleClassReference],
    code: Option[JimpleMethodCodeInfo]
  )

  final case class JimpleClassInfo(
    sourcePath: String,
    outputPath: Path,
    internalName: String,
    fullyQualifiedName: String,
    superInternalName: Option[String],
    superFullyQualifiedName: Option[String],
    interfaces: List[JimpleClassReference],
    minorVersion: Int,
    majorVersion: Int,
    accessFlags: Int,
    accessFlagsText: List[String],
    sourceFile: Option[String],
    signature: Option[String],
    fields: List[JimpleFieldInfo],
    methods: List[JimpleMethodInfo],
    byteLength: Long
  )

  final case class JimpleAstGenRunnerResult(
    parsedFiles: List[String] = List.empty,
    skippedFiles: List[String] = List.empty,
    classFiles: List[ClassFile] = List.empty,
    classInfo: List[JimpleClassInfo] = List.empty
  ) extends AstGenRunnerResult

  private object astGenMetaData
      extends AstGenProgramMetaData(
        name = "jimpleastgen",
        configPrefix = "jimple2cpg",
        binEnvVar = Option("JIMPLEASTGEN_BIN"),
        versionFlag = "-version"
      )
}

class JimpleAstGenRunner(config: Config) extends AstGenRunner(JimpleAstGenRunner.astGenMetaData, config) {

  import JimpleAstGenRunner.*

  private val logger = LoggerFactory.getLogger(getClass)

  override def execute(out: Path): JimpleAstGenRunnerResult = {
    runAstGenNative(config.inputPath, out, config.ignoredFilesRegex.toString(), "") match {
      case scala.util.Success(_) =>
        val manifestPath = out.resolve("manifest.json")
        if (!Files.isRegularFile(manifestPath)) {
          logger.error(s"jimpleastgen did not create expected manifest at '$manifestPath'")
          JimpleAstGenRunnerResult()
        } else {
          val manifest  = ujson.read(Files.readString(manifestPath, StandardCharsets.UTF_8))
          val classInfo = parseClassInfo(manifest)
          val classes   = classInfo.map(info => ClassFile(info.outputPath, Option(info.internalName)))
          val skipped   = parseSkippedFiles(manifest)
          skipped.foreach(path => logger.warn(s"\t- jimpleastgen skipped '$path'"))
          JimpleAstGenRunnerResult(
            parsedFiles = classes.map(_.file.toString),
            skippedFiles = skipped,
            classFiles = classes,
            classInfo = classInfo
          )
        }
      case scala.util.Failure(f) =>
        logger.error(s"\t- running jimpleastgen failed!", f)
        JimpleAstGenRunnerResult()
    }
  }

  override protected def runAstGenNative(in: String, out: Path, exclude: String, include: String): Try[Seq[String]] = {
    val recurseArgs = Option.when(config.recurse)("-recurse").toSeq
    val depthArgs   = Seq("-depth", config.depth.toString)
    val excludeArgs = Option(exclude).filter(_.nonEmpty).toSeq.flatMap(regex => Seq("-exclude", regex))
    val args        = Seq(astGenCommand, "-out", out.toString) ++ recurseArgs ++ depthArgs ++ excludeArgs ++ Seq(in)
    ExternalCommand.run(args).toTry
  }

  override protected def skippedFiles(in: Path, astGenOut: List[String]): List[String] = List.empty

  private def parseClassInfo(manifest: ujson.Value): List[JimpleClassInfo] = {
    manifest("classes").arr.toList.map { entry =>
      JimpleClassInfo(
        sourcePath = entry("source_path").str,
        outputPath = Paths.get(entry("output_path").str),
        internalName = entry("internal_name").str,
        fullyQualifiedName = entry("fully_qualified_name").str,
        superInternalName = optionalString(entry, "super_internal_name"),
        superFullyQualifiedName = optionalString(entry, "super_fully_qualified_name"),
        interfaces = arrayValues(entry, "interfaces").map(parseClassReference),
        minorVersion = intField(entry, "minor_version"),
        majorVersion = intField(entry, "major_version"),
        accessFlags = intField(entry, "access_flags"),
        accessFlagsText = stringArray(entry, "access_flags_text"),
        sourceFile = optionalString(entry, "source_file"),
        signature = optionalString(entry, "signature"),
        fields = arrayValues(entry, "fields").map(parseFieldInfo),
        methods = arrayValues(entry, "methods").map(parseMethodInfo),
        byteLength = longField(entry, "byte_length")
      )
    }
  }

  private def parseClassReference(entry: ujson.Value): JimpleClassReference =
    JimpleClassReference(
      internalName = entry("internal_name").str,
      fullyQualifiedName = entry("fully_qualified_name").str
    )

  private def parseFieldInfo(entry: ujson.Value): JimpleFieldInfo =
    JimpleFieldInfo(
      name = entry("name").str,
      descriptor = entry("descriptor").str,
      typeName = optionalString(entry, "type_name"),
      accessFlags = intField(entry, "access_flags"),
      accessFlagsText = stringArray(entry, "access_flags_text"),
      signature = optionalString(entry, "signature"),
      constantValue = optionalString(entry, "constant_value")
    )

  private def parseMethodInfo(entry: ujson.Value): JimpleMethodInfo =
    JimpleMethodInfo(
      name = entry("name").str,
      descriptor = entry("descriptor").str,
      parameterTypes = stringArray(entry, "parameter_types"),
      returnType = optionalString(entry, "return_type"),
      accessFlags = intField(entry, "access_flags"),
      accessFlagsText = stringArray(entry, "access_flags_text"),
      signature = optionalString(entry, "signature"),
      exceptions = arrayValues(entry, "exceptions").map(parseClassReference),
      code = optionalValue(entry, "code").map(parseMethodCodeInfo)
    )

  private def parseMethodCodeInfo(entry: ujson.Value): JimpleMethodCodeInfo =
    JimpleMethodCodeInfo(
      maxStack = intField(entry, "max_stack"),
      maxLocals = intField(entry, "max_locals"),
      bytecodeLength = longField(entry, "bytecode_length"),
      instructions = arrayValues(entry, "instructions").map(parseBytecodeInstructionInfo),
      bodyIr = arrayValues(entry, "body_ir").map(parseMethodBodyIrInfo),
      exceptionTable = arrayValues(entry, "exception_table").map(parseExceptionHandlerInfo),
      lineNumbers = arrayValues(entry, "line_numbers").map(parseLineNumberInfo),
      localVariables = arrayValues(entry, "local_variables").map(parseLocalVariableInfo)
    )

  private def parseExceptionHandlerInfo(entry: ujson.Value): JimpleExceptionHandlerInfo =
    JimpleExceptionHandlerInfo(
      startPc = intField(entry, "start_pc"),
      endPc = intField(entry, "end_pc"),
      handlerPc = intField(entry, "handler_pc"),
      catchType = optionalValue(entry, "catch_type").map(parseClassReference)
    )

  private def parseLineNumberInfo(entry: ujson.Value): JimpleLineNumberInfo =
    JimpleLineNumberInfo(startPc = intField(entry, "start_pc"), lineNumber = intField(entry, "line_number"))

  private def parseBytecodeInstructionInfo(entry: ujson.Value): JimpleBytecodeInstructionInfo =
    JimpleBytecodeInstructionInfo(
      offset = longField(entry, "offset"),
      opcode = intField(entry, "opcode"),
      mnemonic = entry("mnemonic").str,
      operands = arrayValues(entry, "operands").map(parseBytecodeOperandInfo)
    )

  private def parseMethodBodyIrInfo(entry: ujson.Value): JimpleMethodBodyIrInfo =
    JimpleMethodBodyIrInfo(
      offset = longField(entry, "offset"),
      operation = entry("operation").str,
      code = entry("code").str,
      result = optionalString(entry, "result"),
      target = optionalString(entry, "target"),
      methodFullName = optionalString(entry, "method_full_name"),
      signature = optionalString(entry, "signature"),
      dispatchType = optionalString(entry, "dispatch_type"),
      receiver = optionalString(entry, "receiver"),
      targets = arrayValues(entry, "targets").map(_.num.toLong),
      arguments = stringArray(entry, "arguments"),
      bootstrapArguments = stringArray(entry, "bootstrap_arguments")
    )

  private def parseBytecodeOperandInfo(entry: ujson.Value): JimpleBytecodeOperandInfo =
    JimpleBytecodeOperandInfo(
      name = entry("name").str,
      kind = entry("kind").str,
      value = intField(entry, "value"),
      resolved = optionalValue(entry, "resolved").map(parseResolvedConstantPoolInfo)
    )

  private def parseResolvedConstantPoolInfo(entry: ujson.Value): JimpleResolvedConstantPoolInfo =
    JimpleResolvedConstantPoolInfo(
      tag = entry("tag").str,
      classReference = optionalValue(entry, "class").map(parseClassReference),
      name = optionalString(entry, "name"),
      descriptor = optionalString(entry, "descriptor"),
      fieldType = optionalString(entry, "field_type"),
      parameterTypes = stringArray(entry, "parameter_types"),
      returnType = optionalString(entry, "return_type"),
      value = optionalString(entry, "value"),
      referenceKind = optionalInt(entry, "reference_kind"),
      referenceKindText = optionalString(entry, "reference_kind_text"),
      referenceIndex = optionalInt(entry, "reference_index"),
      bootstrapMethodAttrIndex = optionalInt(entry, "bootstrap_method_attr_index")
    )

  private def parseLocalVariableInfo(entry: ujson.Value): JimpleLocalVariableInfo =
    JimpleLocalVariableInfo(
      startPc = intField(entry, "start_pc"),
      length = intField(entry, "length"),
      name = entry("name").str,
      descriptor = entry("descriptor").str,
      typeName = optionalString(entry, "type_name"),
      signature = optionalString(entry, "signature"),
      index = intField(entry, "index")
    )

  private def parseSkippedFiles(manifest: ujson.Value): List[String] = {
    manifest.obj
      .get("skipped")
      .toList
      .flatMap(_.arr.toList)
      .map(_("path").str)
  }

  private def arrayValues(entry: ujson.Value, key: String): List[ujson.Value] =
    entry.obj.get(key).toList.flatMap(_.arr.toList)

  private def stringArray(entry: ujson.Value, key: String): List[String] =
    arrayValues(entry, key).map(_.str)

  private def optionalString(entry: ujson.Value, key: String): Option[String] =
    entry.obj.get(key).collect { case ujson.Str(value) => value }

  private def optionalValue(entry: ujson.Value, key: String): Option[ujson.Value] =
    entry.obj.get(key).filterNot(_ == ujson.Null)

  private def optionalInt(entry: ujson.Value, key: String): Option[Int] =
    entry.obj.get(key).collect { case ujson.Num(value) => value.toInt }

  private def intField(entry: ujson.Value, key: String): Int =
    entry.obj.get(key).fold(0)(_.num.toInt)

  private def longField(entry: ujson.Value, key: String): Long =
    entry.obj.get(key).fold(0L)(_.num.toLong)
}
