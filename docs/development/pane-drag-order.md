# 面板层级与菜单交互

面板层级区分三种操作：在桌面面板之间抬升、一次性前置，以及用户设置的永久置顶。它们分别处理，避免点击、菜单关闭或输入框激活意外改变其他窗口的位置。本文说明窗口顺序；内容与合成树的关系见[架构说明](architecture.md)。

## 实现入口

| 入口 | 职责 |
| --- | --- |
| `app/src/pane/window/mod.rs` | `set_layer`、`raise_among_peers`、`raise_once` 和 `borderless_proc` |
| `app/src/pane/search/mod.rs` | 搜索窗口、原生 EDIT 的定位、层级同步、输入处理及托盘激活 |
| `app/src/pane/hybrid/mod.rs` | 托盘动作与面板显示操作 |

普通分组和文件夹面板共用 `window::create`，使用 Shell owner；搜索面板使用独立窗口，并复用无边框处理和层级设置。搜索 EDIT 是由搜索窗口拥有的独立 popup，不是随父窗口自动排列的 child 控件。

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

多个面板共享 Shell owner，Windows 可能在菜单关闭或激活变化后成组调整 owned 窗口。显式抬升及菜单定位使用 `SWP_NOOWNERZORDER`，避免带动 owner 和其他面板。

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

这些原生测试需要允许跨进程窗口排序的桌面权限。受限沙箱中，`SetWindowPos` 使用其他进程 HWND 作为插入参照可能返回错误 5（拒绝访问），Shell owner 设置也可能受限。出现层级断言失败时先核对原生返回值，再在获准的沙箱外会话中独立复验；不要通过放宽断言或强行前置面板绕过权限失败。

手动验收按以下场景进行：

1. 两个未置顶面板重叠，反复点击和拖动被遮挡的面板，确认只改变面板间顺序。
2. 用普通应用覆盖面板，检查托盘搜索与显示面板操作各自符合预期。
3. 在普通和置顶面板上反复打开菜单及两级子菜单，取消或执行后确认下面的面板没有被意外抬升。
4. 保持搜索输入框聚焦，让其他面板覆盖它后再次点击；切换置顶、移动窗口及跨屏 DPI，确认 EDIT 跟随所属面板。
5. 关闭面板并操作剩余窗口，确认关闭中的 HWND 不再参与层级计算。

Windows 11 x64 是主要验证平台。自动发送消息不能完全覆盖真实鼠标、焦点切换及 Shell 的窗口重排，Windows 10、混合 DPI 和不同 Shell 环境仍需实机验证，见[验证与兼容边界](validation.md)。
