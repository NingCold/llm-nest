# 运行内网测试服务

[服务说明](../README.md) · [验证指南](../../../docs/validation/README.md)

这套 Compose 在一台 Linux 服务器运行 PostgreSQL、聊天服务和 HTTPS 入口。只有 HTTPS 的 8443 端口发布到指定 Tailscale 地址，数据库和应用端口留在容器网络内。当前配置使用私有测试收件箱，不发送真实邮件。

服务器需要 Docker Compose。先在客户端源码目录构建网页，再把源码和 `client/web/dist` 放到服务器；不要复制个人配置、模型密钥或聊天库。Dockerfile 会编译独立的 `cloud-server`，无需启动模型服务。

## 准备目录与证书

以下路径和地址都是示例，替换为自己的测试目录与 Tailscale 地址：

```bash
mkdir -p /srv/llmn-test/private/tls /srv/llmn-test/data/{attachments,outbox}
chmod 700 /srv/llmn-test/private
sudo chown 10001:10001 /srv/llmn-test/data/{attachments,outbox}
sudo chmod 700 /srv/llmn-test/data/{attachments,outbox}
python3 scripts/cloud-test-certificate.py /srv/llmn-test/private/tls 100.64.0.10
```

脚本生成九十天有效的测试 CA 和三十天有效的服务证书。私钥留在服务器；将 `ca.crt` 提供给测试客户端，只在测试连接中信任它。重新运行脚本会生成新的证书，需要同步客户端使用的 CA。正式使用应换成客户端已经信任的证书。

在 `/srv/llmn-test/private/cloud.env` 填写以下内容，文件权限设为 `600`，不提交到仓库：

```dotenv
POSTGRES_PASSWORD=填写随机生成的十六进制密码
CLOUD_PUBLIC_URL=https://100.64.0.10:8443
CLOUD_DATA_DIR=/srv/llmn-test/data
CLOUD_CERT_DIR=/srv/llmn-test/private/tls
TAILSCALE_IP=100.64.0.10
```

在仓库根目录构建网页并启动：

```bash
pnpm --dir client/web build
docker compose --env-file /srv/llmn-test/private/cloud.env -f server/cloud/deploy/compose.yaml up -d --build
curl --cacert /srv/llmn-test/private/tls/ca.crt https://100.64.0.10:8443/health
```

`health` 返回 `ok` 表示 HTTP 服务可达；账号、聊天与附件还需要下方的完整测试。检查 `docker compose ps` 和 `ss -lntp`，确认只有指定的内网地址发布 8443，未发布数据库端口。

## 邀请与测试邮件

```bash
docker compose --env-file /srv/llmn-test/private/cloud.env -f server/cloud/deploy/compose.yaml exec app cloud-server invite 7
docker compose --env-file /srv/llmn-test/private/cloud.env -f server/cloud/deploy/compose.yaml exec app cloud-server revoke-invite 邀请码
```

第一个命令返回邀请码，交给受邀人使用，也可以组成站点的 `/#/register?invite=邀请码` 链接。邮件写入 `data/outbox` 下的 JSON 文件，包含收件人和验证或重置链接。维护者在服务器上读取对应邮件，不把整个目录公开到网站。

切换到真实邮件时，移除应用的 `CLOUD_MAIL_OUTBOX`，通过私有环境配置 `CLOUD_SMTP_HOST`、`CLOUD_SMTP_FROM` 和需要的端口、用户名、密码。完整变量见[服务说明](../README.md)。测试收件箱通过不代表真实邮件已经送达。

桌面测试连接可在启动进程时设置 `CLOUD_CA_CERT_FILE=/绝对路径/ca.crt`，无需关闭证书校验。网页使用浏览器自己的证书信任设置。客户端里填写与证书一致的 HTTPS 地址。

## 复验与备份

`scripts/cloud_acceptance.py` 会创建三个独立测试账号，检查邀请与验证、附件、重复发送、第三账号隔离，并通过另一台 SSH 设备登录和回复，最后重启应用容器确认数据保留。它不会调用模型或读取日常聊天；输出文件包含测试登录资料，放在忽略目录中。

```bash
python scripts/cloud_acceptance.py \
  --url https://100.64.0.10:8443 --ca /本机路径/ca.crt \
  --server-host 测试服务器SSH别名 --server-root /srv/llmn-test \
  --client-host 第二台客户端SSH别名 \
  --output target/cloud-work/accounts.private.json
```

脚本假定服务器目录下有 `src/` 源码、`private/cloud.env` 和 `data/outbox`，SSH 用户可使用 `sudo -n` 运行测试容器命令。

备份时先停止应用写入，保留 PostgreSQL 导出和同一时刻的附件目录，再恢复应用。恢复时两者一起恢复，核对附件引用。不要用删除持久卷的方式更新服务；日常更新只重新构建并启动应用容器。
