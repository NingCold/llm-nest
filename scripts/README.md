# 客户端脚本

[开发指南](../docs/development.md) · [发布指南](../docs/releases.md)

- `release.py` 核对客户端版本、收集安装包与 SHA-256；`test_release.py` 为离线验证。
- `package-web.py` 打包已构建的 Web 静态文件和版本摘要，服务端接收该产物；`test_build_artifacts.py` 验证打包内容。
- `generate-icons.mjs` 从唯一 SVG 编辑源生成 Web/桌面图标。
- `verify-windows-icons.ps1`、`verify-windows-branding.ps1` 和 `windows_package_acceptance.py` 检查 Windows 包；`linux/` 包含 Linux 构建和启动检查。
- `clean-build-artifacts.py` 先生成完整清单再逐文件清理，不递归删除；使用前读帮助和 AGENTS。
- `package-crypto-fixture.cjs OUTPUT` 从现有客户端模块编译测试专用加密夹具。需要先安装 web 依赖。输出 CJS 和含客户端版本/文件 SHA-256 的 manifest.json，API 替身由隔离集成测试注入，不进入产品运行。

服务端、迁移和跨仓加密验收由[服务端仓](https://github.com/NingCold/llm-nest-server)维护；模型及 Agent 验收由[Agent 仓](https://github.com/NingCold/llm-nest-agent)维护。新远端本轮尚未发布。
