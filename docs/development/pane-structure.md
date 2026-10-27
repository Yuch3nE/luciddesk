# Pane 模块边界

`pane` 目前仍是桌面 UI 的组合入口。优先按状态所有权和副作用拆分，暂不移动设置、搜索、控制协议的公共模块路径。

| 入口 / 子模块 | 职责 |
| --- | --- |
| `events/mod.rs` | 分发事件；先完成无需长期持有应用状态的交互，再进入面板状态修改 |
| `events/appearance.rs` | 外观预览、选项保存与失败回滚 |
| `events/lifecycle.rs` | 面板创建、搜索启用、关闭与窗口销毁 |
| `events/panel.rs` | 单面板状态、几何及项目移动 |
| `events/shell.rs` | 复制 Shell 操作所需数据，延迟执行文件操作 |
| `events/tab_events.rs` | 标签命令，复用现有 `tabs` 业务实现 |
| `window/mod.rs` | 原生窗口创建与消息顺序协调 |
| `window/context_menu.rs` | 菜单内容路由与事件提交 |
| `window/input.rs` | 指针、命中、网格、滚动条与拖动预览计算 |
| `window/interaction.rs` | 图标拖动状态、预览释放和目标身份解析；原生捕获及事件分派仍由窗口入口协调 |
| `window/scheduling.rs` | 延迟回调队列、执行和取消 |
| `window/hit_tests.rs` | 窗口层级与命中回归测试 |
| `control/content_layout.rs` | 合并已保存与计划内位置，构造只读布局上下文并分发操作 |
| `control/content_layout/measurement.rs` | 内容计数、文件夹就绪快照、网格尺寸和查询结果 |
| `control/content_layout/fitting.rs` | 单个面板内容适配与工作区边界限制 |
| `control/content_layout/snapping.rs` | 相对面板吸附、对齐及碰撞校验 |
| `control/content_layout/arrangement.rs` | 多列排列、共享窗口去重及工作区容量校验 |
| `control/content_layout/tests.rs` | 布局与无写入回归测试，保留原测试模块路径 |
| `composition/recovery.rs` | 绘制失败时释放窗口资源、有限延迟重试及错误去重 |
| `canvas/brushes.rs` | 固定画刷数组复用，颜色更新及图形上下文失效重建 |
| `settings/` | 设置窗口、操作、外观转换、字体编辑框与生命周期 |

## 目录约定

窗口和业务模块包含多个实现文件时使用 `<module>/mod.rs` 作为入口，入口与子模块放在同一目录，不使用 `#[path]` 维持分散的文件布局。`canvas.rs` 与 `composition.rs` 保留共享图形入口，专用的画刷和恢复帮助模块放在同名目录内。`pane/mod.rs` 按应用状态、内容配置、窗口交互、渲染资源分组声明模块；独立且没有子模块的小文件保留在顶层。

```text
pane/
├── mod.rs              # 组合入口与共享状态
├── acrylic/mod.rs      # 材质入口，与 effects、host、runtime 等实现相邻
├── control/mod.rs      # CLI 控制入口，与计划及布局实现相邻
├── events/mod.rs       # UI 事件分发
├── folder/
│   ├── mod.rs          # 文件夹面板入口
│   └── tests.rs        # 文件夹回归测试
├── hybrid/mod.rs       # 桌面同步与后台调度入口
├── render/
│   ├── mod.rs          # Renderer 实现
│   ├── tests.rs        # 绘制回归测试
│   └── bench.rs        # 忽略的绘制性能测试
├── settings/mod.rs     # 设置组合入口
└── window/mod.rs       # 原生窗口入口
```

其余既有子模块省略。此次归组保留 Rust 模块路径和可见性，文件夹与渲染回归测试名称不变；渲染基准测试路径从 `pane::render_bench::` 调整为 `pane::render::bench::`。运行测试应按 Rust 模块路径筛选，而不是按文件名推断。

## 所有权约束

- 窗口层只提交事件，不直接写数据库；保存和回滚保留在事件及控制层。
- 菜单、Shell 调用、窗口销毁可能同步处理其他窗口消息。先复制所需 ID、路径或窗口句柄，再释放 `PaneApp` / 模型借用。
- 回调队列先取出任务，再调用或释放捕获对象；任务执行、任务析构都可能再次访问队列。
- 关闭窗口时先从应用状态取出窗口，再释放状态借用，最后销毁原生窗口。
- 不为了消除借用冲突而静默丢弃事件；保留明确的重试、错误和回滚语义。

图标拖动通过 `ItemDrag` 集中保存按下位置、偏移、身份和预览。结束预览时保留身份，供鼠标释放后重新定位项目；显式取消和 OLE 交接清除身份。`ReleaseCapture` 可能同步触发捕获丢失消息，不能把身份清理合并到所有取消路径。

`GroupModel::apply_options` 统一网格变化后的模型更新，调用方传入工作区原选项及滚动策略。UI 预览和直接设置保留横向位置，加载设置时重置双轴；列表视图保持滚动和悬停状态。持久化、窗口失效通知与失败回滚仍由各入口负责。

控制计划的 `PreparedAction` 区分工作区、设置和运行时操作，各自保存提交所需数据。提交失败保留计划供重试；成功后转换为无负载的 `Applied`，仅保留拒绝重复提交所需的公共信息，异步执行数据转交回执。

## CLI 布局边界

内容布局入口只读取一次显示器布局，再覆盖同一计划中尚未提交的位置。各布局算法接收同一个只读 `LayoutContext`，返回 `Operation::Geometry`，不持有数据库对象、不执行保存，也不修改工作区。现有控制计划层继续负责验证、提交和失败处理。

内容测量独立于放置算法；修改图标网格或文件夹就绪判断时从 `measurement.rs` 入手，增加放置方式时增加对应算法并接入入口分发。保持校验与错误顺序，尤其是文件夹快照、标签共享窗口、负坐标和分数 DPI。

## 后续拆分原则

500 行以上的文件、100 行以上的函数作为人工检查信号，不设机械截断。按完整职责提取，避免把大闭包搬到另一个文件后宣称复杂度已经消失。`window/mod.rs` 的原生消息协调、`render/mod.rs` 和 `search/mod.rs` 仍是后续重点；其消息顺序和渲染状态需要各自验证。

涉及窗口层级、焦点和桌面全局状态的测试应关闭运行中的 Debug 后串行运行，完成后恢复应用。只读数据及纯布局测试可并行。专门标记为 ignored 的交互 / GDI 测试按各自说明单独执行，不能把未运行项计为通过。

控制层原生窗口测试还存在同进程顺序依赖：当前环境中整组串行执行在 `presentation_removes_windows_without_resetting_transient_collapse` 发生访问冲突，布局拆分前的二进制也能复现。单独执行该测试和按测试逐个启动独立进程均通过；需要完整验证控制层时，使用独立进程隔离，并记录整组运行失败，不能把它当作整组通过。
