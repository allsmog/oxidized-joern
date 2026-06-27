import com.typesafe.config.{Config, ConfigFactory}
import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "rubysrc2cpg"

dependsOn(
  Projects.dataflowengineoss  % "test->test",
  Projects.x2cpg              % "compile->compile;test->test",
  Projects.linterRules % ScalafixConfig
)

lazy val appProperties = settingKey[Config]("App Properties")
appProperties := {
  val path            = (Compile / resourceDirectory).value / "application.conf"
  val applicationConf = ConfigFactory.parseFile(path).resolve()
  applicationConf
}

lazy val joernTypeStubsVersion = settingKey[String]("joern_type_stub version")
joernTypeStubsVersion := appProperties.value.getString("rubysrc2cpg.joern_type_stubs_version")

libraryDependencies ++= Seq(
  "io.shiftleft"        %% "codepropertygraph" % Versions.cpg,
  "org.apache.commons" % "commons-compress" % Versions.commonsCompress, // For unpacking Gems with `--download-dependencies`
  "org.scalatest"      %% "scalatest"         % Versions.scalatest % Test
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)

lazy val astGenVersion = settingKey[String]("rubyastgen version")
astGenVersion := appProperties.value.getString("rubysrc2cpg.rubyastgen_version")

// Differential reference (rust/crates/rubyastgen-cli/tests/differential_json.rs,
// gated on RUBYASTGEN_REFERENCE): the upstream reference is `ruby_ast_gen`
// (github.com/joernio/astgen-monorepo, `ruby-astgen`), a Ruby gem wrapped around
// the `parser` gem. It is NOT a standalone native binary — it runs through JRuby
// after being embedded under src/main/resources, and is published as a packaged
// gem ZIP, not an executable. It was historically fetched via:
//   https://github.com/joernio/astgen-monorepo/releases/download/ruby-astgen/v<ver>/ruby_ast_gen_v<ver>.zip
// (unzipped into resources by an `astGenResourceTask`). The oxidized track builds
// the Rust `rubyastgen` binary instead (rubyAstGenBuildRust below), so no gem
// download task is wired in here. To run the differential test, point
// RUBYASTGEN_REFERENCE at a `ruby_ast_gen` launcher (needs Ruby/JRuby) or another
// `rubyastgen` revision honouring the positional `<input> <output>` interface.

lazy val RubyAstgenWin      = "rubyastgen-win.exe"
lazy val RubyAstgenLinux    = "rubyastgen-linux"
lazy val RubyAstgenLinuxArm = "rubyastgen-linux-arm"
lazy val RubyAstgenMac      = "rubyastgen-macos"
lazy val RubyAstgenMacArm   = "rubyastgen-macos-arm"

lazy val rubyAstGenCurrentBinaryName = taskKey[String]("rubyastgen binary name for the current host")
rubyAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, _)                                => RubyAstgenWin
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => RubyAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => RubyAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => RubyAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => RubyAstgenMacArm
    case _                                                                           => RubyAstgenLinux
  }
}

lazy val rubyAstGenBuildRust = taskKey[File]("Build local Rust rubyastgen and install it under bin/astgen")
rubyAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "rubyastgen.exe" else "rubyastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "rubyastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / rubyAstGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust rubyastgen to $targetFile")
  targetFile
}

lazy val joernTypeStubsDlUrl = settingKey[String]("joern_type_stubs download url")
joernTypeStubsDlUrl := s"https://github.com/joernio/joern-type-stubs/releases/download/v${joernTypeStubsVersion.value}/"

lazy val joernTypeStubsDlTask = taskKey[Unit]("Download joern-type-stubs")
joernTypeStubsDlTask := {
  val joernTypeStubsDir = baseDirectory.value / "type_stubs"
  val fileName          = "rubysrc_builtin_types.zip"
  val shaFileName       = s"$fileName.sha512"

  joernTypeStubsDir.mkdir()

  DownloadHelper.ensureIsAvailable(s"${joernTypeStubsDlUrl.value}$fileName", joernTypeStubsDir / fileName)
  DownloadHelper.ensureIsAvailable(s"${joernTypeStubsDlUrl.value}$shaFileName", joernTypeStubsDir / shaFileName)

  val typeStubsFile = better.files.File(joernTypeStubsDir.getAbsolutePath) / fileName
  val checksumFile  = better.files.File(joernTypeStubsDir.getAbsolutePath) / shaFileName

  val typestubsSha = typeStubsFile.sha512

  // Checksum file must contain exactly 1 line, if more or less we automatically fail.
  if (checksumFile.lineIterator.size != 1) {
    throw new IllegalStateException("Checksum File should only have 1 line")
  }

  // Checksum from terminal adds the filename to the line, so we split on whitespace to get the checksum
  // separate from the filename
  if (checksumFile.lineIterator.next().split("\\s+")(0).toUpperCase != typestubsSha) {
    throw new Exception("Checksums do not match for type stubs!")
  }

  val distDir = (Universal / stagingDirectory).value / "type_stubs"
  distDir.mkdirs()
  IO.copyDirectory(joernTypeStubsDir, distDir)
}

Compile / compile := ((Compile / compile) dependsOn (joernTypeStubsDlTask, rubyAstGenBuildRust)).value

Universal / packageName       := name.value
Universal / topLevelDirectory := None

/** write the astgen version to the manifest for downstream usage */
Compile / packageBin / packageOptions +=
  Package.ManifestAttributes(new java.util.jar.Attributes.Name("Ruby-AstGen-Version") -> astGenVersion.value)
