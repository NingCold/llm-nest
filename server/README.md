# 服务端

[项目首页](../README.md) · [开发指南](../docs/development.md)

这里负责账号、联系人、聊天记录和消息同步。[http](http/README.md) 提供浏览器接口和页面，[social](social/README.md) 处理具体聊天操作。

现在运行的仍是本地服务，在仓库根目录执行 `cargo run -p web-server --locked` 即可启动，准备方法见开发指南。它还通过共用运行时调用本机模型、保存 SQLite 数据。

接下来会接入云端账号和 PostgreSQL，再把模型运行交给独立 Agent。当前服务还没有云端登录和节点认证，不能作为已完成的云端服务部署。
