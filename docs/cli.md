# CLI 使用

`luciddesk-cli.exe` 查询并整理正在运行的 LucidDesk。先启动同一版本主程序；不自动启动应用，不创建或直接打开数据库。

## 帮助与命令发现

帮助可离线使用，不需要启动主程序：

```powershell
luciddesk-cli help
luciddesk-cli help pane
luciddesk-cli help pane snap
luciddesk-cli pane snap --help
luciddesk-cli folder fit -h
luciddesk-cli help pane snap --json
```

总览按资源组列出命令及用途，资源组列出子命令，具体命令显示在线/离线模式、读取/预览/执行行为、参数取值和必需字段。示例随命令变化，快捷修改示例默认带 `--dry-run`；示例中的 ID 与路径需要替换为实时查询结果。字段说明来自内置协议；复杂字段用 `--input` 提供。`help ... --json` 返回标准响应信封，`data` 含 `usage`、`commands`，修改命令还包含 `field_flags` 和 `operation_schema`，适合 Agent 读取。在线实际支持范围仍以 `capabilities --json` 为准。

CLI 与主程序继续使用同一发布版本，帮助标题显示 CLI 版本和独立的控制协议版本。`--version` 查看版本，不连接主程序。

面向 Agent，JSON 帮助保留原有字段并提供以下结构化信息，无需解析终端文本：

| 字段 | 用途 |
| --- | --- |
| `help_version`、`cli_version`、`protocol_version` | 帮助格式、CLI 发布版及控制协议版本 |
| `command_args`、`command_details`、`groups` | 叶命令参数、带说明的命令清单及总览资源组 |
| `requires_app`、`mutates_workspace`、`supports_dry_run` | 是否需要主程序、默认是否改变状态、是否支持只预览；资源组前两项为 null，应继续查询叶命令 |
| `default_effect` | `read`、`preview`、`apply`、`preview_then_apply` 或 `group` |
| `fields`、`required_fields` | 字段名、参数别名、取值 schema、必填项与是否只能通过 JSON 输入；创建命令的 `ref` 默认由 CLI 填充 |
| `example_args` | 可直接传给可执行文件的参数数组；与 `examples` 文本对应，仍需替换示例 ID 和路径 |
| `schema_args` | 获取完整协议的参数；`operation_schema` 与字段中的引用按该协议解析 |

字段只描述当前 CLI 的语法，不保证运行中的应用支持该操作。Agent 应先读取帮助，再查询 `capabilities` 与工作区；修改后核对回执和最终状态。

帮助只接受命令主题和可选的 `--json`，不要同时传入 `--input` 或修改参数。无参数、`--help` 和 `-h` 均显示总览。参数错误保留 `INVALID_REQUEST` 和原退出码，并提示帮助入口；普通文本模式下，连接失败、状态冲突和超时另附下一步操作建议。

## 构建与运行

```powershell
cargo build -p luciddesk -p luciddesk-cli -p luciddesk-explorer --locked --offline
.\target\debug\luciddesk.exe
.\target\debug\luciddesk-cli.exe status --json
.\target\debug\luciddesk-cli.exe capabilities --json
.\target\debug\luciddesk-cli.exe workspace get --json
.\target\debug\luciddesk-cli.exe pane list --json
.\target\debug\luciddesk-cli.exe pane get --id 1 --json
.\target\debug\luciddesk-cli.exe item list --pane 1 --json
.\target\debug\luciddesk-cli.exe item list --unassigned --json
.\target\debug\luciddesk-cli.exe schema --json
```

普通输出为格式化 JSON 数据，`--json` 输出单行 JSON 响应信封，适合脚本；错误码同时反映在退出码中。`schema` 是离线本地命令，直接输出协议信封 JSON Schema。`--help`、`--version` 也不需要主程序。

CLI 主要面向 Agent。调用时应始终显式传 `--json`，按 JSON 字段处理结果，不解析普通提示或错误消息文本。`help --json` 的 `output_contract` 描述输出约定；新增字段应按向前兼容原则处理。

需要后续操作时，CLI 在 `data.next_step` 提供指引（普通查询或已生效结果可能没有此字段）：

| `action` | 含义 |
| --- | --- |
| `review_then_apply` | 预览未执行；核对 diff 后，可使用 `args` 提交 |
| `query_receipt` | 提交结果不确定或仍等待生效；使用 `args` 查询原请求回执 |
| `refresh_and_preview` | 工作区冲突或计划过期；先按 `args` 读取工作区，再重新生成计划 |
| `inspect_state` | 回执未知、展示失败或已被覆盖；先检查当前工作区，不能直接重复修改 |
| `read_help` | 参数错误；按 `args` 读取结构化帮助 |
| `start_matching_app` / `check_access_and_cli_setting` | 需要启动匹配主程序，或检查访问权限和 CLI 开关 |
| `inspect_error` | 检查原始错误码及具体条件后决定下一步 |

