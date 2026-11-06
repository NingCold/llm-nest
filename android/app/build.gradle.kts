import com.github.gradle.node.task.NodeTask
import org.gradle.api.artifacts.component.ModuleComponentIdentifier
import java.util.zip.ZipFile

plugins {
    id("com.android.application")
    id("com.github.node-gradle.node")
}

android {
    namespace = "com.llmnest.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.llmnest.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 4
        versionName = "0.1.0-dev.4"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    providers.gradleProperty("llmnTestResDir").orNull?.let {
        sourceSets.getByName("debug").res.directories.add(it)
        buildTypes.getByName("debug").applicationIdSuffix = ".debug"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.6.4")
    testImplementation("junit:junit:4.13.2")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation("com.squareup.okhttp3:okhttp-tls:4.12.0")
    testImplementation("org.json:json:20240303")
    androidTestImplementation("androidx.test:runner:1.5.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
}

// The Android runtime consumes the same pinned Matrix SDK and engine as Web.
val webRoot = rootProject.projectDir.resolve("../web")
val nodeVersion = "24.18.0" // Same baseline as the Web CI; pnpm 11 and the frozen lockfile require modern Node.
val windows = System.getProperty("os.name").startsWith("Windows")
val pathKey = if (windows && System.getenv().containsKey("Path")) "Path" else "PATH"
val systemNode = try {
    providers.exec {
        commandLine("node", "--print", "process.versions.node+'\\n'+process.execPath")
        isIgnoreExitValue = true
    }.standardOutput.asText.get().trim().lines().let { lines ->
        if (lines.size != 2 || lines[0] != nodeVersion) null else file(lines[1]).parentFile
    }
} catch (_: Exception) { null }
fun corepackIn(bin: File) = if (windows) bin.resolve("node_modules/corepack/dist/corepack.js")
    else bin.resolve("../lib/node_modules/corepack/dist/corepack.js")
val reuseNode = systemNode?.takeIf { corepackIn(it).isFile }
node {
    version.set(nodeVersion)
    download.set(reuseNode == null)
    workDir.set(rootProject.layout.projectDirectory.dir(".gradle/nodejs"))
    nodeProjectDir.set(webRoot)
    distBaseUrl.set(null as String?) // Distribution repository is centrally declared in settings.
    enableTaskRules.set(false)
}
val nodeBin = node.resolvedNodeDir.map { if (windows) it.asFile else it.dir("bin").asFile }
val selectedBin = providers.provider { reuseNode ?: nodeBin.get() }
val corepack = selectedBin.map { corepackIn(it) }
fun NodeTask.pnpm(vararg command: String) {
    workingDir.set(webRoot)
    script.fileProvider(corepack)
    args.set(listOf("pnpm@11.22.0") + command)
    environment.put(pathKey, selectedBin.map { it.absolutePath + File.pathSeparator + System.getenv(pathKey).orEmpty() })
    environment.put("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
}
val installCryptoDependencies = tasks.register<NodeTask>("installCryptoDependencies") {
    inputs.files(webRoot.resolve("package.json"), webRoot.resolve("pnpm-lock.yaml"))
    outputs.file(webRoot.resolve("node_modules/.modules.yaml"))
    pnpm("install", "--frozen-lockfile")
}
val bundleCrypto = tasks.register<NodeTask>("bundleCrypto") {
    dependsOn(installCryptoDependencies)
    inputs.dir(rootProject.projectDir.resolve("crypto"))
    inputs.dir(webRoot.resolve("src/cloud/crypto"))
    inputs.files(webRoot.resolve("vite.android-crypto.config.ts"), webRoot.resolve("pnpm-lock.yaml"))
    outputs.dir(layout.buildDirectory.dir("generated/cryptoAssets/crypto"))
    pnpm("exec", "vite", "build", "--config", "vite.android-crypto.config.ts")
}
val verifyCryptoBundle = tasks.register<NodeTask>("verifyCryptoBundle") {
    dependsOn(bundleCrypto)
    script.set(rootProject.projectDir.resolve("crypto/check-bundle.cjs"))
    inputs.dir(layout.buildDirectory.dir("generated/cryptoAssets/crypto"))
}
abstract class CryptoAssetNotices : NodeTask() {
    @get:OutputDirectory abstract val outputDirectory: DirectoryProperty
}
val cryptoNotices = tasks.register<CryptoAssetNotices>("cryptoNotices") {
    dependsOn(verifyCryptoBundle)
    inputs.files(webRoot.resolve("pnpm-lock.yaml"), rootProject.projectDir.resolve("../scripts/web-notices.cjs"))
    inputs.dir(rootProject.projectDir.resolve("../packaging/licenses"))
    outputDirectory.set(layout.buildDirectory.dir("generated/cryptoAssets"))
    workingDir.set(rootProject.projectDir.resolve(".."))
    script.set(rootProject.projectDir.resolve("../scripts/web-notices.cjs"))
    args.set(listOf(layout.buildDirectory.file("generated/cryptoAssets/crypto/THIRD_PARTY_NOTICES.txt").get().asFile.absolutePath))
    environment.put(pathKey, selectedBin.map { it.absolutePath + File.pathSeparator + System.getenv(pathKey).orEmpty() })
    environment.put("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
}
tasks.named("preBuild") { dependsOn(cryptoNotices) }
androidComponents.onVariants { variant ->
    variant.sources.assets?.addGeneratedSourceDirectory(cryptoNotices) { it.outputDirectory }
    val runtime = configurations.named("${variant.name}RuntimeClasspath")
    val notices = tasks.register<NativeAssetNotices>("${variant.name}NativeNotices") {
        artifacts.from(runtime)
        coordinates.set(runtime.flatMap { it.incoming.artifacts.resolvedArtifacts }.map { artifacts ->
            artifacts.map { artifact ->
                val id = artifact.id.componentIdentifier as ModuleComponentIdentifier
                "${id.group}:${id.module}:${id.version}|${artifact.file.absolutePath}"
            }.sorted()
        })
        sourceLicense.set(rootProject.layout.projectDirectory.file("../LICENSE"))
        sourceNotice.set(rootProject.layout.projectDirectory.file("../NOTICE"))
        mplLicense.set(rootProject.layout.projectDirectory.file("gradle/licenses/MPL-2.0.txt"))
        outputDirectory.set(layout.buildDirectory.dir("generated/nativeNotices/${variant.name}"))
    }
    variant.sources.assets?.addGeneratedSourceDirectory(notices) { it.outputDirectory }
}

/** Runtime artifacts, not test/Gradle dependencies, determine the packaged native inventory. */
abstract class NativeAssetNotices : DefaultTask() {
    @get:InputFiles @get:PathSensitive(PathSensitivity.NONE) abstract val artifacts: ConfigurableFileCollection
    @get:Input abstract val coordinates: ListProperty<String>
    @get:InputFile abstract val sourceLicense: RegularFileProperty
    @get:InputFile abstract val sourceNotice: RegularFileProperty
    @get:InputFile abstract val mplLicense: RegularFileProperty
    @get:OutputDirectory abstract val outputDirectory: DirectoryProperty

    @TaskAction fun generate() {
        val sources = mapOf(
            "com.squareup.okhttp3:okhttp" to "https://github.com/square/okhttp",
            "com.squareup.okio:okio" to "https://github.com/square/okio",
            "com.squareup.okio:okio-jvm" to "https://github.com/square/okio",
            "org.jetbrains.kotlin:kotlin-stdlib" to "https://github.com/JetBrains/kotlin",
            "org.jetbrains.kotlin:kotlin-stdlib-jdk7" to "https://github.com/JetBrains/kotlin",
            "org.jetbrains.kotlin:kotlin-stdlib-jdk8" to "https://github.com/JetBrains/kotlin",
            "org.jetbrains.kotlinx:kotlinx-coroutines-core-jvm" to "https://github.com/Kotlin/kotlinx.coroutines",
            "org.jetbrains.kotlinx:kotlinx-coroutines-android" to "https://github.com/Kotlin/kotlinx.coroutines",
            "org.jetbrains:annotations" to "https://github.com/JetBrains/java-annotations",
            "androidx.annotation:annotation" to "https://android.googlesource.com/platform/frameworks/support/+/androidx-main/annotation/",
        )
        val text = StringBuilder("LLM Nest Android native runtime third-party notices\n\n")
        var hasOkHttp = false
        for (entry in coordinates.get()) {
            val (coordinate, path) = entry.split('|', limit = 2)
            val module = coordinate.substringBeforeLast(':')
            val source = sources[module] ?: error("Missing native license mapping for $coordinate")
            text.append("$coordinate\nLicense: Apache-2.0\nSource: $source\n\n")
            if (module == "com.squareup.okhttp3:okhttp") hasOkHttp = true
            ZipFile(path).use { archive ->
                archive.entries().asSequence().filter { file ->
                    !file.isDirectory && file.name.substringAfterLast('/').matches(Regex("(?i)(LICENSE|NOTICE|COPYING)([._-].*)?"))
                }.sortedBy { it.name }.forEach { file ->
                    text.append("${file.name}\n").append(archive.getInputStream(file).bufferedReader(Charsets.UTF_8).use { it.readText() }).append("\n\n")
                }
            }
        }
        text.append("Apache License 2.0 — applies to the native dependencies listed above\n\n")
            .append(sourceLicense.get().asFile.readText(Charsets.UTF_8))
        if (hasOkHttp) text.append("\n\nOkHttp Public Suffix List resource — Mozilla Public License 2.0\n")
            .append("Source: https://publicsuffix.org/list/public_suffix_list.dat\nLicense source: https://www.mozilla.org/media/MPL/2.0/index.txt\n\n")
            .append(mplLicense.get().asFile.readText(Charsets.UTF_8))
        val output = outputDirectory.get().asFile.apply { mkdirs() }
        output.resolve("THIRD_PARTY_NOTICES.txt").writeText(text.toString(), Charsets.UTF_8)
        output.resolve("LICENSE.txt").writeText(sourceLicense.get().asFile.readText(Charsets.UTF_8), Charsets.UTF_8)
        output.resolve("NOTICE.txt").writeText(sourceNotice.get().asFile.readText(Charsets.UTF_8), Charsets.UTF_8)
    }
}
