import better.files.File
import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "php2cpg"

// Reference parser for the Rust differential harness
// (rust/crates/phpastgen-cli/tests/differential_json.rs, gated on
// PHPASTGEN_REFERENCE). It is the `joernio/PHP-Parser` fork published as the
// `php-parser.phar` archive below and installed by `phpParseInstallTask`. The
// phar is NOT a standalone binary: it runs through the system `php` interpreter
// (see PhpParser.scala), so the differential test needs PHP on PATH. Point
// PHPASTGEN_REFERENCE at this phar (or a native phpastgen-shaped binary).
val upstreamParserBinName  = "php-parser.phar"
val versionedParserBinName = s"php-parser-${Versions.phpParser}.phar"
val phpParserDlUrl =
  s"https://github.com/joernio/PHP-Parser/releases/download/v${Versions.phpParser}/$upstreamParserBinName"

lazy val PhpAstgenWin      = "phpastgen-win.exe"
lazy val PhpAstgenLinux    = "phpastgen-linux"
lazy val PhpAstgenLinuxArm = "phpastgen-linux-arm"
lazy val PhpAstgenMac      = "phpastgen-macos"
lazy val PhpAstgenMacArm   = "phpastgen-macos-arm"

dependsOn(
  Projects.dataflowengineoss  % "test->test",
  Projects.x2cpg              % "compile->compile;test->test",
  Projects.linterRules % ScalafixConfig
)

libraryDependencies ++= Seq(
  "com.lihaoyi"       %% "upickle"                % Versions.upickle,
  "com.lihaoyi"       %% "ujson"                  % Versions.upickle,
  "io.shiftleft"      %% "codepropertygraph"      % Versions.cpg,
  "com.github.sh4869" %% "semver-parser-scala"    % Versions.semverParser,
  "org.scalatest"     %% "scalatest"              % Versions.scalatest % Test,
  "com.github.albfernandez" % "juniversalchardet" % Versions.juniversalchardet
)

lazy val phpParseInstallTask = taskKey[Unit]("Install PHP-Parse using PHP Composer")
phpParseInstallTask := {
  val phpBinDir = baseDirectory.value / "bin" / "php-parser"
  DownloadHelper.ensureIsAvailable(phpParserDlUrl, phpBinDir / versionedParserBinName)
  File((phpBinDir / "php-parser.php").getPath)
    .createFileIfNotExists()
    .overwrite(s"<?php\nrequire('$versionedParserBinName');?>")

  val distDir = (Universal / stagingDirectory).value / "bin" / "php-parser"
  distDir.mkdirs()
  IO.copyDirectory(phpBinDir, distDir)
}

lazy val phpAstGenCurrentBinaryName = taskKey[String]("phpastgen binary name for the current host")
phpAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, _)                                => PhpAstgenWin
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => PhpAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => PhpAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => PhpAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => PhpAstgenMacArm
    case _                                                                           => PhpAstgenLinux
  }
}

lazy val phpAstGenBuildRust = taskKey[java.io.File]("Build local Rust phpastgen and install it under bin/php-parser")
phpAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "phpastgen.exe" else "phpastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "phpastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val phpBinDir   = baseDirectory.value / "bin" / "php-parser"
  val targetFile  = phpBinDir / phpAstGenCurrentBinaryName.value
  phpBinDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "php-parser"
  distDir.mkdirs()
  IO.copyDirectory(phpBinDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust phpastgen to $targetFile")
  targetFile
}

Compile / compile := ((Compile / compile) dependsOn phpAstGenBuildRust).value

enablePlugins(JavaAppPackaging, LauncherJarPlugin)
Global / onChangedBuildSource := ReloadOnSourceChanges

/** write the php parser version to the manifest for downstream usage */
Compile / packageBin / packageOptions +=
  Package.ManifestAttributes(new java.util.jar.Attributes.Name("PHP-Parser-Version") -> Versions.phpParser)
