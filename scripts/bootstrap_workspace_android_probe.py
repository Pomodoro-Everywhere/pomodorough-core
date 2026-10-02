"""Compile the complete production projection factory, dispatcher, and models."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ANDROID = ROOT.parent / "android/app/src/main/java/me/egigoka/pomodorough/data"


def section(path, start, end):
    source = path.read_text()
    begin = source.index(start)
    return source[begin:source.index(end, begin)]


def android(rows, temporary, bridge):
    compiler = Path(os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc"))
    libraries = compiler.parent.parent / "lib"
    gradle = Path.home() / ".gradle/wrapper/dists"
    serialization = [next(gradle.glob(f"**/{name}-1.6.2.jar")) for name in
                     ("kotlinx-serialization-json-jvm", "kotlinx-serialization-core-jvm")]
    classpath = os.pathsep.join(map(str, [*serialization, libraries / "kotlinx-coroutines-core-jvm.jar"]))
    program = (ROOT / "scripts/bootstrap_workspace_android.kt").read_text()
    methods = section(ANDROID / "TimerRepository.kt", "    private fun visibleHistoryCount(", "    private fun accountNetworkBlocked(")
    pending = section(ANDROID / "TimerSyncConstruction.kt", "internal data class PendingSyncQueues(", "internal data class SentSyncIds(")
    program = program.replace("// INSERT_METHODS", methods).replace("// INSERT_PENDING_MODEL", pending)
    source = temporary / "Probe.kt"
    source.write_text(program)
    jar = temporary / "probe.jar"
    env = dict(os.environ, JAVA_HOME=os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    bounds = temporary / "SyncWireBounds.kt"
    bounds.write_text("package me.egigoka.pomodorough.data\ninternal object SyncWireBounds {\n" +
                      section(ANDROID / "SyncWireBounds.kt", "    fun compareUtf8(", "    fun mutationStamps(") + "}\n")
    sources = [source, bounds, ANDROID / "Models.kt", ANDROID / "CoreProjectionDispatcher.kt", ANDROID / "SynchronizedProjectionRequest.kt"]
    subprocess.run([str(compiler), *map(str, sources), "-cp", classpath, "-Xplugin=" + str(libraries / "kotlinx-serialization-compiler-plugin.jar"),
                    "-include-runtime", "-d", str(jar)], env=env, check=True, capture_output=True)
    import json
    result = subprocess.run([str(Path(env["JAVA_HOME"]) / "bin/java"), "-cp", str(jar) + os.pathsep + classpath,
                             "me.egigoka.pomodorough.data.ProbeKt", str(bridge)],
        input="".join(json.dumps(value) + "\n" for _, value in rows), text=True, capture_output=True, check=True)
    return [json.loads(line) for line in result.stdout.splitlines()]
