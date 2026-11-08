# 面板层级与菜单交互

面板层级区分三种操作：在桌面面板之间抬升、一次性前置，以及用户设置的永久置顶。它们分别处理，避免点击、菜单关闭或输入框激活意外改变其他窗口的位置。本文说明窗口顺序；内容与合成树的关系见[架构说明](architecture.md)。

## 实现入口

| 入口 | 职责 |
| --- | --- |
| `app/src/pane/window/mod.rs` | `set_layer`、`raise_among_peers`、`raise_once` 和 `borderless_proc` |
| `app/src/pane/search/mod.rs` | 搜索窗口、原生 EDIT 的定位、层级同步、输入处理及托盘激活 |
| `app/src/pane/hybrid/mod.rs` | 托盘动作与面板显示操作 |

普通分组和文件夹面板共用 `window::create`，搜索面板使用独立顶层窗口。Win10 普通面板以图标宿主为 Z 顺序参照，搜索面板沿用自身层级规则。Win11 普通面板和搜索面板均为 Shell 拥有的顶层工具窗口，保留 DWM 原生背景画刷。搜索 EDIT 始终是由搜索窗口拥有的独立 popup。

## 行为约束

| 操作 | 层级行为 | 保留的状态 |
| --- | --- | --- |
| 点击或拖动未置顶面板 | 在桌面面板之间抬升，不主动越过上方普通应用 | 置顶偏好、窗口尺寸和位置 |
| 点击搜索输入框 | 抬升所属搜索面板，并同步 EDIT | 输入焦点逻辑与所属面板的置顶状态 |
| 托盘搜索入口 | 沿用桌面层级，不承担跨应用前置功能 | 置顶偏好 |
| 托盘“显示面板” | `raise_once` 一次性前置 | 原有 topmost 状态；普通面板后续仍可被应用覆盖 |
| 用户切换置顶 | `set_layer` 切换 topmost，并更新相关输入窗口 | 其他面板的独立偏好 |
| 打开或关闭菜单 | 菜单可在置顶面板上方显示，不借此重新抬升下面的面板 | 面板之间原有的操作顺序 |

窗口激活、键盘焦点和 Z 顺序不是同一状态。显式层级操作使用 `SWP_NOACTIVATE`，不能仅凭窗口被抬升就推断它应获得键盘焦点。

## 桌面面板之间的抬升

`DESKTOP_LAYER` 是用于识别未置顶面板的窗口属性，不是 Windows 提供的独立桌面窗口层。`desktop_insert_after` 从当前 Z 顺序寻找与目标窗口属于同一 UI 线程、可见、未关闭且带该属性的同类面板，计算插入位置。属性名不能区分实例，其他进程或 UI 线程的同名窗口不能作为同伴；它们及其拥有的窗口仍属于不可随普通点击越过的应用边界。

- 位置已正确时跳过 `SetWindowPos`，避免重复点击产生无意义重排。
- 没有其他面板时，普通抬升不改变位置；`set_layer` 初始化或切回非置顶时才允许以 `HWND_BOTTOM` 为后备位置。
- 遇到 topmost 前驱时使用 `HWND_TOP` 保持非置顶区间，不能直接插到 topmost HWND 后面而改变层级语义。
- 关闭中的窗口通过 `CLOSING_PANE` 排除，不能继续作为抬升参照。

仅处理 `WM_MOUSEACTIVATE` 无法覆盖已经激活、又被其他面板遮挡的情况。`borderless_proc` 同时处理客户区点击、非客户区点击及进入移动循环，并调用 `raise_among_peers`；这一入口不依赖模型回调中的可变借用。

## owner 重排与菜单

桌面图标视图可以在同一个 HWND 不变的情况下从 Progman 移到 WorkerW。`window/desktop_owner.rs` 在每个拥有面板的 UI 线程上共用进程外 WinEvent 监听。两套路径都跟踪桌面窗口对象和前台变化，Win11 另外监听普通应用从最小化恢复。回调过滤事件后投递任务，窗口枚举和层级调整由任务执行。窗口对象事件跳过本进程，前台和最小化恢复事件包含本进程。监听不绑定 Explorer PID。

