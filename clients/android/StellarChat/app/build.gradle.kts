plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("com.google.devtools.ksp")
}

val isFdroidReleaseBuild = gradle.startParameter.taskNames.any { task ->
    task.contains("fdroid", ignoreCase = true) && task.contains("release", ignoreCase = true)
}

android {
    namespace = "chat.onym.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "chat.onym.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 10
        versionName = "1.0.9"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables.useSupportLibrary = true

        // Read relayer/.env for build-time defaults
        val envFile = rootProject.file("../../../relayer/.env")
        val envMap = mutableMapOf<String, String>()
        if (envFile.exists()) {
            envFile.readLines().forEach { line ->
                val trimmed = line.trim()
                if (trimmed.isEmpty() || trimmed.startsWith("#")) return@forEach
                val eqIdx = trimmed.indexOf('=')
                if (eqIdx > 0) {
                    val key = trimmed.substring(0, eqIdx)
                    var value = trimmed.substring(eqIdx + 1).trim()
                    if (value.startsWith("\"") && value.endsWith("\"")) {
                        value = value.substring(1, value.length - 1)
                    }
                    envMap[key] = value
                }
            }
        }

        // Read root .env for DOMAIN only (do NOT ship secrets like API keys)
        val rootEnvFile = rootProject.file("../../../.env")
        var domain = ""
        if (rootEnvFile.exists()) {
            rootEnvFile.readLines().forEach { line ->
                val trimmed = line.trim()
                if (trimmed.isEmpty() || trimmed.startsWith("#")) return@forEach
                val eqIdx = trimmed.indexOf('=')
                if (eqIdx > 0) {
                    val key = trimmed.substring(0, eqIdx)
                    val value = trimmed.substring(eqIdx + 1).trim().removeSurrounding("\"")
                    if (key == "DOMAIN") domain = value
                }
            }
        }

        val relayerBind = envMap["RELAYER_BIND"] ?: ""
        val defaultRelayerURL = if (domain.isNotEmpty()) "https://relay.$domain"
            else if (relayerBind.isNotEmpty()) "http://$relayerBind"
            else ""
        val authTokens = envMap["RELAYER_AUTH_TOKENS"] ?: ""
        val firstToken = authTokens.split(",").firstOrNull()?.trim() ?: ""
        val defaultNostrRelay = if (domain.isNotEmpty()) "wss://nostr.$domain" else ""
        val defaultBlossomServer = if (domain.isNotEmpty()) "https://blossom.$domain" else ""

        buildConfigField("String", "DEFAULT_CONTRACT_ENDPOINT", "\"${envMap["RELAYER_RPC_URL"] ?: ""}\"")
        buildConfigField("String", "DEFAULT_CONTRACT_ID", "\"${envMap["RELAYER_CONTRACT_ID"] ?: ""}\"")
        buildConfigField("String", "DEFAULT_RELAYER_URL", "\"$defaultRelayerURL\"")
        buildConfigField("String", "DEFAULT_RELAYER_AUTH_TOKEN", "\"$firstToken\"")
        buildConfigField("String", "DEFAULT_NOSTR_RELAY", "\"$defaultNostrRelay\"")
        buildConfigField("String", "DEFAULT_BLOSSOM_SERVER", "\"$defaultBlossomServer\"")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            vcsInfo.include = false
        }
    }

    flavorDimensions += "distribution"
    productFlavors {
        create("play") { dimension = "distribution" }
        create("fdroid") { dimension = "distribution" }
    }

    if (isFdroidReleaseBuild) {
        splits {
            abi {
                isEnable = true
                reset()
                include("arm64-v8a")
                isUniversalApk = false
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    packaging {
        resources.excludes += "META-INF/*.version"
        // Exclude baseline profile for reproducible builds
        resources.excludes += "assets/dexopt/baseline.prof"
        resources.excludes += "assets/dexopt/baseline.profm"
    }
}

tasks.whenTaskAdded {
    if (name.contains("ArtProfile")) {
        enabled = false
    }
}

dependencies {
    // Compose
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")

    // Networking
    implementation("com.squareup.okhttp3:okhttp:4.12.0")

    // kotlin-mls: ZK proofs, Schnorr signing, commitment verification
    implementation(project(":kotlin-mls"))

    // Security
    implementation("androidx.security:security-crypto:1.1.0-alpha06")

    // Bouncy Castle (Ed25519 + X25519 for API 26+)
    implementation("org.bouncycastle:bcprov-jdk18on:1.78.1")

    // Room persistence
    implementation("androidx.room:room-runtime:2.6.1")
    implementation("androidx.room:room-ktx:2.6.1")
    ksp("androidx.room:room-compiler:2.6.1")

    // WebRTC (voice/video calls)
    implementation("io.github.webrtc-sdk:android:144.7559.01")

    // QR code generation
    implementation("com.google.zxing:core:3.5.3")

    // ML Kit barcode scanning (play flavor only — proprietary)
    "playImplementation"("com.google.mlkit:barcode-scanning:17.3.0")

    // CameraX for QR scanner
    implementation("androidx.camera:camera-core:1.4.1")
    implementation("androidx.camera:camera-camera2:1.4.1")
    implementation("androidx.camera:camera-lifecycle:1.4.1")
    implementation("androidx.camera:camera-view:1.4.1")

    // Push notifications (play flavor — proprietary FCM)
    "playImplementation"(platform("com.google.firebase:firebase-bom:33.7.0"))
    "playImplementation"("com.google.firebase:firebase-messaging")
    "playImplementation"("org.jetbrains.kotlinx:kotlinx-coroutines-play-services:1.7.3")

    // Push notifications (fdroid flavor — FOSS UnifiedPush)
    "fdroidImplementation"("org.unifiedpush.android:connector:2.5.0")
    // Core
    implementation("androidx.core:core-ktx:1.15.0")

    // Android instrumented tests
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.ext:junit-ktx:1.2.1")
}

if (file("google-services.json").exists()) {
    apply(plugin = "com.google.gms.google-services")
}
