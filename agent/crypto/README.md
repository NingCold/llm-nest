# 设备加密

这部分让 Agent 保存自己的加密设备身份、加密和读取聊天，并导出或恢复历史解密资料。它使用 Matrix 官方 Rust SDK；浏览器使用同一项目提供的 WebAssembly SDK。

每台设备拥有自己的身份。更换设备时，恢复历史资料并建立新身份，不复制正在使用的设备数据库。模型 Key、节点连接凭据和未完成的模型调用都不随恢复资料转移。

使用范围和恢复规则见[加密设计](../../docs/design/encryption.md)，接入方式见[协议约定](../../docs/design/crypto-contract.md)。
