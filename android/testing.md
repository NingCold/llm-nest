# Android 客户端验收

日常修改先从 [Android 构建命令](README.md#安装与构建)运行单元测试、Lint 和 APK 构建。改动加密适配层时，从客户端仓库根目录运行：

```text
node android/crypto/test.cjs
corepack pnpm@11.22.0 --dir web exec tsc -p ../android/crypto/tsconfig.json
```

## 在独立模拟器上复验

这套用例使用三个临时真人账号（两人互发消息，另一个专测恢复文件）、真实服务端和 PostgreSQL，不登录日常账号，不连接模型。它先走原生界面收发文字，再在同一个私聊上建立加密。只对明确选定的新模拟器执行，不指向正在使用的手机。

1. 准备可执行的 `cloud-server`、本机 PostgreSQL 的 `initdb` 和 `pg_ctl`、OpenSSL、Python 3.11+，以及运行中的专用 Android 模拟器。测试服务、数据库和 TLS 代理只监听本机回环地址。
2. 在客户端仓库的 `android` 目录启动 fixture。输出路径必须是**尚不存在的私有目录**。脚本新建数据库、测试证书和收件箱，不使用已部署站点。保持此终端运行，等待 `Fixture ready`。
3. 在另一终端生成仅供本次测试的 APK 并运行。以下为 PowerShell 示例；把路径和模拟器序号换成实际值：

```powershell
# 终端一
$qa = Join-Path $env:TEMP ("llmn-qa-" + [guid]::NewGuid().ToString("N"))
$server = (Resolve-Path "..\..\llm-nest-server\target\debug\cloud-server.exe").Path
python tools/interop_fixture.py --output $qa --cloud-server $server

# 终端二：$qa 使用上面同一个目录
.\gradlew.bat "-PllmnTestResDir=$qa/res" :app:assembleDebug :app:assembleDebugAndroidTest
python tools/run_interop.py --fixture $qa --serial emulator-5554
```

QA APK 使用单独的应用 ID 和临时测试 CA；**不要把它交给使用者**。不要把 fixture 目录加入版本控制或发送给别人。检查结束后在其中创建 `STOP` 空文件，等终端确认数据库停止。此目录内的口令、恢复测试资料、证书私钥和数据库只属于这次测试；按项目约定列清单核对后清理。要重新验收首次身份建立，请使用新的 fixture。

`AndroidChatUiTest` 通过可见控件登录、从通讯录打开既有私聊，并让对端核对收到的正文和条数；对端发送后，不点击刷新即可在手机看到回复。它还会核对返回原会话后历史和草稿是否还在。`CryptoInteropTest` 在 Android WebView 中实际运行官方 Matrix SDK，检验双方信任、密文收发、恢复原文和拒绝明文降级。截图、编译通过或单方“已发送”都不能代替另一端读回同一条原文。

## 2026-11-06 · Android 0.1.0-dev.2

在 API 36 的独立模拟器上，用本机 HTTPS 测试服务、独立 PostgreSQL 实例和临时账号完成了三项检查：

- **原生聊天：** A 从通讯录进入原私聊并发送中文，B 读到同一条消息且只收到一次。B 回复后，A 不点刷新即可看见；返回再打开仍有原历史和未发送草稿。
- **加密与恢复：** 两个独立设备身份互核指纹。原生输入框发送后，服务端收到的正文字段为空、内容为 Matrix 密文，对端能读出原文。篡改密文和替换成明文均被拒绝。为 B 建立另一设备身份后，使用恢复文件读回旧消息；错误口令不能解锁。B 随后的加密回复经 SSE 出现在 A 的原生聊天页。
- **恢复文件保存：** `DeviceRecoveryUiTest` 拦截系统选择器 Intent，分别返回取消和选定测试文件。取消不确认；成功写入后核对文件格式、服务器和账号，仍须在原生对话框中明确确认才生效。本项验证了应用对选择结果的处理，未手动操作系统文件选择器。

普通调试 APK 已在同一模拟器从预览版（versionCode 1）覆盖安装到开发版（versionCode 2），应用 ID 不变，冷启动成功。13 项 JVM 单元测试通过，Lint 无错误。上述检查未使用真实模型。日常手机的系统证书安装、中文输入法、系统文件选择器，以及不同厂商的后台行为，仍需真机验证。
