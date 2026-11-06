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

`DeviceRecoveryUiTest` 单独运行时，只传 `origin`、`emailA`、`passwordA` 即可验证恢复文件保存。模拟器脚本还会传入对端账号和 `backgroundDraftCheck=true`，在同一个已建立密钥的账号上验证草稿慢写、写失败时的后台锁定。

## 在授权真机上验证输入法、文件保存和后台锁定

沿用上面的**新 fixture 和对应的两个 QA APK**。测试使用独立包 `com.llmnest.android.debug`，不需要动日常账号或系统证书。用 `adb devices -l` 核对设备，将完整序号赋给 `$serial`。先检查当前用户是否已有 QA 包：如果已有，保留其数据，改用另一台测试设备或与机主确认用途。普通包 `com.llmnest.android` 不参与这项测试。

```powershell
$profileId = (adb -s $serial shell am get-current-user).Trim()
adb -s $serial shell pm list packages --user $profileId com.llmnest.android.debug
# 当前用户没有上述 QA 包时，逐个安装并确认系统安装结果：
adb -s $serial install --user $profileId app/build/outputs/apk/debug/app-debug.apk
adb -s $serial install --user $profileId app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
# 一个终端只看本次测试的交互提示；另一个终端启动测试：
adb -s $serial logcat -T 1 -s LLMNHandset:I
python tools/run_interop.py --assisted-handset --fixture $qa --serial $serial
```

出现 `WAIT_LAUNCH_QA` 后，在手机上打开 QA 客户端（`com.llmnest.android.debug`）。测试会在这个前台窗口登录临时账号；随后跟随提示完成三步：

1. **输入：** 在聊天输入框用手机键盘打出「你好」，点发送。对端必须恰好读到一次。若仅验证了输入连接的组合文本，记录为输入连接通过，不能算作中文输入法通过。
2. **保存：** 第一次在系统文件选择器点返回；第二次选本机「下载」，用提示中的唯一文件名保存。确认写入成功后才点「我已安全保存」。
3. **锁定：** 按 Home，看到 `BACKGROUND_LOCK_VERIFIED` 后从最近任务回到 QA 客户端，点「解锁加密设备」。测试会用临时口令解锁，核对回到原会话、草稿恢复且没有误发。

要复查**草稿写完前就返回，或写入失败后再返回**，在同一套 QA 包上运行下面的命令。它使用 fixture 中独立的 Recovery 账号，请在该账号首次建立加密身份时运行。

```powershell
# 一个终端看提示，另一个终端运行测试：
adb -s $serial logcat -T 1 -s LLMNRecoveryUi:I
python tools/run_interop.py --handset-drafts --user $profileId --fixture $qa --serial $serial
```

看到 `WAIT_LAUNCH_QA` 时打开 QA 客户端；之后每次看到 `WAIT_RETURN_QA` 就重新打开它。用例会分别检查慢写、写失败、失败后再次退到后台、重试落盘后的恢复。最终应显示 `DeviceRecoveryUiTest: passed`。

要单独复查「历史刷新时打字不被打断」，保留已安装的两个 QA 包，执行以下命令后在手机上打开 QA 客户端。此项不需要服务器或账号：

```powershell
adb -s $serial shell am instrument --user $profileId -w -e class com.llmnest.android.ChatComposerRefreshTest com.llmnest.android.debug.test/androidx.test.runner.AndroidJUnitRunner
```

结束后只清理本次新装的两个 QA 包和本次保存的唯一文件；按上节步骤停止并清理 fixture。执行普通 `:app:assembleDebug` 后，APK 输出才恢复为供用户安装的普通包。

## 2026-11-07 · Android 0.1.0-dev.5：连续发言与输入

在 API 36 的独立模拟器上，使用合成会话检查了 411dp 宽的亮色聊天页和 360dp 宽的暗色聊天页。同一人连续发言只显示一次署名和时间，换人、超过五分钟或跨日都会重新分组。通过「更多操作 → 刷新」仍能刷新，输入框和选区保留；未知发送和保存失败时，各自的重试入口仍能触发原动作。正在输入时连续刷新历史，组合文本和焦点也保持不变。

要复查排版，可在专用模拟器安装普通 debug 包及对应的 instrumentation 包，运行 `com.llmnest.android.ChatVisualQaTest`。它将合成内容传给实际的聊天视图，截取该视图的画面，不使用日常账号，也不关闭安全窗口标志。