相关事件合并为一个立即投递的窗口任务，重新解析实际图标宿主，随后以 250 ms、1000 ms 做两次有限重试以覆盖 Explorer 的过渡阶段。新事件会取消尚未执行的延迟重试并立即投递任务，不能排在旧重试后；连续事件合并到已经排队的任务，不推迟其执行时间。接收任务的面板关闭时，将任务转交仍存活的面板。处理完后不再周期唤醒。临时找不到图标视图时保持现有宿主参照，不回退到旧 Progman，等待后续生命周期事件或有限重试。只有 WinEvent 注册失败时降级为 5 秒检查并重试注册。最后一个面板销毁时取消待处理任务和计时器并卸载监听。此功能不依赖诊断日志级别；INFO 模式的只读诊断采样是独立功能，仍按日志指南开启和关闭。

宿主位于面板上方时，通常只在没有可见普通应用阻隔的情况下，将面板放到宿主紧上方；若图标宿主本身处于前台，则夹在宿主和面板之间的窗口已被桌面遮挡，不再阻止恢复，插入位置仍限于宿主紧上方，不越过宿主上方的应用；不激活、不显示隐藏面板、不改变尺寸位置或永久置顶状态。已在正确层级的面板不会重复调用 `SetWindowPos`。最小化或 DWM cloaked 的窗口不作为可见应用边界，DWM 查询失败时保守保留边界。事件送达和排序修复都是异步的，不能保证零延迟；不同系统上的 Explorer 事件覆盖仍需实际 Win+D 验证。

`dynamic_host_switch_preserves_apps_geometry_visibility_and_topmost` 用原生窗口模拟图标视图往返迁移，验证覆盖关系、隐藏/置顶状态、几何、无变化时不重排，以及监听随面板销毁清理；`host_repair_does_not_cross_an_intervening_application` 验证不能为了追随桌面跨过普通应用。`winevent_delivery_coalesces_work_and_stops_after_settling` 用 `NotifyWinEvent` 验证真实进程外回调、事件过滤、合并和重试结束后不保留周期计时器。它们不替代 Windows 10/Server 上实际 Win+D 的复测。

Win10 的普通面板保持无 owner，避免其激活关系进入 Explorer 的显示桌面流程。`active_pane_stays_out_of_desktop_popup_chain_across_host_switches` 覆盖这条路径。Win11 顶层面板使用 Shell owner。显式抬升和菜单定位使用 `SWP_NOOWNERZORDER`，避免带动 owner。

`WM_WINDOWPOSCHANGING` 区分显式操作和系统被动重排：对带 `SWP_NOACTIVATE`、但没有 `SWP_NOOWNERZORDER` 的被动请求禁止改变 Z 顺序；允许的桌面层重排仍按同类面板位置约束处理。不能无条件拦截所有位置消息，否则会破坏用户置顶或正常定位。

`set_layer` 在切换实际 topmost 状态前暂时移除桌面层标记，切回非置顶后再恢复标记与桌面相对位置。`raise_once` 也暂时绕过桌面层约束完成一次前置，随后恢复原标记，不修改持久化置顶偏好。

菜单自身使用 topmost，以覆盖置顶面板；这不应传播成面板永久置顶。菜单循环可能泵送消息，重复打开、关闭与子菜单切换都必须保持 owner 约束。

## 关闭面板与设置窗口

根菜单执行命令时，如果菜单仍在前台，先将激活权交还 owner，再销毁菜单；用户已切到其他窗口时不强行激活。子菜单保留原有返回父菜单的逻辑。

`create_view` 的关闭回调在淡出禁用窗口之前，检查关闭面板是否为前台窗口或前台窗口的根 owner。若设置窗口已显示且未最小化，先将其激活，再开始关闭。原生激活可能同步重入消息处理，因此取出 HWND 后必须释放 `PaneApp` 借用。后台关闭不改变其他应用的焦点，也不恢复已最小化的设置。

设置窗口使用 `WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP`，在任务栏和 Alt+Tab 中可见；关闭设置仍只销毁设置窗口，不退出程序。回归测试 `closing_search_preserves_open_settings_window` 覆盖菜单关闭路径；`closing_search_returns_focus_to_settings` 需要交互桌面，使用 `--ignored --test-threads=1` 单独运行。