`args` 是不含可执行文件的参数数组，可直接传给进程调用接口，无需拼接 shell 字符串；在线指引保留数据目录、协议版本和超时设置。没有 `args` 时需要人工或环境处理。`automatic_retry:false` 表示不能仅根据这条指引自动重试原修改；允许执行只读的回执查询。预览的提交参数含新的固定请求 ID，若提交不确定，应保留整组参数，不能再次生成 ID 重复提交。

`ok:true` 和退出码 0 只表示请求成功；仍需检查 `commit_status`、`presentation_status`，系统操作还需检查 `operation_status`。`request get` 的原回执在 `data.result` 内。`next_step` 只是 CLI 的辅助指引，不覆盖原始状态；已有 `data.recovery` 的精确查询与重试参数保持不变。

`--data-dir <现有目录>` 只核对主程序的数据目录，不切换或创建工作区。`--timeout-ms` 范围 1–60000，默认 10000；服务端查询等待 UI 最多 5 秒，整个连接 I/O 最多 10 秒。只有一个服务端连接同时执行，繁忙时客户端在自己的超时期限内等待。

## 响应与范围

响应包含 `protocol_version/request_id/ok/context/data/error`。退出码：0 成功，2 输入错误，3 应用未运行，4 面板不存在，5 版本冲突/锁定/过期计划/重复请求冲突/数据目录不匹配，6 响应过大/忙碌/桌面组件不可用，7 保存失败，8 超时或结果未知，9 权限拒绝，10 协议不兼容，1 其他错误。

`workspace get` 返回当前应用的面板、桌面项目、标签组快照，不强制重新扫描磁盘。面板明确区分 `manual_collapsed` 与 `effective_collapsed`；没有对应窗口时后者为 null。项目 ID 是当前应用实例内的不透明 token，不要解析或跨重启复用。项目清单不包含文件夹面板内文件或搜索结果。

当前支持计划式写入，`capabilities` 返回 `writes:true`、`plans:true`、`concurrency_tokens:true`。单项写入快捷命令已开放；发行包附带配套 Skill，`skill show --json` 可离线读取。

本地管道限制当前用户，核验对端用户与会话，并拒绝远程连接。独立 CLI 不链接桌面存储模块。查询队列只读访问 UI 状态，不调用保存、桌面刷新或备份维护；应用自己的既有后台任务仍可能独立运行。

当前通过 Cargo 构建运行，安装包与便携包的 CLI 分发属于后续交付。

## 整理计划：查询 → 预览 → 应用

`pane.create` 继承“设置 → 面板布局”中的新建普通面板排列默认值。已有面板不受默认值变化影响；需要固定行为的 Agent 应在计划中显式设置 `auto_compact`／`align_icons_to_grid`，覆盖创建时继承的默认值。

所有写入通过 `plan preview` / `plan apply` 执行。计划包含协议版本、查询返回的完整 `context` 和有序操作列表。支持：

| op | 参数 | 行为 |
| --- | --- | --- |
| `pane.create` | `ref`, `title` | 创建普通面板，继承外观，使用 GUI 默认尺寸及避让位置 |
| `pane.update` | `pane_id`；可选 `title`, `locked`, `auto_hide`, `collapsed`, `always_on_top` | 至少提供一个修改字段；窗口选项作用于整个标签组 |
| `pane.remove` | `pane_id`, 可选 `release_items` | 删除指定内容面板，非空时必须显式释放项目 |
| `item.release` | `item_ids` | 将项目归还桌面，不移动或删除真实文件 |
| `item.assign` | `item_ids`，以及二选一的 `pane_id` / `pane_ref` | 收纳项目；保留目标现有顺序，新项目按输入追加 |
| `item.reorder` | `pane_id`, `item_ids` | 提供该面板全部项目 ID 的完整顺序；空面板允许空列表 |

`pane_ref` 引用同一计划中此前创建的面板。ID 均为字符串；项目 ID 必须取自当前实例的查询结果。源/目标面板锁定时拒绝修改；使用 `pane.update` 显式设置 `locked:false` 后可继续修改。不移动或删除真实文件。标题最多 256 个字符，计划最多 256 个操作，每个项目列表最多 10000 项，输入及 IPC 消息上限 4 MiB。

PowerShell 示例（创建空面板）：

```powershell
$cli = '.\target\debug\luciddesk-cli.exe'
$snapshot = & $cli workspace get --json | ConvertFrom-Json
if (-not $snapshot.ok) { throw '查询失败' }
$plan = @{
    protocol_version = 1
    base = $snapshot.context
    operations = @(@{ op = 'pane.create'; ref = 'documents'; title = '文档' })
}
$path = Join-Path $PWD 'plan.json'
[IO.File]::WriteAllText($path, ($plan | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
$preview = & $cli plan preview --input $path --json | ConvertFrom-Json
if (-not $preview.ok) { throw ($preview.error | ConvertTo-Json) }
$preview.data.diff | ConvertTo-Json -Depth 20
$requestId = [guid]::NewGuid().ToString()
& $cli plan apply --token $preview.data.plan_token --request-id $requestId --json
& $cli request get --id $requestId --json
```

