import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "kotlin2cpg"

val kotlinVersion = "2.3.21"

lazy val KotlinAstgenWin      = "kotlinastgen-win.exe"
lazy val KotlinAstgenWinArm   = "kotlinastgen-win-arm.exe"
lazy val KotlinAstgenLinux    = "kotlinastgen-linux"
lazy val KotlinAstgenLinuxArm = "kotlinastgen-linux-arm"
lazy val KotlinAstgenMac      = "kotlinastgen-macos"
lazy val KotlinAstgenMacArm   = "kotlinastgen-macos-arm"

dependsOn(
  Projects.dataflowengineoss  % "test->test",
  Projects.x2cpg              % "compile->compile;test->test",
  Projects.javasrc2cpg        % "compile->compile;test->test",
  Projects.linterRules % ScalafixConfig
)

libraryDependencies ++= Seq(
  "com.lihaoyi"             %% "requests"                   % Versions.requests,
  "com.lihaoyi"             %% "ujson"                      % Versions.upickle,
  "com.squareup.tools.build" % "maven-archeologist"         % Versions.mavenArcheologist,
  "io.shiftleft"            %% "codepropertygraph"          % Versions.cpg,
  "org.gradle"               % "gradle-tooling-api"         % Versions.gradleTooling,
  "org.jetbrains.kotlin"     % "kotlin-stdlib-jdk8"         % kotlinVersion,
  "org.jetbrains.kotlin"     % "kotlin-stdlib"              % kotlinVersion,
  "org.jetbrains.kotlin"     % "kotlin-compiler-embeddable" % kotlinVersion,
  "org.jetbrains.kotlin"     % "kotlin-allopen"             % kotlinVersion,
  "org.scalatest"           %% "scalatest"                  % Versions.scalatest % Test
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)
trapExit    := false
Test / fork := true
Test / javaOptions ++= Seq("-ea")

lazy val kotlinAstGenCurrentBinaryName = taskKey[String]("kotlinastgen binary name for the current host")
kotlinAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.X86)   => KotlinAstgenWin
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.ARMv8) => KotlinAstgenWinArm
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)     => KotlinAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8)   => KotlinAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)       => KotlinAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)     => KotlinAstgenMacArm
    case _                                                                             => KotlinAstgenLinux
  }
}

lazy val kotlinAstGenBuildRust = taskKey[File]("Build local Rust kotlinastgen and install it under bin/astgen")
kotlinAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "kotlinastgen.exe"
    else "kotlinastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "kotlinastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / kotlinAstGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust kotlinastgen to $targetFile")
  targetFile
}

Compile / compile := ((Compile / compile) dependsOn kotlinAstGenBuildRust).value