## 搜索输入框的跟随规则

搜索 EDIT 的点击不会经过面板 subclass，因此输入框处理器主动调用所属面板的 `raise_among_peers`。即使输入框已经有焦点，后续点击也需要正确处理重叠面板顺序。

面板移动、尺寸或置顶状态变化时同步输入框。只有状态或相对顺序不一致时才重排，避免输入框和面板互相触发重复更新。输入框的被动位置变化仍受 owner 约束，不能独自越过普通应用或留在已取消置顶的面板上方。

新增浮动输入窗口时，应明确 owner、定位方式、焦点与 topmost 同步规则，不能仅复制搜索窗口的样式标志而忽略其消息处理。

## 验证与边界

在可交互 Windows 会话中分别运行以下测试，环境准备见[构建与验证](build.md)：

```powershell
cargo test -p luciddesk --bin luciddesk desktop_panes_raise_among_peers_without_covering_apps --locked --offline -- --test-threads=1
```

搜索测试会激活真实窗口，默认跳过，需单独执行：

```powershell
cargo test -p luciddesk --bin luciddesk editor_click_raises_search_among_panes_but_tray_activation_stays_on_desktop --locked --offline -- --ignored --test-threads=1
```

第一项覆盖共享 owner、重复点击、点击与拖动、菜单及两级子菜单、普通应用约束和置顶切换；第二项覆盖实际搜索窗口与 EDIT 的点击、托盘激活和 topmost 同步。命令过滤后应确认目标测试确实执行，不能把零测试视为通过。

`foreign_ui_thread_panes_are_not_peers_and_remain_application_boundaries` 在另一个 UI 线程创建带同名属性的窗口，验证单面板判断与跨线程应用边界，不依赖桌面是否已有另一个 LucidDesk 实例。标签分离后的窗口顺序由 `tabs_share_one_window_keep_selection_and_release_only_closed_membership` 覆盖。

这些原生测试需要允许跨进程窗口排序的桌面权限。受限沙箱中，`SetWindowPos` 使用其他进程 HWND 作为插入参照可能返回错误 5（拒绝访问），Shell owner 设置也可能受限。原生调用返回成功也不能替代实际 Z 顺序断言。出现层级断言失败时先核对原生返回值和窗口前驱，再用同一个测试二进制在获准的沙箱外会话中独立复验；不要通过放宽断言或强行前置面板绕过权限失败。

手动验收按以下场景进行：

1. 两个未置顶面板重叠，反复点击和拖动被遮挡的面板，确认只改变面板间顺序。
2. 用普通应用覆盖面板，检查托盘搜索与显示面板操作各自符合预期。
3. 在普通和置顶面板上反复打开菜单及两级子菜单，取消或执行后确认下面的面板没有被意外抬升。
4. 保持搜索输入框聚焦，让其他面板覆盖它后再次点击；切换置顶、移动窗口及跨屏 DPI，确认 EDIT 跟随所属面板。
5. 关闭面板并操作剩余窗口，确认关闭中的 HWND 不再参与层级计算。

Windows 11 x64 是主要验证平台。自动发送消息不能完全覆盖真实鼠标、焦点切换及 Shell 的窗口重排，Windows 10、混合 DPI 和不同 Shell 环境仍需实机验证，见[验证与兼容边界](validation.md)。

桌面宿主恢复的应用边界检查忽略矩形面积为零的辅助窗口，即使它带有 `WS_VISIBLE`；尺寸查询失败时仍保守保留边界。`host_repair_crosses_zero_area_helpers_but_keeps_apps_above` 覆盖辅助窗口夹在宿主与面板之间、真实应用仍位于面板上方的情况。INFO 日志中的 `repair blocked` 记录阻止提升的窗口及矩形，供实机区分真实应用约束与辅助窗口误判。旧日志没有矩形，不能仅凭 `Internet Explorer_Hidden` 类名认定它就是零面积窗口。

