# Android 随包许可补文

`NativeAssetNotices` 读取实际运行依赖及这些固定版本材料，将结果写入 APK 的 `assets/THIRD_PARTY_NOTICES.txt`。

- `MPL-2.0.txt`：OkHttp 内嵌 Public Suffix List 的许可，原署名来自 OkHttp JAR 的 `NOTICE`。
- `kotlinx-coroutines-1.6.4-NOTICE.txt`：上游 [1.6.4 的库 NOTICE](https://github.com/Kotlin/kotlinx.coroutines/blob/1.6.4/license/NOTICE.txt) 原文；发布 JAR 未携带该文件。
- `kotlin-stdlib-2.2.10.txt`：按上游 [2.2.10 许可清单](https://github.com/JetBrains/kotlin/blob/v2.2.10/license/README.md) 补齐 JVM 标准库涉及的 GWT、Guava、ThreeTen 与 Boost 条款和归属，各段保留原文来源。未加入仅适用于编译器的 NOTICE。

升级对应依赖时重新核对来源与适用范围。Matrix/WASM 的材料由共用 Web 许可脚本生成，位于 APK 的 `assets/crypto/`；项目 LICENSE、NOTICE 同时进入这两组资产。
