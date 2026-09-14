# 运行时插件内容区高度验收

2026-09-14，已部署并通过公网记忆图谱布局验收。

## 修复与发布

宿主 iframe 原来固定为 640px，导致高屏下插件底部留下大片空白。共享 PageDeck 现在继承内容区高度，运行时 iframe 使用共享 `.application-frontend` 样式填满容器。保留桌面 24px、手机 16px 内容边距，插件自行处理内部滚动，不需要分别修改或重新发布插件。

- 宿主发布提交：`2516ca1366e615fc5f1dad78efdfe5ad338dfa11`。
- 共享 workbench 提交：`1e1befcff82210d3b047f8635bdb9d3c0db5ab73`。
- 252 激活目录：`/opt/aio-idea/releases/2516ca1366e615fc5f1dad78efdfe5ad338dfa11`。
- 两仓提交均已推送，发布脚本已检查服务健康和实际运行二进制路径。

## 验证结果

公网 `https://aio.addzero.site`，现有隔离验收租户中的真实记忆图谱：

| 浏览器视口 | iframe 尺寸 | 超出正常边距的底部空白 | 横向或外部纵向溢出 |
| --- | --- | --- | --- |
| 1920×1200 | 1632×1096 | 0px | 无 |
| 1440×1000 | 1152×896 | 0px | 无 |
| 390×844 | 358×756 | 0px | 无 |
| 844×390 | 556×286 | 0px | 无 |

真实 `/graph` 服务返回 200；四种尺寸的 Compose canvas 均正常绘制，并保持原 JS 实例。新建记忆弹窗的取消按钮在所有尺寸均处于 iframe 内。横屏下正文滚动造成 4299 个画布像素变化，截图确认能滚动到标签、别名字段，最终取消表单，未保存数据。最终验收浏览器错误 0，外部字体拦截警告 0。

本地真实 Compose 与 v2 协议夹具另验证：窗口缩放和侧栏折叠不重新挂载、场景切换保留实例、原生长页面可滚动、账户全屏 iframe 填满内容区。发布检查 56 项通过、9 项环境依赖测试跳过；Web 检查、Web 发布构建和 Linux glibc 2.17 构建通过。

## 证据

- 本地布局报告与截图：`target/frontend-layout/`。
- 公网记忆图谱报告与截图：`target/component-delivery/memory-workbench/`，包括 `desktop-tall-dialog.png`、`mobile-dialog.png` 和 `mobile-landscape-dialog-scrolled.png`。
- 发布日志：`target/layout-deploy.log`。
- 公网验收日志：`target/layout-memory-live.log`。

布局随新壳资源加载生效；此前已打开并运行旧壳代码的浏览器需要刷新一次。后续插件导航和窗口缩放无需刷新。
