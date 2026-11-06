# LLM Nest · Android

用 Android Studio 打开本目录，选择模拟器或测试设备，点击 **Run**。应用有“消息”和“通讯录”两个入口，跟随系统切换浅色、深色。当前是离线界面预览，不需要账号，不会请求网络或读取手机通讯录。

## 从命令行构建

安装 JDK 17 或 21、Android SDK Platform 36 和 Build Tools 36.0.0。让 Android Studio 设置 SDK 路径，或将 `ANDROID_HOME` 指向本机 SDK。设备至少需要 Android 8.0（API 26）。

在本目录运行：

```powershell
.\gradlew.bat :app:assembleDebug :app:lintDebug
```

macOS 或 Linux 使用 `./gradlew`。调试包位于 `app/build/outputs/apk/debug/app-debug.apk`。无需另装 Gradle、Node.js、Rust 或 Agent。

## 从哪里修改

- [MainActivity.kt](app/src/main/java/com/llmnest/android/MainActivity.kt)：应用入口、标签切换和页面状态。
- [布局](app/src/main/res/layout/activity_main.xml)、[文案与浅色样式](app/src/main/res/values)、[深色样式](app/src/main/res/values-night)：两个入口共用的原生界面。
- [应用构建配置](app/build.gradle.kts)：最低系统版本、应用 ID 和构建版本。启动图标沿用[客户端品牌源文件](../assets/branding/README.md)。
