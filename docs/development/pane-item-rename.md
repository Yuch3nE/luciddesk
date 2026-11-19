# 图标菜单与重命名

本文说明文件菜单、内联编辑和文件改名后的身份交接。面板或标签标题也复用编辑器，但只修改应用模型，不执行文件改名；命令路由见[Win11 精简菜单技术文档](../win11-compact-menu-command-routing.md)。

## 实现入口

| 入口 | 职责 |
| --- | --- |
| `app/src/pane/shell_menu.rs` | 菜单准备、打开和结束／取消 |
| `app/src/pane/rename.rs` | 原生 EDIT、输入法、文本选择、提交及销毁 |
| `app/src/pane/hybrid/rename_transaction.rs` | 托管桌面成员的身份交接与失败补偿 |
| `crates/luciddesk-shell/src/rename.rs` | `IFileOperation` 与 Shell 返回的新项目 |
| `crates/luciddesk-explorer/src/filter/menu/` | Explorer 内的独立宿主与命令路由 |

菜单返回的是操作意图，Shell 返回的是文件操作结果，两者不能混为一谈。打开编辑器时保留当初选中的身份，不根据菜单关闭后的选择状态重新猜测目标。

## 独立菜单宿主

`app/src/pane/shell_menu.rs` 将选中项目的 Shell 解析名、owner 和菜单锚点交给 `FilterSession::prepare_menu`。Explorer 内的菜单 STA 创建或复用独立 Shell 视图，重新解析并校验目标身份。真实桌面持续过滤分组成员，菜单使用独立的选择状态。

控制端安装菜单观察器后，通过 `luciddesk_shell::show_isolated_item_menu` 打开菜单，保留鼠标与键盘来源。菜单宿主使用空窗口区域，不显示图标；其职责是提供 Shell 上下文、原生菜单和命令服务。准备和弹出可能泵送消息，调用期间必须释放应用及模型借用。

宿主将原生注册消息转给本次 Presenter，普通命令继续交给 Shell。重命名按本次菜单编号范围查询标准 `rename` 动词，转换为 Pane 编辑请求，不写死命令编号，也不依赖隐藏视图一定产生 `LVN_BEGINLABELEDIT`。缺失精简菜单能力时使用经典菜单降级。平台验证状态见[验证与兼容边界](validation.md)。

正常关闭调用 `finish_menu`，保留可复用宿主；失败调用 `cancel_menu`，即使准备超时也尝试取消。错误清理保留原始打开错误，不因菜单失败撤销桌面过滤。有属性等命令窗口仍打开的宿主继续保留，待窗口关闭后在所属 STA 清理。

