plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.stellarmls.mls"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        jniLibs {
            // Rust .so files are already stripped; skip AGP stripping for reproducible builds
            keepDebugSymbols += "**/libsep_xxxx_circuits.so"
        }
    }
}

// Pick up freshly built Rust .so files from `build/android/jniLibs/` (the default
// output of `scripts/build-android.sh`) so a rebuild is consumed without a manual
// copy. No-op on a fresh clone where that dir is absent — the committed .so under
// src/main/jniLibs/ is the fallback. Hooked into preBuild so it runs before AGP
// merges JNI sources.
val syncRustNativeLibs by tasks.registering(Copy::class) {
    val freshlyBuilt = layout.projectDirectory.dir("../build/android/jniLibs")
    onlyIf { freshlyBuilt.asFile.exists() }
    from(freshlyBuilt)
    into(layout.projectDirectory.dir("src/main/jniLibs"))
}

tasks.named("preBuild") {
    dependsOn(syncRustNativeLibs)
}
