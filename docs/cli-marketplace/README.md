# CLI 插件市场验收

CLI 与普通插件共用市场列表，使用 CLI 类型筛选。安装按钮生成 `aio://install/codex-model-sync?version=0.1.4`，不调用宿主安装接口。原插件即使具有 cli 标签也不会变成本机执行条目。

已完成桌面和手机实际构建页面的浏览器验证，包括类型筛选、平台切换、精确 URI、卸载说明、无宿主安装请求和无横向页面溢出。截图使用测试数据和实际编译前端，不代表线上已部署。

![桌面](desktop.png)

![手机](mobile.png)

```sh
dx build --platform web --release
node tests/browser/cli-marketplace.cjs
```

测试需要 playwright；非默认构建目录通过 AIO_WEB_ROOT 指定。
