import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "jimple2cpg"

lazy val JimpleAstgenWin      = "jimpleastgen-win.exe"
lazy val JimpleAstgenWinArm   = "jimpleastgen-win-arm.exe"
lazy val JimpleAstgenLinux    = "jimpleastgen-linux"
lazy val JimpleAstgenLinuxArm = "jimpleastgen-linux-arm"
lazy val JimpleAstgenMac      = "jimpleastgen-macos"
lazy val JimpleAstgenMacArm   = "jimpleastgen-macos-arm"

dependsOn(
  Projects.dataflowengineoss  % "test->test",
  Projects.x2cpg              % "compile->compile;test->test",
  Projects.linterRules % ScalafixConfig
)

libraryDependencies ++= Seq(
  "io.shiftleft"  %% "codepropertygraph" % Versions.cpg,
  "org.soot-oss"   % "soot"              % Versions.soot,
  "org.typelevel" %% "cats-core"         % Versions.catsCore,
  "com.lihaoyi"   %% "ujson"             % Versions.upickle,
  "org.scalatest" %% "scalatest"         % Versions.scalatest % Test,
  "org.benf"       % "cfr"               % Versions.cfr
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)
trapExit    := false
Test / fork := true

lazy val jimpleAstGenCurrentBinaryName = taskKey[String]("jimpleastgen binary name for the current host")
jimpleAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.X86)   => JimpleAstgenWin
    case (Environment.OperatingSystemType.Windows, Environment.ArchitectureType.ARMv8) => JimpleAstgenWinArm
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)     => JimpleAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8)   => JimpleAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)       => JimpleAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)     => JimpleAstgenMacArm
    case _                                                                             => JimpleAstgenLinux
  }
}

lazy val jimpleAstGenBuildRust = taskKey[File]("Build local Rust jimpleastgen and install it under bin/astgen")
jimpleAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "jimpleastgen.exe" else "jimpleastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "jimpleastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / jimpleAstGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust jimpleastgen to $targetFile")
  targetFile
}

Compile / compile := ((Compile / compile) dependsOn jimpleAstGenBuildRust).value
