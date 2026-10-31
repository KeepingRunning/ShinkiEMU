plugins {
    id("com.android.application")
}

// Release signing material comes only from the environment. Blank values count as
// missing because GitHub Actions expands absent secrets to empty strings.
val releaseSigningVariables = listOf(
    "SHINKI_EMU_ANDROID_KEYSTORE",
    "SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD",
    "SHINKI_EMU_ANDROID_KEY_ALIAS",
    "SHINKI_EMU_ANDROID_KEY_PASSWORD",
)
val releaseSigning: Map<String, String> = releaseSigningVariables
    .mapNotNull { name ->
        providers.environmentVariable(name).orNull?.takeIf { it.isNotBlank() }?.let { name to it }
    }
    .toMap()
val missingReleaseSigning = releaseSigningVariables.filterNot(releaseSigning::containsKey)

val generatedRoot = layout.projectDirectory.dir(
    providers.environmentVariable("SHINKI_EMU_ANDROID_GENERATED_ROOT")
        .orElse("../generated").get()
)

android {
    namespace = "org.shinkirou.shinkiemu"
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    buildToolsVersion = "37.0.0"
    ndkVersion = "30.0.16248370"

    buildFeatures { aidl = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "org.shinkirou.shinkiemu"
        minSdk = 26
        targetSdk = 37
        versionCode = providers.environmentVariable("SHINKI_EMU_ANDROID_VERSION_CODE")
            .orElse("1")
            .get()
            .toInt()
        versionName = providers.environmentVariable("SHINKI_EMU_ANDROID_VERSION_NAME")
            .orElse("0.1.0")
            .get()
        ndk {
            abiFilters += providers.environmentVariable("SHINKI_EMU_ANDROID_ABI")
                .orElse("arm64-v8a,x86_64").get().split(",")
        }
    }

    signingConfigs.getByName("debug") {
        // Keep the generated test key beside the Gradle cache, not in ~/.android.
        storeFile = gradle.gradleUserHomeDir.resolve("shinkiemu-debug.keystore")
    }

    if (missingReleaseSigning.isEmpty()) {
        signingConfigs {
            create("release") {
                storeFile = file(releaseSigning.getValue("SHINKI_EMU_ANDROID_KEYSTORE"))
                storePassword = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEY_ALIAS")
                keyPassword = releaseSigning.getValue("SHINKI_EMU_ANDROID_KEY_PASSWORD")
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
                enableV4Signing = true
            }
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
        }
        release {
            isMinifyEnabled = false
            isDebuggable = false
            if (missingReleaseSigning.isEmpty()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    sourceSets.getByName("main") {
        jniLibs.directories.add(generatedRoot.dir("jniLibs").asFile.absolutePath)
        assets.directories.add(generatedRoot.dir("assets").asFile.absolutePath)
        res.directories.add(generatedRoot.dir("res").asFile.absolutePath)
        java.srcDir(generatedRoot.dir("assets/cores/kirikiroid2-web/Resources/android-host/java"))
        aidl.srcDir(generatedRoot.dir("assets/cores/kirikiroid2-web/Resources/android-host/java"))
    }

    androidResources {
        // The complete inventoried runtime includes underscore-prefixed modules.
        ignoreAssetsPattern = ""
    }

    packaging {
        jniLibs {
            // NativeActivity and dladdr need ordinary extracted filesystem paths.
            // RFVP is statically linked into the Manager library.
            useLegacyPackaging = true
        }
    }
}

// APKs are named ShinkiEMU-<versionName>-<buildType>.apk.
base {
    archivesName = "ShinkiEMU-${android.defaultConfig.versionName}"
}

// Without the release key the release variant would package an unsigned APK. Refuse
// to schedule any release APK/AAB packaging task instead, so no such file is ever produced.
val releasePackagingTasks = setOf("packageRelease", "packageReleaseBundle", "signReleaseBundle")
gradle.taskGraph.whenReady {
    if (missingReleaseSigning.isEmpty()) return@whenReady
    val releasePackaging = allTasks.filter { task ->
        task.project == project && task.name in releasePackagingTasks
    }
    if (releasePackaging.isNotEmpty()) {
        throw GradleException(
            "ShinkiEMU release packaging (${releasePackaging.joinToString { it.path }}) needs the " +
                "release signing key; missing or empty: ${missingReleaseSigning.joinToString()}. " +
                "Use :app:assembleDebug for a debug-signed APK."
        )
    }
}

kotlin {
    jvmToolchain(27)
    compilerOptions.jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
}

abstract class RustlsVersion : ValueSource<String, RustlsVersion.Params> {
    interface Params : ValueSourceParameters {
        val lockFile: RegularFileProperty
    }

    override fun obtain(): String {
        val lines = parameters.lockFile.get().asFile.readLines()
        val nameIndex = lines.indexOfFirst {
            it.trim() == "name = \"rustls-platform-verifier-android\""
        }
        val version = if (nameIndex < 0) null else lines.drop(nameIndex + 1)
            .firstOrNull { it.trimStart().startsWith("version = ") }
            ?.substringAfter('"', "")
            ?.substringBefore('"', "")
            ?.takeIf { it.isNotEmpty() }
        return version ?: error("rustls-platform-verifier-android not found in Cargo.lock")
    }
}

val rustlsPlatformVerifierVersion = providers.of(RustlsVersion::class.java) {
    parameters.lockFile.set(rootProject.layout.projectDirectory.file("../../Cargo.lock"))
}

dependencies {
    implementation(fileTree(generatedRoot.dir("assets/cores/kirikiroid2-web/Resources/android-host/libs")) {
        include("*.jar")
    })
    // The Kotlin verifier component must match Cargo's resolved Android shim.
    // Minify stays off; enabling it also requires keeping its JNI-referenced classes.
    implementation(rustlsPlatformVerifierVersion.map {
        "org.rustls:rustls-platform-verifier:$it"
    })
}
