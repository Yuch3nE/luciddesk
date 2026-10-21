# 开发文档

这里介绍 LucidDesk 当前实现、代码维护入口和验证方法。Explorer 管理未收纳的原生桌面图标，LucidDesk 自绘分组内容，通过成员过滤 Hook 隔离已收纳项目，并由独立 Shell 宿主提供原生菜单。

首次参与开发建议按下面的顺序阅读；处理具体问题时，可直接使用任务导航。用户介绍见[项目 README](../../README.md)，贡献与数据要求见[贡献指南](../../CONTRIBUTING.md)。

## 开始开发

1. 阅读[构建与验证](build.md)，准备 Windows、Rust 和本地构建工具，完成与任务相关的基础检查。
2. 阅读[架构说明](architecture.md)，了解主程序、Explorer 内组件和 Shell 宿主之间的关系。
3. 使用[目录结构](structure.md)找到实现入口，再阅读相应专题；涉及原生接口时同时核对 [Rust API 与资源生命周期约定](rust-api-review.md)。
4. 按[验证与兼容边界](validation.md)选择测试，记录本次执行结果、实机观察及未覆盖范围。

## 按任务查找

| 要处理的任务 | 建议阅读顺序 |
| --- | --- |
| 构建、打包或发布 | [构建与验证](build.md) → [包验收与兼容边界](validation.md)；MSIX 另见[打包与运行](../msix.md) |
| 双击隐藏、恢复闪烁或 hook 性能 | [Hook 验证](hook-validation.md) → [绘图与绑定](rendering.md) |
| 图标收纳、释放或 Explorer 恢复 | [成员过滤](hybrid-desktop.md) → [选择与刷新时序](selection-latency.md) → [后台调度](event-driven-runtime.md) |
| 菜单、重命名、选择或焦点异常 | [菜单与重命名](pane-item-rename.md) → [面板层级](pane-drag-order.md) → [选择时序](selection-latency.md) |
| CLI、Agent 或 Skill | [CLI 接口设计](cli-agent.md) → [用户命令指南](../cli.md) → [Pane 模块边界](pane-structure.md) |
| 绘制、材质、首帧或资源占用 | [绘图与绑定](rendering.md) → [背景材质](mica-materials.md) → [图标内存管理](memory-optimization.md) |
| 标签切换、排序或窗口合并 | [普通面板标签页](pane-tabs.md) → [面板层级](pane-drag-order.md) → [存储](storage.md) |
| 设置页、语言或字体布局 | [设置组件](settings-components.md) → [多语言](localization.md) → [绘图](rendering.md) |
| 配置、数据兼容或备份恢复 | [配置与工作区存储](storage.md) → [API 与提交边界](rust-api-review.md) → [验证要求](validation.md) |
| 托盘入口或任务栏重建 | [运行时托盘](tray.md) → [架构说明](architecture.md) |
| 插件接口与新插件 | [插件开发指南](plugins.md) → [SDK 示例](../../crates/luciddesk-plugin-api/examples/minimal.rs) |
| 可选监控插件（默认发行包不包含） | [原生系统监控面板](system-monitor.md) → [绘图与绑定](rendering.md) → [存储](storage.md) |

## 文档索引

### 工程基础

| 文档 | 内容 |
| --- | --- |
| [开发更新日志](changelog.md) | 各版本的内部实现、接口、构建与测试变更 |
| [构建与验证](build.md) | 环境准备、构建命令、工具链、打包、CI、发布与版本编号 |
| [架构说明](architecture.md) | 模块和进程职责、启动流程、通信及退出恢复 |
| [Pane 模块边界](pane-structure.md) | 面板子模块、状态所有权、重入与布局边界 |
| [CLI 与 Agent](cli-agent.md) | 本地通信、控制开关、计划提交与技能安装入口 |
| [目录结构](structure.md) | 源码目录、功能归属、工具与新增文件约定 |
| [Rust API 与资源生命周期约定](rust-api-review.md) | 所有权、COM 线程、重入、错误与 unsafe 边界 |
| [日志与诊断](logging.md) | 日志格式、内存保留、轮转、故障恢复与复制报告 |
| [验证与兼容边界](validation.md) | 检查层次、实机流程、包验收、平台限制与结果记录 |

