import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "c2cpg"

dependsOn(
  Projects.dataflowengineoss % "test->test",
  Projects.x2cpg             % "compile->compile;test->test",
  Projects.linterRules       % ScalafixConfig
)

libraryDependencies ++= Seq(
  "org.scala-lang.modules" %% "scala-parallel-collections" % Versions.scalaParallel,
  "org.eclipse.platform"    % "org.eclipse.core.resources" % Versions.eclipseCore,
  "org.eclipse.platform"    % "org.eclipse.text"           % Versions.eclipseText,
  // see note in readme re self-publishing cdt-core
  "io.joern"       % "eclipse-cdt-core" % Versions.eclipseCdt,
  "org.scalatest" %% "scalatest"        % Versions.scalatest % Test
)

dependencyOverrides ++= Seq(
  /* tl;dr; we'll stay on 2.19.0
   * Full story: if we upgrade to 2.20.0 we run into the following osgi error:
   *   Unknown error checking OSGI environment.
   *   java.lang.reflect.InvocationTargetException
   *     at java.base/jdk.internal.reflect.NativeMethodAccessorImpl.invoke0(Native Method)
   *     at java.base/jdk.internal.reflect.NativeMethodAccessorImpl.invoke(NativeMethodAccessorImpl.java:77)
   *     at java.base/jdk.internal.reflect.DelegatingMethodAccessorImpl.invoke(DelegatingMethodAccessorImpl.java:43)
   *     at java.base/java.lang.reflect.Method.invoke(Method.java:568)
   *     at org.apache.logging.log4j.util.OsgiServiceLocator.checkOsgiAvailable(OsgiServiceLocator.java:39)
   *   ...
   *   Caused by: java.lang.NullPointerException: Cannot invoke "org.osgi.framework.BundleContext.getBundles()" because "context" is null
   *     at com.diffplug.spotless.extra.eclipse.base.osgi.SimpleBundle.<init>(SimpleBundle.java:57)
   *     at com.diffplug.spotless.extra.eclipse.base.osgi.SimpleBundle.<init>(SimpleBundle.java:49)
   *     at com.diffplug.spotless.extra.eclipse.base.osgi.FrameworkBundleRegistry.getBundle(FrameworkBundleRegistry.java:47)
   *     at org.osgi.framework.FrameworkUtil.lambda$5(FrameworkUtil.java:234)
   */
  "org.apache.logging.log4j" % "log4j-core"        % "2.19.0" % Optional,
  "org.apache.logging.log4j" % "log4j-slf4j2-impl" % "2.19.0" % Optional
)

Compile / doc / scalacOptions ++= Seq("-doc-title", "semanticcpg apidocs", "-doc-version", version.value)

compile / javacOptions ++= Seq("-Xlint:all", "-Xlint:-cast", "-g")
Test / fork := true

enablePlugins(JavaAppPackaging, LauncherJarPlugin)

lazy val CxxAstgenWin      = "cxxastgen-win.exe"
lazy val CxxAstgenLinux    = "cxxastgen-linux"
lazy val CxxAstgenLinuxArm = "cxxastgen-linux-arm64"
lazy val CxxAstgenMac      = "cxxastgen-macos"
lazy val CxxAstgenMacArm   = "cxxastgen-macos-arm64"

lazy val cxxAstGenCurrentBinaryName = taskKey[String]("cxxastgen binary name for the current host")
cxxAstGenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, _)                                => CxxAstgenWin
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => CxxAstgenLinux
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => CxxAstgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => CxxAstgenMac
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => CxxAstgenMacArm
    case _                                                                           => CxxAstgenLinux
  }
}

lazy val cxxAstGenBuildRust = taskKey[File]("Build local Rust cxxastgen and install it under bin/astgen")
cxxAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "cxxastgen.exe" else "cxxastgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "cxxastgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / cxxAstGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)

  streams.value.log.info(s"installed Rust cxxastgen to $targetFile")
  targetFile
}

Compile / compile := ((Compile / compile) dependsOn cxxAstGenBuildRust).value

Universal / packageName       := name.value
Universal / topLevelDirectory := None
