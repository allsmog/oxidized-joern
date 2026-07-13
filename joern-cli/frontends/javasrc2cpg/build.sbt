import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "javasrc2cpg"

lazy val JavaAstgenWin      = "javaastgen-win.exe"
lazy val JavaAstgenWinArm   = "javaastgen-win-arm.exe"
lazy val JavaAstgenLinux    = "javaastgen-linux"
lazy val JavaAstgenLinuxArm = "javaastgen-linux-arm"
lazy val JavaAstgenMac      = "javaastgen-macos"
lazy val JavaAstgenMacArm   = "javaastgen-macos-arm"

dependsOn(
  Projects.dataflowengineoss  % "test->test",
  Projects.x2cpg              % "compile->compile;test->test",
  Projects.linterRules % ScalafixConfig
)

libraryDependencies ++= Seq(
  "io.shiftleft"           %% "codepropertygraph"             % Versions.cpg,
  "com.lihaoyi"            %% "ujson"                         % Versions.upickle,
  "com.github.javaparser"   % "javaparser-symbol-solver-core" % Versions.javaParser,
  "org.gradle"              % "gradle-tooling-api"            % Versions.gradleTooling,
  "org.scalatest"          %% "scalatest"                     % Versions.scalatest % Test,
  "org.projectlombok"       % "lombok"                        % Versions.lombok,
  "org.scala-lang.modules" %% "scala-parallel-collections"    % Versions.scalaParallel,
  "org.scala-lang.modules" %% "scala-parser-combinators"      % Versions.scalaParserCombinators,
  "net.lingala.zip4j"       % "zip4j"                         % Versions.zip4j,
  "org.ow2.asm"             % "asm"                           % Versions.asm
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)
trapExit                      := false
Global / onChangedBuildSource := ReloadOnSourceChanges

lazy val packTestCode = taskKey[Unit]("Packs test code for JarTypeReader into jars.")
packTestCode := {
  import better.files._
  import net.lingala.zip4j.ZipFile
  import net.lingala.zip4j.model.ZipParameters
  import net.lingala.zip4j.model.enums.{CompressionLevel, CompressionMethod}
  import java.nio.file.Paths

  val pkgRoot              = "io"
  val testClassOutputPath  = target.value / ("scala-" + scalaVersion.value) / "test-classes"
  val relativeTestCodePath = Paths.get(pkgRoot, "joern", "javasrc2cpg", "jartypereader", "testcode")

  val jarFileRoot = target.value.toScala / "testjars"
  if (jarFileRoot.exists()) jarFileRoot.delete()
  jarFileRoot.createDirectories()

  File(testClassOutputPath.toPath.resolve(relativeTestCodePath)).list.filter(_.exists).foreach { testDir =>
    val tmpDir                     = File.newTemporaryDirectory()
    val tmpDirWithCorrectPkgStruct = File(tmpDir.path.resolve(relativeTestCodePath)).createDirectoryIfNotExists()
    testDir.copyToDirectory(tmpDirWithCorrectPkgStruct)
    val testRootPath = tmpDir.path.resolve(pkgRoot)

    val jarFilePath = jarFileRoot / (testDir.name + ".jar")
    if (jarFilePath.exists()) jarFilePath.delete()
    val jarFile       = new ZipFile(jarFilePath.canonicalPath)
    val zipParameters = new ZipParameters()
    zipParameters.setCompressionMethod(CompressionMethod.DEFLATE)
    zipParameters.setCompressionLevel(CompressionLevel.NORMAL)
    zipParameters.setRootFolderNameInZip(relativeTestCodePath.toString)
    jarFile.addFolder(File(testRootPath).toJava)
  }
}
packTestCode := packTestCode.triggeredBy(Test / compile).value

lazy val javaAstGenCurrentBinaryName = taskKey[String]("javaastgen binary name for the current host")
javaAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.X86)   => JavaAstgenWin
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.ARMv8) => JavaAstgenWinArm
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)     => JavaAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8)   => JavaAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)       => JavaAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)     => JavaAstgenMacArm
    case _                                                                             => JavaAstgenLinux
  }
}

lazy val javaAstGenBuildRust = taskKey[File]("Build local Rust javaastgen and install it under bin/astgen")
javaAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "javaastgen.exe" else "javaastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "javaastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / javaAstGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust javaastgen to $targetFile")
  targetFile
}

Compile / compile := ((Compile / compile) dependsOn javaAstGenBuildRust).value
