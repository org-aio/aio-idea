# 宿主启动恢复验收

`startup_recovery.py` 使用真实服务二进制、HTTP 登录和独立 PostgreSQL，验证在线拒绝、首次资源初始化失败、强杀后恢复、显式重试与恢复、锁冲突、来源缺失、端口冲突、写盘失败和损坏记录。全部子进程使用临时缓存及文件目录；清理只终止脚本自己创建的进程。输出日志目录用于检查失败现场。

先准备一个本机独立测试库 `identity_recovery_test`，撤销该库 `public` schema 的公共权限；不能指向现役业务库。使用 Python 3 标准库，无需额外包：

```sh
cargo build --locked --no-default-features --features server
export AIO_RECOVERY_TEST_BINARY="$PWD/target/debug/aio-idea"
export AIO_RECOVERY_TEST_DATABASE_URL='postgresql://postgres@127.0.0.1:5432/identity_recovery_test'
python3 tests/runtime/startup_recovery.py
```

凭据由本机测试环境提供。脚本拒绝非 `127.0.0.1` 或不同数据库名，首次使用空库；重复执行前由测试环境重建专用库。测试会生成随机临时账户密码，不输出或写入恢复记录。完成后删除专用数据库或任务容器。该验收证明 Linux 进程崩溃恢复，不代表操作系统掉电、macOS/Windows 或生产部署验收。
