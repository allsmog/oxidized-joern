# V4 runtime boundary — inspected without executing Java

The distributed joern script invokes bin/repl-bridge and supplies
conf/log4j2.xml. The bridge derives its lib/* classpath from the distribution,
reads conf/application.ini if present, and chooses JAVA_HOME/bin/java unless a
bundled-JVM override or command/config option changes that choice. The current
application.ini is absent; adding it changes the full tree inventory. These
script bytes are retained under runtime-provenance/ with the current log config.

runtime-bindings.json freezes all 266 files in the Joern distribution and all
527 regular or file-symlink entries in the selected JDK Home. This includes all
JARs, configs, launch scripts, JDK modules/native libraries, release identity and
Java executables. The selected Homebrew alias resolves to openjdk@21/21.0.12;
root and descendant symlink identities, target bytes and executable modes are
bound. Additions, deletions, mode changes and symlink retargets change the
snapshot. No symlink directories are present or accepted in the initial trees.
The before/after run snapshots include these complete runtime inventories even
for rejected, nonzero or timeout attempts. No installed runtime is copied or
modified by this preparation.

The launch PATH is fixed to the prepared value, beginning with the selected JDK
bin directory. Selected host launcher tools (env, sh/bash and the utilities used
by the wrapper) have resolved-file/mode/symlink bindings. Direct JVM, shell-startup
and loader override variables are currently empty or absent; V4 requires that
state and checks it again before and after execution. It does not log their
values. This prevents an inherited bundled JVM, startup script or injected Java
agent from silently replacing the selected runtime.

Read-only otool evidence for java, libjli and libjvm records the native loader
boundary. Core dependencies outside the selected JDK are macOS frameworks and
system libraries. The host OS identity and selected command files are recorded;
this is not a vendored or hermetic macOS image. No target executable, including
java -version or the Joern launcher, was executed to prepare this inventory.

V4 retains every prior V3 check and adds isolated runtime mutation scenarios.
All test mutations use temporary inert files. Actual Joern/JDK trees are only
read and hashed before and after the offline checks. Rejected attempts preserve
raw selected bytes as non-references and never publish new expected files.