托盘“显示所有面板”只逐个执行不激活的临时抬高，不调用 `SetForegroundWindow` 激活桌面面板。托盘菜单选中命令时不先调用 `NIM_SETFOCUS` 把焦点交回 Explorer；仅取消菜单时返回托盘焦点。临时显示测试同时断言面板确实超过普通窗口，以及之后普通窗口仍能覆盖面板；显示全部的回归测试检查前台窗口不变。Win10 实机需检查 `show_all no-activate` 和 `reveal-before/after`，并验证随后 Win+D 能恢复其他应用窗口，不能把 `SetWindowPos` 成功当成层级已改变的证明。

宿主实际变化成功后以及显式显示面板后，额外请求内容重绘；宿主未变化的同步不产生重绘请求。回归测试验证失效区域和已有层级约束。此举用于处理窗口关系恢复后没有新帧的问题，仍需 Win10 验证，不能将 HWND 可见、合成透明度为 1 或失效区域存在视为屏幕像素已经恢复的证据。

标题栏、缩放边缘和内容区保留正常鼠标激活，不再靠禁止标题栏激活绕过 Win+D 问题。`desktop_frame_and_content_preserve_normal_mouse_activation` 验证原生消息路由；真实长拖、内容区键盘交互后连续两次 Win+D 恢复其他应用仍需 Win10 实机验证。

Win10 在宿主切换或跟踪到普通应用从显示桌面返回时，将合适的面板归位到宿主紧上方。Win11 归位到仍位于面板下方的普通应用后方，避开桌面图标窗口组。隐藏、最小化及永久置顶面板不参与归位。普通稳定状态的同步保留显式“显示所有面板”的结果。`returning_host_lowers_pane_below_restored_apps_without_changing_focus` 验证 Win10 式的归位和焦点行为。

屏幕外辅助窗口同样不作为应用边界：矩形查询成功后，用 MonitorFromRect / MONITOR_DEFAULTTONULL 检查与实际显示器的交集，避免 NarratorHelperWindow 的非零矩形（日志中为 -48000,-48000 至 -47976,-47976）阻止恢复。不能简单排除负坐标，负坐标显示器和部分可见窗口仍必须受保护。host_round_trip_crosses_offscreen_helper_without_covering_restored_apps 覆盖连续三次宿主往返、屏幕外辅助窗口、恢复应用覆盖面板、焦点与非置顶状态不变，以及部分屏幕外窗口仍构成边界。

宿主上移恢复、下移归位均沿 GetTopWindow / GetWindow 收集实际 Z 顺序，用已访问句柄集合检测环，不再以 512 个窗口截断；遍历期间不调整层级，检测到环或目标不存在则保持现状。crowded_desktop_recovers_and_lowers_without_crossing_apps 创建 520 个隐藏辅助窗口，验证跨越旧上限的恢复和归位、间隔内真实应用仍阻止提升、焦点与置顶状态不变。此修复仍由既有 WinEvent 和有限重试触发，不增加周期轮询。

Win11 面板或其搜索 EDIT 持有焦点时，Win+D hook 先把焦点交回 Shell，再继续传递快捷键。Shell 前台期间记录待归位的面板，普通应用恢复前台或退出最小化时触发归位。标记保留到已有的有限重试结束，以覆盖 Explorer 异步调整层级的阶段。“显示所有面板”对 Shell 拥有的非置顶面板同步切换 TOPMOST / NOTOPMOST 来完成抬升。

桌面子窗口试验已撤回。Win11 探针中，设为 `WS_CHILD` 后 DWM 启用原生背景画刷返回 `0x80070006`。Hyper-V 的 Win10 试验还在 Progman 和 WorkerW 切换期间观察到闪烁。

`real_shell_show_desktop_preserves_pane_pixels` 默认忽略，须在交互桌面单独运行。测试在隔离虚拟桌面创建面板和具有独立输入队列的普通应用，执行真实 Win+D，覆盖显式显示、拖动、普通面板焦点和搜索框焦点，采样两种面板的不透明内容标记，并断言普通应用恢复后能盖住面板。Win11 还检查两种面板的原生亚克力画刷在往返后仍存在。内容采样的间隔受屏幕读取耗时影响，画刷检查也不测量实际背景模糊。
