import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

// Ключ релизов: из переменных окружения (CI: ZHOR_KEYSTORE, ZHOR_KEYSTORE_PASSWORD, ZHOR_KEY_ALIAS,
// ZHOR_KEY_PASSWORD) или из ~/.android/zhor-release.properties (storeFile, storePassword, keyAlias,
// keyPassword). Без ключа релиз подписывается отладочным ключом — для своих сборок этого достаточно.
val releaseKey: Properties? = System.getenv("ZHOR_KEYSTORE")?.let { store ->
    Properties().apply {
        setProperty("storeFile", store)
        setProperty("storePassword", System.getenv("ZHOR_KEYSTORE_PASSWORD") ?: "")
        setProperty("keyAlias", System.getenv("ZHOR_KEY_ALIAS") ?: "zhor")
        setProperty("keyPassword", System.getenv("ZHOR_KEY_PASSWORD") ?: System.getenv("ZHOR_KEYSTORE_PASSWORD") ?: "")
    }
} ?: File(System.getProperty("user.home"), ".android/zhor-release.properties").takeIf { it.isFile }?.let { f ->
    Properties().apply { f.inputStream().use { load(it) } }
}

android {
    namespace = "dev.niazlv.zhor"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.niazlv.zhor"
        minSdk = 30
        targetSdk = 36
        versionCode = 7
        versionName = "0.7.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    signingConfigs {
        if (releaseKey != null) create("release") {
            storeFile = file(releaseKey.getProperty("storeFile"))
            storePassword = releaseKey.getProperty("storePassword")
            keyAlias = releaseKey.getProperty("keyAlias")
            keyPassword = releaseKey.getProperty("keyPassword")
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            signingConfig = signingConfigs.getByName(if (releaseKey != null) "release" else "debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
    buildFeatures { compose = true }
    // zhord — исполняемый файл под видом libzhord.so: распаковывается в nativeLibraryDir
    packaging { jniLibs { useLegacyPackaging = true } }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.androidx.navigation.compose)
}