添加收纳操作时使用：

```json
{"op":"item.assign","pane_ref":"documents","item_ids":["从 workspace get 获取的项目 ID"]}
```

`--input -` 从标准输入读取 UTF-8 JSON；文件也必须为 UTF-8（不带 BOM）。未知字段和无效组合会被拒绝。

预览只在内存中执行，不保存数据库。返回完整前后标题/归属/排序快照、临时面板引用和 300 秒有效 token；最多保留 64 个计划，超过后淘汰最早的计划。预览中的新面板 ID 是临时结果，不构成预留。

应用会重新核对实例、工作区、清单与显示器状态。版本是保守的、不透明的进程内 token：当前各字段共用一个版本，任一相关变化均可能导致 `CONFLICT`，需重新查询和预览。临时自动收起不参与版本，也不触发保存。应用响应的 `context` 是本次校验的基础版本；下一次计划前重新 `workspace get`。

主程序在 UI 线程校验全部操作，以一次存储事务提交候选工作区，成功后替换内存状态并刷新界面。没有变化则不调用保存；失败预览和读取请求均不写库。项目收纳/排序发生变化时要求桌面组件已连接。数据库提交与桌面呈现分开：`commit_status:committed` 表示保存成功，`presentation_status:pending` 表示已请求刷新，但尚未确认 Explorer 完成呈现；呈现错误记录诊断日志，不重复提交数据库。

## 超时和重试

直接 `plan apply` 和快捷修改命令均返回 `data.recovery`，包括原 token、request ID、数据目录、协议版本，以及 `query_args` / `retry_args` 参数数组。省略 `--request-id` 时，自动生成的 ID 也会保留。非 JSON 错误输出会打印同一份恢复信息。先查询回执；需要重试时，将完整参数数组传给程序，不拼接成 shell 命令字符串，也不要遗漏原来的 `--data-dir`。

```powershell
$lookupArgs = [string[]]$result.data.recovery.query_args
& $cli @lookupArgs
# 确定需要原请求重试时：
$retryArgs = [string[]]$result.data.recovery.retry_args
& $cli @retryArgs
```

需要值的参数若遇到 `--...` 或 `-h` 会报告缺值，避免将 `--dry-run` 等选项当成标题。字面标题、搜索词等若采用这种形式，请通过 JSON `--input` 提供；负数坐标和 stdin 标记 `-` 仍可正常使用。

应用时建议显式提供唯一 `--request-id` 并保留 token。相同请求 ID 和相同内容重试，返回原结果，不重复写入；同 ID 不同内容返回 `REQUEST_ID_REUSED`。同一 token 使用另一请求 ID 再应用返回 `PLAN_ALREADY_APPLIED`。

完成结果只在当前主程序内存保留最多 10 分钟，回执总量最多 1024 条；未完成异步回执不会被淘汰，全部槽位占用时新提交在写入前返回 BUSY；不为请求回执增加数据库 I/O。超时先 `request get --id ...`，或在保留期限内使用完全相同的应用请求重试。`RESULT_UNKNOWN` 不代表未执行：主程序重启或结果被淘汰后，应检查工作区再决定下一步，不自动生成新 ID 重做。

## 设置查询

`luciddesk-cli settings get --json` 返回 `scope: "application_config"` 与 `values`。
`values` 使用稳定的命名字段（如 `diagnostics.level`、`search.enabled`、`panel_defaults.grid_scale`），保留字符串、布尔和数值类型。返回应用已加载的有效配置，包含缺省值；不读取外部尚未重新加载的编辑，不写入文件。
`values` 为 TOML 全局设置；`workspace_values` 为数据库设置，两组均支持 `settings.update`，同一计划不能混合两组字段。开机启动通过独立的 `startup.get` / `startup.set` 控制。

## 设置修改

操作格式：`{"op":"settings.update","values":{"diagnostics.level":"debug","panel_defaults.grid_scale":125}}`。
一个计划必须只包含这一个设置操作；多个字段放入同一个 `values`，不能混合面板/项目操作。设置保存使用 TOML 原子替换，工作区保存使用 SQLite 事务，两者不构成跨文件事务。预览会校验全部字段、类型和值，并返回修改前后的有效配置。同值提交不写配置文件。

