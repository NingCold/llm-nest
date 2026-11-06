plugins {
    id("com.android.application")
}

android {
    namespace = "com.llmnest.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.llmnest.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-dev"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