真实桌面菜单诊断入口由 `desktop-menu-diagnostics` 显式启用，不属于默认菜单路径。计时的构建开关见[构建指南](build.md#诊断构建)，选择互斥见[选择与刷新时序](selection-latency.md)。

## Pane 编辑器

菜单返回重命名结果后，为当初选中的身份打开编辑器；F2 使用同一编辑入口。编辑器是 Pane 拥有的 `WS_POPUP | WS_EX_TOOLWINDOW` 原生 EDIT 窗口，独立合成、不显示任务栏按钮，以屏幕坐标覆盖图标标签并跟随 Pane 移动。编辑期间隐藏原标签并阻止自动收起。

编辑框以系统桌面字体参数为基础，并应用界面字体选择；标题编辑另外采用标题字号。尺寸随文本换行、网格缩放、滚动位置和 DPI 调整。隐藏扩展名的快捷方式只编辑显示名称并保留原扩展名；显示扩展名的文件预选扩展名前的部分。Enter 确认、Esc 取消，失焦确认；输入法组词期间不把 Enter 当作提交。

### 输入与编辑器生命周期

- 空白名称不会提交，编辑器保留并重新获得焦点；名称未改变时直接结束编辑，避免重复 Shell 操作。
- 文件名禁止空名称、NUL 和路径分隔符；其他文件系统规则、重名冲突及用户取消由 Shell 处理。
- Shell 取消返回未完成状态，编辑器保留供用户继续操作；提交错误显示错误并恢复编辑状态。
- 提交使用 `finishing` 状态防止重复进入。Shell 可能泵送消息并关闭所属面板，返回后先核对编辑器是否仍属于该 owner，再访问窗口状态。
- 销毁时清理字体、窗口关联和所属面板的编辑状态。取消编辑不触发文件操作。

面板／标签标题走独立的标题回调，去除首尾空白并将换行替换为空格；不适用文件扩展名规则，也不进入 Hook 身份事务。

## 提交与身份交接

实际改名通过 `IFileOperation` 执行，由 Shell 处理冲突和通知，并通过 `PostRenameItem` 获取新项目。托管桌面成员由 `hybrid/rename_transaction.rs` 提交：

1. 开始更新事务，暂停桌面绘制并保持成员过滤。
2. 执行 Shell 改名，取得真实的新身份，不拼接推测路径。
3. 更新模型、标签和快照，保留分组位置，并尝试保存工作区。
4. 交接 Hook 中的身份与恢复信息，以 `set_hidden` 同步发布完整过滤名单，再释放更新事务。

这里使用事务内的同步确认，与普通成员更新的 `submit_hidden` / `poll_hidden` 调度不同。IPC 可能处理窗口消息，保存模型后先释放应用借用，再调用这些原生入口。

Shell 改名成功后，即使保存或 IPC 失败，也保留已提交的新身份，继续尝试过滤修复与事务释放；后续同步重试待保存的工作区。不会把旧路径重新当成可改名目标。后台审计的修订键同时包含解析名，防止旧结果覆盖新身份。

Shell 已确认改名而新身份暂时读不到时，编辑器结束提交，应用保留原分组和位置并暂停显示旧图标。后台重试读取 Shell 返回的项目，也可用 Shell 回报的名称与原文件 ID 核验新路径。取得身份后先保存工作区，再由常规同步发布过滤名单。

非托管项目直接使用 Shell 改名入口。未收纳桌面图标继续使用 Explorer 自身的编辑器。

## 失败与补偿

| 失败位置 | 当前处理 |
| --- | --- |
| 菜单准备或显示失败 | 尝试 `cancel_menu`，保留原始打开错误，不撤销桌面过滤 |
| `begin_update` 失败或超时 | 仍尝试 `finish_update`，解除可能迟到的暂停，并结束控制端菜单状态 |
| Shell 改名错误或取消 | 通过事务守卫尝试结束更新；取消不产生新身份 |
| Shell 成功、工作区保存失败 | 保留真实新身份，标记待保存，后续同步重试 |
| 身份交接失败 | 仍发布完整名单，尝试修复 Hook 状态 |
| 名单发布失败 | 使发布缓存失效，保留待同步状态 |
| 事务结束失败 | 记录恢复待办，守卫在必要时再次尝试释放 |

Shell 成功之后，文件系统结果已经生效。这不是能整体回滚的数据库事务：后续失败应修复保存与过滤，不能再次对旧路径发起改名。补偿日志使用 `Rename committed; <阶段> recovery pending` 区分 identity、filter、release 和 save 阶段。

正常取消、窗口关闭和错误路径都应收束事务；强制终止主程序不能依赖 Rust 析构，Explorer 内的控制端存活监测承担恢复兜底，见[成员过滤](hybrid-desktop.md#菜单与恢复)。

## 验证边界

先准备[构建环境](build.md)，按改动选择检查：

```powershell
cargo test -p luciddesk --bin luciddesk hidden_extensions_are_preserved_and_visible_extensions_are_not_preselected --locked --offline -- --test-threads=1
cargo test -p luciddesk --bin luciddesk committed_rename_is_saved_before_failed_ipc_and_still_attempts_repair_and_release --locked --offline -- --test-threads=1
cargo test -p luciddesk-shell --lib shell_rename_keeps_the_same_file_identity --locked --offline -- --test-threads=1
```

原生编辑器测试在交互桌面单独运行：

```powershell
cargo test -p luciddesk --bin luciddesk inline_editor_tracks_label_and_escape_cleans_up_without_a_dialog --locked --offline -- --test-threads=1
```

测试过滤后确认目标确实执行。Shell 改名测试操作测试文件，原生编辑器测试涉及窗口及输入；不要与其他桌面交互测试并行运行。

手动验收包括：

- F2、鼠标菜单和键盘菜单进入同一文件编辑流程；多选菜单不错误选取新的改名目标。
- 隐藏扩展名快捷方式、显示扩展名文件、长名称及中文输入法的预选、确认和取消。
- Enter、Esc、失焦、空白输入、名称不变和重名冲突的不同结果。
- 编辑时移动面板、滚动、改变 DPI 或关闭面板，检查定位和资源释放。
- 托管成员改名后仍保持原分组位置；退出恢复及下一轮清单审计使用新身份。
- 菜单取消、属性窗口留存、经典菜单回退及第三方扩展执行后的清理。

菜单显示或命令返回成功不能代替实际文件结果检查。自动补偿测试也不能证明所有 Shell 扩展与系统版本兼容，平台限制见[验证指南](validation.md)。