沿用 `plan preview`、`plan apply`、`request get` 与相同 request ID 重试规则。设置回执 `scope` 为 `settings`。`commit_status:committed` 只确认保存；检查 `presentation_status:applied` 才能确认此次生效处理完成。若为 `failed`，读取 `presentation_error`，配置仍已保存，不应重新执行原提交；修复依赖后可使用新上下文提交同值设置重新尝试生效。快捷键冲突、缺少 Everything 或预览提供程序都会明确报告。界面重绘使用正常消息循环。

`settings get` 的 `runtime` 同时返回当前日志等级、图标缩放、圆角、搜索窗口可见性、预览实际启用状态与快捷键状态。窗口外观更新保留普通/文件夹面板实例及临时收起状态；改变图标缩放会重置网格滚动，和 GUI 操作一致。

## 显示器与面板几何

`monitor list --json` 返回显示器 ID、DPI、物理工作区和工作区 DIP 尺寸。`pane get` 返回持久布局 `geometry`，以及实际窗口像素边界 `window_bounds_px`（无窗口时为 null，标签成员返回共享活动窗口）。临时收起后的实际高度与保存的展开高度不同。

计划操作 `{"op":"pane.geometry","pane_id":"1","monitor_id":"查询返回的ID","x":40,"y":50,"width":480,"height":320}` 使用指定显示器**工作区相对 DIP**。完整矩形必须位于工作区内，最小尺寸与手动调整面板一致，由视图、图标文字和标签组决定（搜索面板仍为 260 × 160 DIP）；拒绝无效显示器、锁定面板及越界矩形。可与普通面板/项目操作组成同一计划。标签组共享窗口，所以几何作用于全部成员；普通、文件夹及搜索面板均支持，搜索保存展开尺寸，实际窗口高度由搜索内容管理。

预览 `diff.geometry` 显示像素取整后的有效几何。提交保存工作区与当前显示器布局到同一个 SQLite 事务；不改变其他显示器拓扑的记录。查询得到的上下文包含拓扑版本，显示器变化会使旧计划冲突。提交后检查 `pane get` 中的布局及实际窗口边界；原生移动失败会返回 `presentation_status:failed`，保存状态仍保留。

## 标签组

普通桌面面板支持以下计划操作，可与面板/项目操作批量提交：

| 操作 | 参数 | 语义 |
| --- | --- | --- |
| `tab.merge` | `pane_id`, `into_pane_id` | 将源整个标签组合并到目标组末尾，保留目标活动标签及共享窗口选项 |
| `tab.select` | `pane_id` | 激活该标签；锁定面板仍允许切换内容 |
| `tab.reorder` | `pane_id`, `pane_ids` | 指定组内完整顺序，必须每个成员恰好一次 |
| `tab.detach` | `pane_id` | 拆分为独立窗口，保留内容及展开位置，可随后用 `pane.geometry` 移动 |

合并、排序、拆分需要解锁。文件夹和搜索面板不支持标签，与 GUI 一致。预览包含修改前后 `tabs`；读取 `workspace get` 的 `tabs` 验证成员、顺序与活动标签。切换复用原生窗口和临时收起状态，合并/拆分保留内容模型缓存。界面处理结果写入幂等回执；`applied` 表示更新已成功处理，重绘和收起动画由正常消息循环完成。

## 文件夹面板

- `folder.create`：`ref`、`title`、绝对目录 `path`。目录必须存在；继承 GUI 的文件夹视图默认值。预览不启动文件夹工作线程，也不写默认设置。
- `folder.update`：`pane_id`，以及至少一个 `path`、`list_view`、`sort_column`、`descending`、`column_widths`、`visible_columns`。排序列和可见列使用 `name/modified/type/size`；名称列必须可见。列宽按 `[name, modified, type, size]` 提供四个正比例，总和为 1。字段省略保持原值，null 无效。
- `folder get --id ID --json`：返回根目录、当前浏览路径、保存偏好、窗口实际视图偏好、加载状态及当前文件快照。`loading:true` 表示异步加载尚未完成，文件列表可能仍是上一批结果。查询不强制扫描，也不写入数据库。

以上写操作可以与普通面板操作组成同一计划，工作区、排序和列设置在同一个 SQLite 事务保存。文件夹面板的标题和窗口选项使用 `pane.update`，位置大小使用 `pane.geometry`，移除映射使用 `pane.remove`；移除只关闭面板，不删除目录内文件。桌面 `item.assign/release/reorder` 不适用于文件夹内容。

## 临时导航与搜索

以下操作各自作为单独计划，仍经过 preview/apply 和幂等回执，但返回 `scope:runtime`、`commit_status:not_persisted`，不会保存路径、搜索词或结果：

| 操作 | 参数 | 行为 |
| --- | --- | --- |
| `folder.navigate` | `pane_id`, 绝对目录 `path` | 浏览目录，保留映射根目录 |
| `folder.back` | `pane_id` | 返回上一目录，没有历史时不操作 |
| `folder.home` | `pane_id` | 回到映射根目录并清空导航历史 |
| `search.query` | `pane_id`, `query` | 设置输入；空字符串清空结果，同值不重发查询 |
| `search.refresh` | `pane_id` | 刷新当前查询并推进查询代次 |
| `search.more` | `pane_id` | 加载下一页；忙碌时报告失败，没有更多时不操作 |

