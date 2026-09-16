# Worker 真实网络验收

`worker-network.mjs` 通过 AIO 正式 HTTPS 入口配对一个临时 Linux worker，使用当前已登录账号提交安全的目录扫描任务。仅需要 Docker 测试机、已有 Node 22 镜像，以及测试机上的 `nsenter` / `iptables`，不修改测试机本身的防火墙。测试用独立 bridge 网络和容器，结束后撤销临时设备并清理容器与网络；设备审计记录留在 AIO。

```sh
AIO_COOKIE_FILE=/private/path/authorized-cookie \
AIO_SPACE_TEST_CLI=/path/to/aio-plugin-space/dist/cli.mjs \
AIO_NETWORK_TEST_HOST=root@192.168.31.252 \
node tests/protocol/worker-network.mjs
```

Cookie 使用 Netscape 格式，只读入内存，不写进报告。CLI 必须是 `npm run build` 生成的单文件 bundle。报告写入 `target/worker-network/result.json`，可用 `AIO_NETWORK_REPORT` 指定位置。测试期间不操作真实用户设备。

验收包括：Docker NAT；无发布端口；拒绝所有新入站连接，且用监听探针和 DROP 计数确认规则有效；仅允许 DNS/HTTPS 出站时任务完成；TCP reset 后恢复；禁止 HTTPS 出站后正确显示离线，放行后恢复；换 Docker 子网和客户端 IP；禁止直连 HTTPS 后通过 HTTP CONNECT 代理完成任务；网页撤权后 worker 实际退出。这里验证的是允许 HTTPS 或允许指定代理的网络，不能把“全出站禁止”宣称为可通信。

本机协议故障测试在空间插件 `test/channel.test.mjs`，服务端协议与租户隔离测试在平台 `generated/worker/tests.rs`。真实远端归档恢复、Mac 升级与 Skill 同步结果记录于 `deploy/252/worker-channel-acceptance.md`。