### 桌面与交互

| 文档 | 内容 |
| --- | --- |
| [原生桌面与成员过滤](hybrid-desktop.md) | 身份、收纳、过滤名单、组件加载与恢复 |
| [后台调度](event-driven-runtime.md) | 事件唤醒、在途请求、维护期限与空闲退避 |
| [选择与刷新时序](selection-latency.md) | 双向选择、清单校验、过滤提交和确认 |
| [图标菜单与重命名](pane-item-rename.md) | Shell 宿主、编辑窗口、身份交接与提交后恢复 |
| [面板层级](pane-drag-order.md) | 桌面层级、临时抬升、置顶及菜单焦点 |
| [普通面板标签页](pane-tabs.md) | 状态归属、切换、关闭、排序、分离和合并 |
| [运行时托盘](tray.md) | 操作入口、延迟分派、定位、重建与释放 |

### 界面与数据

| 文档 | 内容 |
| --- | --- |
| [绘图与绑定](rendering.md) | 绘制、呈现背压、失败恢复、资源复用、绑定转换与首帧 |
| [背景材质与主题](mica-materials.md) | 材质参数、主题、系统策略及回退 |
| [图标内存管理](memory-optimization.md) | 像素共享、CPU/GPU 缓存预算及回收 |
| [设置页公共组件](settings-components.md) | 场景与 Action、动态布局、滚动、字体搜索和测试 |
| [多语言](localization.md) | Fluent 资源、语言注册、字体回退及布局验证 |
| [配置与工作区存储](storage.md) | TOML 与数据库分工、限定升级、保存和备份恢复 |

### 相关文档与工具说明

| 入口 | 用途 |
| --- | --- |
| [MSIX 打包与运行](../msix.md) | 包身份、标记、签名、DLL 缓存及更新 |
| [配置示例](../config.example.toml) | 可编辑 TOML 字段和默认值示例 |
| [品牌规范](../brand.md) | 名称、标识与数据目录兼容规则 |
| [渲染诊断](../../tools/render-diagnostics/RENDER-TEST.md) | 诊断变体、开关与日志 |
| [崩溃转储](../../tools/render-diagnostics/CRASH-DUMPS.md) | 转储采集、符号与恢复步骤 |

## 文档维护约定

只描述当前实现和仍适用的约束；已替代方案、旧测量与临时排查过程通过 Git 历史查询。实现保留的兼容读取规则仍需说明，不能为了删去历史叙述而删掉有效约束。

- 用户更新日志只记录可感知的变化与升级说明；内部重构、接口、构建及测试变更记入[开发更新日志](changelog.md)。
- 用户介绍与使用说明放在 `docs` 根目录；模块机制和开发约束放在本目录，工具操作细节随工具维护。
- 正文使用中文，保留 API、命令和必要原始错误信息；相对源码路径以仓库根目录为基准，Markdown 链接以文档目录为基准。
- 专题文档负责具体规则，本入口负责导航。调整行为、默认值、路径、测试或兼容范围时，同步所属专题和必要的交叉链接。
- 文档中的命令是执行入口，不是已经通过的证明。区分自动测试、快照检查与真实桌面结论，不将调度参数写成性能保证。
- 新增、重命名或移除文档时更新索引，核对本地链接与锚点；新增页面、语言或测试范围时核对代码中的显式枚举。
- 文档优化按专题逐份核对源码，保留最终实现方式；测试日志与一次性验收结果放在提交、问题报告或本地产物中。

插件源码目录和统一构建入口见 [plugins/README.md](../../plugins/README.md)；`tools/validation/test-plugins.ps1` 覆盖 SDK、DLL 与宿主有/无插件两种路径。