`search get --id ID --json` 返回 `query`、`generation`、`busy`、`failed`、`replacing`、`total`、`loaded_count`、`has_more` 和已加载 `entries`。执行成功只说明输入/分页请求已经处理，不能据此认定异步查询完成。以返回的代次查询，等待 `busy:false`，再检查 `failed`；加载替换期间旧结果仍可能存在。Everything 必须可用，关闭的搜索面板需先通过设置启用。查询最长 32767 UTF-16 单元，不接受 NUL。

应用的并发上下文包含当前文件夹导航历史与搜索代次/加载状态；GUI 的并发操作可能使旧计划冲突，此时重新查询和预览。重试必须沿用原 request ID，避免返回/刷新/分页重复执行。`runtime_result` 是操作处理后的即时状态，最新结果仍通过查询取得。


### 数据库设置

`workspace_values` 提供下列字段，使用 `settings.update.values` 提交。多个数据库字段在单个 SQLite 事务内保存；预览和同值更新不写入。禁止混合 TOML 字段，以免跨文件保存仅部分成功。

| 字段 | 类型与取值 |
| --- | --- |
| `font.family` | 字符串；空字符串跟随语言默认字体，否则须为支持当前语言的已安装字体 |
| `interface.title_emoji_color` | 布尔，标题彩色表情 |
| `interface.compact_menu` | 布尔，紧凑菜单 |
| `interface.header_divider` | 布尔，标题分隔线 |
| `folder_defaults.list_view` | 布尔，新文件夹面板的列表视图 |
| `folder_defaults.show_modified`、`folder_defaults.show_type`、`folder_defaults.show_size` | 独立布尔字段，名称列始终显示 |
| `folder_defaults.entry_mode` | `inline` 或 `explorer` |
| `backup.enabled` | 布尔 |
| `backup.interval_minutes` | 5、15、30、60 |
| `backup.keep` | 10、20、50 |

文件夹视图默认值只作用于以后新建的面板。字体和界面开关刷新现有窗口，`runtime.font_family` 及界面运行时字段可用于核对。备份策略由现有调度读取，修改策略不代表已经生成备份。


### 字体候选查询

`luciddesk-cli font list --json` 按需在后台枚举支持当前语言的字体。首次返回可能为 `busy:true`，此时不要使用空候选判断没有可用字体；适度轮询并设置超时，直到 `busy:false`，检查 `error` 后读取 `families`。返回 `default_family`、`effective_family`、语言样本文本 `sample` 和查询代次 `generation`。

结果缓存 60 秒，在后续查询发现缓存过期或语言样本改变时重新加载，旧任务会被取消；没有周期性扫描，不保存结果。系统中新安装的字体最长在缓存到期后的下一次查询可见。选择候选后用 `settings.update` 设置 `font.family`，仍须检查保存/生效回执：字体可能在查询之后被卸载。空字符串恢复随语言变化的默认字体。


### 开机启动

`startup get --json` 异步查询 Windows/MSIX 的开机启动状态，等待 `busy:false` 并检查 `error`。结果按需缓存 5 秒。`status` 是稳定值：`off`、`enabled`、`disabled_by_windows`、`unknown`、`other_location`、`disabled_by_user`、`disabled_by_policy`、`enabled_by_policy`。同时返回 `registered`、`effective_enabled` 和 `editable`，不要把已注册误认为系统允许启动。

修改示例：`{"op":"startup.set","enabled":true,"expected_status":"off"}`。必须作为唯一操作预览和提交，并使用刚查询到的 `status`；只允许 off/enabled/disabled_by_windows，系统限制和其他安装位置不能通过此接口绕过。写入前再次核对 OS 状态并串行化 GUI/CLI 操作，同值跳过写入。不读写工作区配置。

应用返回 `scope:system`、`operation_id`（原 request ID）及 `operation_status:pending`，这仅表示后台任务受理。使用 `request get --id ORIGINAL_ID` 有界轮询其 `data.result.data`，直到 completed/failed，再检查 `commit_status`（committed/unchanged/not_committed/unknown）、`startup_status` 和 `error`。Windows 拒绝期望状态时会报告 failed；unknown 表示无法确认最终保存结果，先重新查询系统状态再决定下一步。相同请求重试返回当前回执，不重复执行；异步回执允许从 pending 推进到最终状态。回执仍受进程寿命及 10 分钟保留期约束。主程序退出后应查询实际系统状态，不自动重新提交。


## 单项命令

