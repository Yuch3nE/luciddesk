# 桌面 Hook：逻辑、逐帧与性能验证

本页描述当前源码的桌面切换路径，以及如何判断修改是否有效。用户操作见[使用说明](../usage.md#桌面面板)，成员过滤与退出清理见[桌面架构](hybrid-desktop.md)。尚未发布的变化单列于[更新记录](../../CHANGELOG.md)。

## 当前行为

- 在 Explorer 桌面线程现有的 `WH_GETMESSAGE` hook 中识别空白处双击，不安装全局低级鼠标 hook。图标命中、修饰键、拖动及中间输入会影响配对；没有待配对按下时，鼠标移动直接返回。
- 识别成功后通知主程序；请求携带消息时间戳。主程序丢弃超过 1 秒的旧请求，并在执行前再次检查，避免桌面忙碌后重放旧手势。时钟差使用回绕安全的毫秒计数。
- 隐藏时暂时隐藏原生 ListView，面板执行淡出；恢复时在 Explorer 内合并 ListView 显示与绘制，再通知面板淡入。不修改持久化的“显示桌面图标”选项，也不以刷新整个 Shell 代替显隐。
- 面板复用现有 160 ms 动画，淡出途中恢复可反向；系统关闭动画时直接切换。原本隐藏、最小化和隐藏期间新建的窗口按既有状态恢复。
- 图标恢复请求完成只说明原生调用和绘制已执行，不保证 DWM、背景材质与另一进程在同一个刷新帧完成合成。

源码入口：`crates/luciddesk-explorer/src/filter.rs`、`filter/engine.rs`、`filter/desktop_visibility.rs`、`app/src/pane/desktop_toggle.rs` 和 `app/src/pane/visibility.rs`。

## 先验证正确性

在 Windows 交互桌面上运行；原生窗口测试串行执行，不与逐帧采样同时运行。使用隔离配置与测试面板，记录测试前的图标状态。结束后核验图标、光标和原应用已恢复，不用用户布局作为测试夹具。

```powershell
cargo test -p luciddesk-explorer --lib --locked -- --test-threads=1
cargo test -p luciddesk --bin luciddesk desktop_toggle --locked -- --test-threads=1
cargo test -p luciddesk --bin luciddesk all_pane_types_fade_and_close_after_the_transition --locked -- --test-threads=1
```

下列光标测试默认忽略，须在允许临时操作桌面的会话中显式执行。它短暂显示自有窗口并恢复原光标：

```powershell
cargo test -p luciddesk-explorer --lib --locked filter::desktop_visibility::tests::busy_pointer_is_restored_only_over_our_desktop_target -- --ignored --exact --test-threads=1
```

确认旧实例已正常退出，再构建并运行桌面端到端测试。主程序、CLI 与 hook DLL 必须来自同次构建：

```powershell
cargo build -p luciddesk -p luciddesk-cli -p luciddesk-explorer --locked
python tools/validation/test-desktop-toggle.py --executable target/debug/luciddesk.exe --cli target/debug/luciddesk-cli.exe --expect-consumed-double-clicks
```

`--expect-consumed-double-clicks` 使用 Debug 计数器，Release 测试应省略。脚本向实际桌面窗口发送合成的 `DOWN/UP/DBLCLK/UP` 消息，经实际 hook 处理；这不是物理鼠标验收。覆盖过期请求、原先隐藏状态、新窗口、功能关闭、托盘、连续切换及正常/异常退出后的恢复。淡出需要等待完成，不能在期望状态刚改变时断言 HWND 已隐藏。

如需覆盖可选监控面板，先构建并放入测试程序的插件目录，再在上面的 Python 命令末尾加 `--with-monitor`：

```powershell
cargo build -p luciddesk-monitor --locked
New-Item -ItemType Directory -Force target/debug/plugins | Out-Null
Copy-Item target/debug/luciddesk_monitor.dll target/debug/plugins/luciddesk_monitor.dll -Force
```

普通发布包不包含监控插件。测试目录缺少插件导致的“找不到监控窗口”不应被解释为显隐回归。使用自定义 `--target-dir` 时，上述所有路径应一并调整。

## 逐帧检查恢复

同时拍到面板和原生图标，至少覆盖云母、亚克力、纯色及深浅主题。记录隐藏参考图、每帧时间、最终稳定参考图和 `LUCIDDESK_RENDER_TRACE` 日志；保留普通显示淡入作为阳性对照，确认采样确实能捕获动画。

| 指标 | 能说明什么 | 不能据此推断什么 |
| --- | --- | --- |
| 请求入队到处理 | 消息与调度延迟 | 材质或图标已完成绘制 |
| HWND 可见状态 | 原生窗口是否显示或隐藏 | 淡入结束、没有闪烁 |
| 透明度记录 | 应用动画的起点、方向和终值 | 系统背景采样已完成 |
| 像素与最终图的差异 | 采样范围内是否仍在变化 | 未采到的显示帧也没有问题 |

将图标区域、面板内容及背景分别比较，避免把透明面板后方的图标变化当作面板透明度。GDI 连续采样不等同于显示器逐刷新帧：报告典型及最大采样间隔，保留重复帧和漏采限制。控制器直接触发可用于隔离渲染，但必须明确它绕过了手势识别，不能替代完整双击测试。

若文字先出现、背景稍后改变，对比“图标与面板一起恢复”和“只恢复面板”；再逐项比较合并绘制、合成树或系统过渡。不要用固定等待掩盖问题，也不要把不同类型的窗口测试与像素采样并行执行。

## 消融与性能比较

复制当前源码和工作区修改，每次只移除一个因素。保持主程序、配置、面板数量、输入序列和机器一致，记录产物哈希；正序、反序各运行一轮，减少顺序影响。测试变体仅在隔离目录运行，结束后重新构建正常 DLL。

| 对照 | 关注点 |
| --- | --- |
| 完整逻辑 | 正确性与时序基线 |
| 关闭手势分派 | 预期不发生切换；CPU 减少包含整个动画和绘制工作，不能全算作 hook 收益 |
| 不消费双击消息 | 是否进入 Shell 默认动作；识别计数不等于消费计数 |
| 不恢复光标 | 普通操作与强制 WAIT / APPSTARTING 场景都要测 |
| 不合并 ListView 恢复绘制 | 同时比较 HWND 可见时间和像素稳定时间 |
| 去掉命中检测 | 只在自有图标夹具中检查误触发，不对用户文件执行危险输入 |
| 去掉空闲移动快速返回 | 高频鼠标移动的额外 Win32 查询开销 |

隐藏和恢复分开统计。隐藏耗时包含淡出；恢复的 HWND 可见时间不包含动画完成。微基准须注明编译优化、预热、批大小及输入状态，使用优化屏障；批均值的 P95 不是单条消息的 P95。无会话的 hook 入口调用也不代表真实钩子链耗时。

进程 CPU 时间包含动画、Shell、后台活动及其他回调。紧邻启动的短采样不能代表长期空闲；只有相同工作量、足够重复和独立证据支持时，才将差异归因于被移除的逻辑。

## 2026-10-21 验证快照

本次测试基于 0.22.1 之后的未发布工作区，Windows x64。以下是单机样本，不是跨机器性能承诺，也不意味着之后的提交已经重新验证。

- 桌面对照共 120 组合成双击：96 次预期切换、24 次预期不切换；40 个单击、慢双击、拖动、Shift 负例检查通过。未观察到非预期状态切换。
- 完整组隐藏中位数约 201.5 ms（含淡出），恢复到 HWND 可见约 45.2 ms。移除合并绘制后约 19.0 ms 可见，但不能据此认定像素恢复更快。
- 优化微基准中普通空白按下约 2 µs；移除命中检查降至约 0.34 µs，但图标命中回归按预期失败。移除空闲移动快速返回增加约 2 µs 查询开销。
- 普通操作未复现忙碌光标；强制光标测试中完整逻辑通过，移除恢复逻辑后按预期失败。未发现足以支持删除双击消费逻辑的性能证据。
- Explorer 库 62 项通过、5 项默认忽略；光标用例另行显式运行通过。桌面请求生命周期 6 项通过。

原始记录保存在当次工作机的 `target/hook-ablation-v2/`，不随仓库分发。此前逐帧记录在 `target/restore-frames-final/`；本次消融没有重新采集像素帧。历史详细验证见[验证历史](validation-history.md)，后续修改须重新执行相关检查。