## 2026-11-06 · Android 0.1.0-dev.4：草稿慢写或失败时，返回仍须解锁

在 API 36 独立模拟器上，把有未发送文字的聊天切到后台，在草稿写入尚未完成时立即返回，聊天没有重新出现。分别让写入成功和失败，两次都必须用本机口令解锁。失败后再进一次后台，回到原会话仍可看到未保存的文字；点重试后，从草稿存储读回了同一段文字。慢写和失败由测试在存储边界注入，切换前后台及 Matrix 解锁走实际客户端。

同轮通过了原生聊天收发、Matrix 加密互通及恢复文件保存用例，普通 APK 也在该模拟器上从 dev.3 覆盖安装到 dev.4。19 项 JVM 单元测试通过，Lint 无错误。其中 HTTPS 测试核对服务端实际收到的请求：客户端不跟随 302；收到 503 或提交后断线时，不自动重发请求。

**真机复验：** vivo V2359A（Android 16 / API 36）的普通包从 dev.3 保留数据升级到 dev.4，正常打开到登录页。独立 QA 包上，草稿写入未完成时返回没有显示输入框；写入失败后再次切到后台，解锁仍能找回原文，重试后从存储读回一致。正常草稿在按 Home、手动解锁后也回到原会话，没有误发。

同机的临时对端恰好收到一次「你好」；系统文件选择器取消不确认，保存到「下载」并在应用内确认后，服务端才登记恢复材料。历史刷新时，正在输入的拼音组合、选区和焦点均保留。中文发送验证使用实际输入框的 Android 输入连接，未验证 Gboard 手工选词。

## 2026-11-06 · Android 0.1.0-dev.3：刷新不断开正在输入的文字

在同一台 vivo V2359A 上，用同一份 `ChatComposerRefreshTest` 对比两个版本。dev.2 第一次刷新会话历史就断开了正在编辑的输入框；dev.3 连续三次刷新，输入框始终挂在原窗口，未丢焦。测试分别保留了尚未提交的拼音组合、正向选区和反向选区，也验证了发送待确认时禁止编辑、发送确认后清空、锁定后移除输入框。

使用新的临时账号，在 dev.3 重跑了下节的真机收发、系统文件选择器和后台解锁流程，全部通过。普通 APK（versionCode 3）在该机覆盖安装 dev.2 后，正常打开到登录页。

## 2026-11-06 · Android 0.1.0-dev.2

在 API 36 的独立模拟器上，用本机 HTTPS 测试服务、独立 PostgreSQL 实例和临时账号完成了三项检查：

- **原生聊天：** A 从通讯录进入原私聊并发送中文，B 读到同一条消息且只收到一次。B 回复后，A 不点刷新即可看见；返回再打开仍有原历史和未发送草稿。
- **加密与恢复：** 两个独立设备身份互核指纹。原生输入框发送后，服务端收到的正文字段为空、内容为 Matrix 密文，对端能读出原文。篡改密文和替换成明文均被拒绝。为 B 建立另一设备身份后，使用恢复文件读回旧消息；错误口令不能解锁。B 随后的加密回复经 SSE 出现在 A 的原生聊天页。
- **恢复文件保存：** `DeviceRecoveryUiTest` 拦截系统选择器 Intent，分别返回取消和选定测试文件。取消不确认；成功写入后核对文件格式、服务器和账号，仍须在原生对话框中明确确认才生效。本项验证了应用对选择结果的处理，未手动操作系统文件选择器。

普通调试 APK 已在同一模拟器从预览版（versionCode 1）覆盖安装到开发版（versionCode 2），应用 ID 不变，冷启动成功。13 项 JVM 单元测试通过，Lint 无错误。上述检查未使用真实模型。

同日，在 vivo V2359A（Android 16 / API 36、WebView 156.0.8078.47）安装普通版并打开到登录页。独立 QA 包用临时账号完成了以下真机检查：

- 系统文件选择器取消后，恢复状态仍未确认；第二次将 JSON 写到本机「下载」，点「我已安全保存」后，服务端才登记加密恢复材料。
- 在带有未发送草稿的私聊按 Home，聊天与订阅卸载，密钥库锁定。回到应用必须用本机口令解锁，之后回到同一私聊，草稿未丢失、未误发。
- Gboard 中文九键在输入框弹出；通过 Android 输入连接组合、提交「你好」，对端恰好收到一次。未用真人选词验证，也没有安装系统证书。