单项修改命令默认在内存中获取当前上下文、预览，然后只提交一次；加 `--dry-run` 只返回预览 token。不会自动重试冲突或重新规划。预览后可使用原有 `plan apply` 提交。复杂、多操作整理继续使用显式计划。

| 命令 | 常用参数 |
| --- | --- |
| `pane create` | `--title TEXT`，结果 ID 在 `data.refs.created` |
| `pane update` | `--id ID`，任选 `--title TEXT`、`--locked true/false`、`--auto-hide true/false`、`--collapsed true/false`、`--always-on-top true/false` |
| `pane remove` | `--id ID`，非空面板需要 `--release-items` |
| `pane geometry` | `--id ID --monitor ID --x N --y N --width N --height N` |
| `folder create` | `--title TEXT --path ABSOLUTE_PATH` |
| `folder update` | `--id ID`，`--path PATH`、`--list-view true/false` 或 `--input FILE` 提供其他更新字段 |
| `folder navigate` | `--id ID --path ABSOLUTE_PATH` |
| `folder back/home` | `--id ID` |
| `search query` | `--id ID --query TEXT` |
| `search refresh/more` | `--id ID` |
| `tab merge` | `--id ID --into ID` |
| `tab select/detach` | `--id ID` |
| `tab reorder` | `--id ID --input FILE`，文件内容为 `{"pane_ids":["1","2"]}` |
| `item assign` | `--ids ID,ID --pane ID` |
| `item release` | `--ids ID,ID` |
| `item reorder` | `--pane ID --input FILE`，文件内容为完整 ID 数组 |
| `settings update` | `--input FILE`，文件内容直接为字段映射，例如 `{"diagnostics.level":"error"}` |
| `startup set` | `--enabled true/false --expected-status STATUS` |

所有单项命令支持 `--input FILE|-` 提供操作字段对象，不含 `op`（settings/reorder 的特殊形式见表）。命令行字段与输入对象中重复的字段、重复 JSON 键、未知字段均会拒绝。输入文件为 UTF-8、无 BOM、最大 4 MiB。ID 保持字符串，不使用标题替代。

Agent 可以先使用 `--dry-run --json` 查看差异，再按既有用户授权提交，无需额外人为确认。默认直接执行仅适用于用户已明确授权的操作。开机启动和搜索等异步操作仍需要核对最终状态。

单项命令在提交响应的 `data.recovery` 中提供原 `plan_token` 和 `request_id`；发生提交传输错误时该字段仍返回，因此此时错误信封的 data 不为 null。超时先查 `request get --id`，必要时使用原 token/request ID 调用 `plan apply`。不要通过重新运行单项命令来恢复超时：它会产生新预览；回执过期或应用重启后须核对实际状态。CLI 不写临时计划文件或回执日志。


## 获取配套 Skill

`luciddesk-cli skill show --json` 离线返回完整技能包：`data.files` 为相对路径到 UTF-8 内容的映射，`bundle_version:1`，入口为 `SKILL.md`。安装时保存全部文件并保留 `references/` 目录，拒绝越出技能目录的路径。兼容字段 `data.content` 和纯文本 `skill show` 仅返回入口。包内提供完整 `skills/luciddesk-control/`，可整体复制到 Agent 配置的技能目录，已有同名文件时请先比较内容。入口按任务链接 references，运行任务时只读取所需文件；不要把安装导出的全部内容每次都加载进上下文。CLI、Skill 和协议 schema 随普通 ZIP、便携包及安装包分发；不自动更改 PATH 或 Agent 设置。


### 桌面呈现确认与重启恢复

桌面收纳和释放通过 Explorer 异步完成。`commit_status:committed` 只确认持久状态；`presentation_status:pending` 表示还在等待图标资源或 Explorer 确认。使用原请求 ID 查询 `request get`，直到 `applied`、`failed` 或 `superseded`，不要重复提交。`status.data.desktop_sync_status` 同时提供当前同步状态 applied/pending/deferred/disconnected。菜单打开可能推迟同步；查询本身不触发写入。

若提交后 GUI 或其他操作改变了目标归属，旧回执会标记 `superseded`；断连则标记 `failed`，数据库提交不会回滚。核对当前工作区后再决定后续操作。项目查询数组不承诺视觉顺序；面板内顺序按 `placement.row`、`placement.column` 排序核对。

程序退出时 CLI 返回 APP_NOT_RUNNING。重启后实例 ID 改变，旧计划 token 返回 PLAN_EXPIRED、旧请求回执返回 RESULT_UNKNOWN；持久化面板仍可查询。收到这些错误应获取新快照并核对实际结果，而不是盲目重做。

开发验收命令：先构建 `cargo build -p luciddesk-shell --example desktop_snapshot --locked --offline --target-dir target/cli-plan-build`，再运行 `tools/validation/test-cli-plan.ps1 -LiveDesktopItems -RestartRecovery`。它要求没有其他主程序运行，使用独立数据目录，并只创建/整理/清理唯一命名的桌面测试文件；测试包含独立 Explorer 可见性及位置探针。

持续查询验证可使用 `tools/validation/test-cli-plan.ps1 -IdleSeconds 30`：先等待之前修改触发的自动备份完成，再比较整个测试数据目录的文件哈希、大小及写入时间，并记录 GUI CPU/内存。保持默认 error 日志，不禁用自动备份。正常自动备份是预期写入，不应误归因于只读查询。


启动 Debug 前确认同一输出目录包含 `luciddesk.exe`、`luciddesk-cli.exe` 和 `luciddesk_explorer.dll`。仅构建主程序和 CLI 不会生成独立桌面组件 DLL；缺失时桌面收纳不可用。

### 内容适配与右侧吸附排列

`pane get/list` 返回 `content_layout`：支持范围、项目数、图标列数、单元格 DIP、内容所需高度与物理吸附间距。Agent 无需读取源码计算布局。

- `pane fit --id ID --icon-columns 6 --dry-run --json`：按完整图标行适配，尽量保留原位置，必要时向工作区内移动。标签组按内容最多的成员适配。
- `pane arrange --input FILE|- --dry-run --json`：输入 `{"monitor_id":"查询所得 ID","columns":[["左上面板ID","左下面板ID"],["右上面板ID"]],"icon_columns":6}`。自动适配后靠右上排列；每列从上到下、列从左到右，保持 GUI 标准物理吸附间距。

两者支持常规 preview/apply、原子保存、版本冲突及回执。icon_columns 范围 1..64，面板列 1..16，总面板最多 256。支持普通桌面面板及已加载的文件夹面板；锁定或手动折叠需先处理，同一标签窗口不可重复选择。空间不足或排列会覆盖未选面板时拒绝，不会偷偷缩放图标、截断内容或覆盖其他窗口。排列不是永久绑定；后续拖动吸附由 panel_defaults.snap 设置控制。


### 相对面板吸附

```powershell
luciddesk-cli pane snap --id 7 --target 4 --side bottom --align start --dry-run --json
```

将 7 号面板放到 4 号面板下方并左对齐。side 支持 left/right/top/bottom；align 默认 start，也支持 center/end。左右吸附时对齐上/中/下，上下吸附时对齐左/中/右。间距固定沿用 GUI 吸附距离（当前 5 个物理像素），不接受单次 gap 参数。间距配置与吸附动作分离。

默认按当前可见窗口换算并保持源面板 DIP 尺寸，可加 --icon-columns N 在同一次事务内适配普通面板内容。目标面板不移动，可处于锁定状态；源面板须解锁，双方须实际展开（自动收起时也需先显示内容）。支持普通及文件夹面板，动态高度的搜索面板不能作为源或目标。同一标签窗口不可互相吸附，源标签组整体移动。超出目标显示器工作区或覆盖第三个面板会拒绝，不自动换边或截断。优先读取可见窗口的物理边界；批量计划中的后续吸附读取之前操作生成的位置。定位与手动拖动共用物理像素目标计算；预览后窗口边界或收起状态改变会返回 CONFLICT，需重新预览。吸附为一次位置调整，不创建联动移动关系。


### 文件夹内容适配与刷新

- `folder fit --id ID --icon-columns 4 --max-rows 5 --dry-run --json`：图标视图每行四个、最多五行可见；省略 max_rows 时适配完整内容。
- `folder fit --id ID --max-rows 8 --dry-run --json`：列表视图保持宽度，按表头及最多八行计算高度；列表视图不接受 icon_columns。
- `folder refresh --id ID --json`：请求重扫当前目录，属于单独的临时操作，不保存导航或偏好。查询 folder get 等待 available=true、loading=false、error=null，并核对 current_path。

文件夹 content_layout 暴露 ready、view、item_count、content_rows、required_height_dip 和当前路径。只使用已加载的快照，不在查询/预览阶段重新扫描目录。pane fit / pane arrange 也支持已加载的文件夹：图标视图使用列数，列表视图保留宽度。max_rows 是可见区域上限，所有文件仍可滚动访问。加载/错误/清单数量或导航变化使旧计划失效；新建或改映射后须等待目录加载再适配。超出工作区会拒绝，不自动隐藏条目。

### 普通面板图标排序

`pane sort --id ID --dry-run --json` 按显示名称自然升序预览，例如“文件2”在“文件10”之前；`--descending true` 为降序。对应操作为 `{"op":"pane.sort","pane_id":"ID","descending":false}`，方向默认为升序。同名项使用稳定身份消除歧义。

取消勾选普通面板菜单中的“自动排列图标”即可进入手动网格排列模式，`pane get/list` 与 `content_layout` 返回 `fixed_icon_positions`（为 true 表示自动排列图标已关闭）。此时 item.assign 填空位、item.release 留空位，pane.fit/snap/arrange 保留坐标和空行，并将实际宽度至少保留到最右侧已占用列；请求的 icon_columns 可能因此被提高。主动 pane.sort 和 item.reorder 会重新紧凑排列，不能用它们作为无副作用的刷新。取消“对齐到网格”后为自由排列，`align_icons_to_grid=false`；图标查询的 `placement.position_dip` 返回相对面板内容区的坐标（若为空则按 column/row 推算）。CLI 适配尺寸会包含自由位置的实际内容范围，不移动图标；自由内容的宽度不再向上取整到整列，请求的 icon_columns 仍作为宽度下限。

排序只改变指定普通面板内部顺序，不改变文件、面板位置或其他标签页，也不启用持续自动排序。名称排序不扫描文件元数据；其他字段在预览期间由后台线程读取，提交沿用预览时确定的顺序。锁定面板须先解锁；文件夹使用 folder.update 排序。复用 preview/apply、冲突保护及回执，顺序与布局均已满足时不写数据库。任意自定义顺序继续使用 `item reorder --pane ID --input FILE`，文件为完整当前成员 ID 数组；查询后按 placement.row/column 核对顺序。

## CLI 控制开关与 Skill 安装

“设置 → 常规 → Agent 与 CLI”提供“允许 CLI 控制”开关，默认开启，保存为 `config.toml` 的 `[cli].enabled`。关闭后立即拒绝所有在线查询、计划预览和提交，返回 `ACCESS_DENIED`（退出码 9）；管道保留以返回明确的禁用提示。已经提交的操作不会因此回滚。重新启用请使用设置界面；离线 `help`、`schema`、`skill show` 不受影响。

同一区域可复制 SKILL 安装提示词，包含安装目录中的 `skills/luciddesk-control/` 绝对路径。Agent 根据自身环境确定技能安装位置，直接复制整个目录（含 `SKILL.md` 和 `references/`），核对文件内容后报告路径与数量。不调用 CLI 导出 JSON，不通过文本重定向重新生成文件，从而避免编码和长输出截断问题。源目录缺失时应报告，不自动下载其他版本。复制按钮只复制提示词，不自动安装、不修改桌面。

普通面板查询还返回 `auto_compact`，表示是否开启“自动排列图标”（连续排列并消除空位）。

手动排列时，显式调整面板宽度允许小于图标内容宽度，图标位置保持不变；窗口通过横向滚动访问超出内容。`pane.fit` 仍按完整内容范围适配宽度；`pane.sort` 和 `item.reorder` 按当前可见列数重新排列，保持手动/自由模式。滚动位置不写入数据库。

### 排列模式与图标定位

`pane sort` 增加 `--sort-column name|modified|type|size`，默认仍为名称升序。所有字段都使用明确方向；日期最近优先请传 `--descending true`，重复调用不切换方向。元数据后台读取期间协议返回 `SORT_METADATA_PENDING`，CLI 在 `--timeout-ms` 范围内轮询同一预览。超时仍待处理时返回 `pending_preview` 和带 `stdin_json` 的下一步；此时没有提交，不应查询提交回执。文件属性缺失和同值比较使用与 GUI 相同的后备规则。

`pane update` 新增 `--auto-compact true|false` 和 `--align-icons-to-grid true|false`（仅普通面板）。开启自动排列同时开启网格对齐；关闭对齐同时关闭自动排列；重新开启对齐不强制紧凑。显式传入 `auto_compact=true` 与 `align_icons_to_grid=false` 会被拒绝。这两个字段只改变指定面板，标签组其他成员保持原模式。

```powershell
luciddesk-cli pane sort --id 1 --sort-column modified --descending true --dry-run --json
luciddesk-cli pane update --id 1 --auto-compact false --dry-run --json
luciddesk-cli item position --pane 1 --item-id ITEM_A --column 2 --row 3 --dry-run --json
luciddesk-cli pane update --id 1 --align-icons-to-grid false --dry-run --json
luciddesk-cli item position --pane 1 --item-id ITEM_A --x 180.5 --y 90 --dry-run --json
```

上述命令各自仅预览；先提交排列模式变更，再定位，或组合为同一计划。`item.position` 必须指定 `pane_id`、`item_id`，以及与当前模式一致的一组坐标：网格用 `column/row`（0..10000 的整数），自由排列用 `x/y`（内容区相对 DIP，0..1000000）。必须先关闭自动排列；图标须已属于该面板，跨面板先 `item.assign`。网格占用格返回错误，不隐式交换或挤动其他图标；自由排列允许重叠。网格交换可在同一计划中经一个空格中转。

读取 `capabilities.pane_sort_columns`、`pane_arrangement`、`item_position` 确认运行中应用支持扩展；协议版本保持 1，旧请求语义不变。预览、冲突保护、幂等回执及无变化不写库沿用统一计划逻辑。
