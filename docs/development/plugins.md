# 插件开发指南

本文对应 `luciddesk-plugin-api 0.1.0`、C ABI **1**、JSON 协议 **1**。接口仍处于开发预览阶段；0.21.0 默认发行包不附带系统监控插件。较早的试验 DLL 缺少协议版本和能力清单，需要重新构建，不会被静默当作兼容插件使用。

## 1. 宿主和插件的职责

| 宿主 | 插件 |
| --- | --- |
| 发现 DLL、校验 ABI/清单、按 ID 路由、结束资源生命周期 | 导出入口、声明身份与能力、实现操作 |
| 面板 HWND、标题栏、DPI、材质、位置、缩放、折叠、层级 | 面板内容、图表、内容交互和后台采集 |
| 设置窗口、导航、标准控件、焦点与滚动 | 设置页的内容描述、选项和动作 |
| 当前语言、字体和持久化适配 | 功能内部状态、设备选择、业务逻辑 |

**能够加载不等于自动拥有 UI。** 通用注册表可以加载任何合法插件并按 ID 调用；目前正式接入 UI 的能力配置是 `system-monitor.v1`。新功能需要定义能力协议并增加对应宿主适配器。仅声明一个未知能力不会自动创建设置页或面板。当前接口没有通用的任意 HWND 注册、菜单注入或跨插件配置存储服务。

源码入口：

- [ABI、协议类型及公共调用辅助函数](../../crates/luciddesk-plugin-api/src/lib.rs)
- [C/C++ 头文件](../../crates/luciddesk-plugin-api/include/luciddesk_plugin.h)
- [主程序加载器](../../app/src/plugins.rs)
- [最小 Rust DLL 示例](../../crates/luciddesk-plugin-api/examples/minimal.rs)
- [监控插件实现](../../plugins/system-monitor/src/lib.rs)
- [监控能力专题](system-monitor.md)

## 2. 发现、身份和版本

首次需要插件时，宿主枚举 **EXE 同级 `plugins` 目录的顶层 `.dll` 文件**，按路径排序后加载。不递归、不搜索工作目录或 PATH。重命名 DLL 不影响身份，身份以清单 `id` 为准。依赖项按 DLL 所在目录和 Windows 系统目录解析。将需要导出插件入口的 DLL 放在此目录；辅助 DLL 也可能被枚举，缺少入口时会记录拒绝加载的诊断。

发现结果在当前进程内固定，添加、移除或更换文件后需要重启。缺少目录是正常状态；文件损坏、位数不符、入口缺失、ABI 不符和无效清单会记录诊断并跳过该文件，不阻止其他插件。相同 ID 的第一个有效文件保留，后续重复 ID 不注册、释放该次 DLL 加载引用并记录诊断。不要同时部署同一插件的多个版本。

宿主先调用导出入口，再调用 `manifest`：

```json
{"op":"manifest","language":"zh-CN"}
```

响应示例：

```json
{
  "id": "system-monitor",
  "name": "系统监控",
  "version": "0.1.0",
  "protocol_version": 1,
  "capabilities": ["system-monitor.v1"]
}
```

- `id`：1–64 个 ASCII 字符，以小写字母开头，只含小写字母、数字、点和连字符；用于路由，应保持稳定。
- `name`：非空显示名称；可参考请求中的语言返回，发现时的名称只是清单快照。
- `version`：非空插件版本字符串，推荐 SemVer；宿主不以它替代 ABI/协议校验。
- `protocol_version`：必须与宿主支持的 JSON 协议版本相同。
- `capabilities`：能力标识列表，空列表合法；每个标识非空。能力版本属于标识的一部分，例如 `example.echo.v1`。

`manifest` 和导出入口不得启动线程、创建窗口、读取用户配置或建立需要显式关闭的资源。资源应在后续能力操作中创建。

## 3. C ABI

插件与主程序都可以用 Rust 编写，但 DLL 边界不传递 Rust `String`、`Vec`、trait 对象、引用或枚举。通用接口只传递 C 布局字段、函数指针和借用的字节区间。

导出符号必须精确为 `luciddesk_plugin_v1`，使用 `extern "C"`：

```rust
#[unsafe(no_mangle)]
pub extern "C" fn luciddesk_plugin_v1(version: u32) -> *const PluginV1 {
    if version == ABI_VERSION { &API } else { std::ptr::null() }
}
```

`PluginV1` 包含 `abi_version: u32`、`struct_size: u32` 和可为空的 `invoke` 函数指针。发布的插件必须提供非空 `invoke`。表和函数地址必须一直有效。宿主只读固定表头，确认版本和长度后才读取函数指针；较大的兼容表可被接受，较短的表会被拒绝。

```text
invoke(context, request_ptr, request_len, reply, user) -> int32
reply(user, response_ptr, response_len)
```

ABI 不匹配时入口返回空指针。新增不兼容 ABI 时升级 ABI 常量与入口符号；不要改变 v1 已有字段含义或位置。

## 4. 调用、错误和内存

- 请求、响应均为 UTF-8 JSON，不附带 NUL 终止符，长度以字节计算，上限均为 **8 MiB**。
- 一次调用必须在返回之前同步调用 `reply` **恰好一次**，即使响应只是 `null`。回调不能在后台线程或调用返回后执行。
- 返回 `0` 表示成功，响应是操作定义的 JSON 值。
- 返回 `-1` 表示失败，响应固定为 `{"code":"...","message":"..."}`。其他返回码属于协议错误。
- 请求内存属于宿主，只能在调用期间读取。响应内存属于插件，宿主在回调期间复制；插件可在回调返回后释放它。谁分配谁释放。
- `user` 是宿主的不透明回调状态，插件只能原样传回，不得解引用或保留。
- 宿主在请求中写入当前 `language`；能力操作可增加 `font`、主题、尺寸或业务参数。请求必须是 JSON 对象，包含非空字符串 `op`；不符合时返回 `BAD_REQUEST`。

SDK 提供 `serve_json`，负责解析、响应和捕获 Rust unwind；`call_json` 在同步回调内直接将响应解析为宿主拥有的 JSON 值，不保留插件的字节缓冲或额外复制整份响应；同时验证回调次数、大小、返回码及 JSON。推荐插件复用这些函数。`serve_json` 产生 `BAD_REQUEST`、`PANIC`、`RESPONSE_TOO_LARGE`；宿主协议异常使用 `INVALID_RESPONSE`。业务错误应使用稳定的机器可读 `code`，例如 `UNSUPPORTED_OPERATION`、`NOT_FOUND`，`message` 用于诊断。

进程内 DLL 具有与宿主相同的权限，ABI 校验不构成沙箱。悬空指针、越界写入、原生访问违例或 `panic=abort` 无法由 `catch_unwind` 隔离。不要加载不可信插件；后台线程不得持有宿主指针。

## 5. 线程与退出

宿主在 UI 线程同步调用插件。插件不要在调用中泵送消息或反向重入宿主；耗时采样放在插件自己的后台线程，用短操作读取快照。宿主提供的 COM 绘制接口受调用线程约束，不能移到后台。

`shutdown` 是通用必需操作，成功返回 `null`，应可重复调用并在没有创建资源时成功。它必须停止并等待工作线程、释放绘图及其他线程相关资源，不能仅发出退出信号就返回。

正常退出时宿主先关闭面板，再在 COM/图形环境结束之前发送 `shutdown`。已注册的 DLL 会固定到进程结束，不支持热卸载。进程崩溃或强制结束不保证收到 `shutdown`，必要配置应通过正常设置操作及时保存，不能依赖退出回调。

## 6. 设置页描述

SDK 的 `Page` 返回 `description` 和 `items`。控件标签与排列由 DLL 决定，宿主沿用原生设置布局。`Item` 当前采用 Serde 外部标签格式，大小写是协议的一部分：

| JSON 形态 | 含义 |
| --- | --- |
| `{"Section":"标题"}` | 小节标题 |
| `{"Toggle":{"title":"...","description":"...","selected":true,"action":{...}}}` | 开关 |
| `{"Toggles":{"title":"...","values":[["标签",{...},true]]}}` | 紧凑开关组 |
| `{"Choices":{"title":"...","values":[["标签",{...},true]]}}` | 选项组 |
| `{"Button":{"title":"...","label":"按钮","action":{...}}}` | 操作按钮 |

`action` 是能力协议定义的 JSON，不是任意命令执行接口。通用 SDK 不解释它；宿主适配器验证后执行支持的动作。不要借用另一个能力的动作名称假定它能运行，也不要让设置描述包含宿主不支持的控件。标签应适配宿主窗口宽度及多语言环境。

## 7. `system-monitor.v1` 能力参考

宿主用 ID `system-monitor` 和能力 `system-monitor.v1` 共同选择适配器。

| 操作 | 请求的主要字段 | 成功响应 |
| --- | --- | --- |
| `settings` | `panels: [[panel_id,title,options], ...]` | `Page` |
| `devices` | 无 | `[network_devices,disk_devices,gpu_devices]`，每项为 `[id,name]` |
| `inventory` | `devices: [network_devices,disk_devices,gpu_devices]` | `null`；更新主进程设置页的设备清单，不启动采样 |
| `client_new` | 无 | 不透明 `u64` 客户端 ID |
| `tick` | `id, visible, options` | 是否需要重绘的布尔值 |
| `client_drop` | `id` | `null` |
| `drawing_new` | `font` | 不透明 `u64` 绘图 ID |
| `paint` | `id, options, font, dark, ink, bounds` | `null` |
| `drawing_drop` | `id` | `null` |
| `shutdown` | 无 | `null` |

`client_new`、`drawing_new` 及后续采样/绘图调用只在监控子进程执行。`client` 和 `drawing` 的 ID 仅在创建它们的进程中有效，不能跨进程使用、当作持久面板 ID 或保存到数据库。`bounds` 为 `[x,y,width,height]`，单位 DIP，内容区由宿主给定。`paint` 的 `context` 是借用的 `ID2D1DeviceContext*`；其他调用传空。宿主拥有 BeginDraw/EndDraw、目标、DPI 和窗口合成，插件只绘制内容，不调用 EndDraw、不保留目标，并配对恢复自己的 clip 状态。

`options` 当前为：

```json
{"enabled":true,"metrics":["Cpu","Memory","Gpu","Network","Disk"],"compact":false,"paused":false,"interval":1,"network":null,"disk":null,"gpu":null}
```

`interval` 支持 1、2、5 秒。设备字段为设备 ID 或 `null`；`metrics` 可以为空。持久化、启停面板与显示器位置由宿主适配器执行。设置动作包括 `{"op":"new"}`、`{"op":"save","id":1,"options":{...}}` 和 `{"op":"task_manager"}`；它们是该适配器的协议，不是通用插件操作。卸下 DLL 后已有监控配置保留，但不创建窗口，也不显示设置入口。

## 8. 编写和构建第一个插件

最小示例是可直接编译的源码，不依赖监控插件或主程序私有模块：

```powershell
cargo build -p luciddesk-plugin-api --example minimal --locked --target-dir target/plugin-dev
```

产物为 `target/plugin-dev/debug/examples/minimal.dll`。复制到运行中的 EXE 所在目录的 `plugins` 文件夹，重启主程序。示例声明 `example.echo.v1`，仅演示发现、路由及错误返回，**不会显示 UI**。宿主开发者可调用：

```rust
plugins::request("example.echo", serde_json::json!({"op":"echo","value":"hello"}))?;
```

独立仓库应建立 `crate-type = ["cdylib"]` 的 crate，使用与目标主程序一致的 Windows 架构，依赖 SDK 并参照示例导出入口。ABI 兼容不等于能力协议兼容：增加 UI 功能时先定义能力版本、数据和动作，再实现宿主适配器、持久化及真实 UI 测试。

本仓库的默认发行与可选监控构建：

```powershell
./tools/package.ps1 -Installer
./tools/package.ps1 -Plugins system-monitor
```

第二条命令显式生成监控 DLL 并放入包内 `plugins`，清单记录其 SHA-256。插件源码统一放在 `plugins/<name>`，通用 SDK 留在 `crates`。新增插件添加 workspace member 和 `[package.metadata.luciddesk] plugin = "插件ID"` 后，可通过 `-Plugins` 选择；构建、打包和安装清单自动使用其 cdylib 产物。旧参数 `-SystemMonitorPlugin` 仍兼容。不要把示例 DLL 放进正式发行包。

## 9. 验证与排障

```powershell
./tools/validation/test-plugins.ps1 -Offline
```

真实宿主测试从**测试 EXE**旁的 `plugins` 目录加载 DLL，通常是目标目录的 `debug/deps/plugins`，不是仓库根目录。统一验证脚本会编译、部署测试 DLL，依次验证有插件和无插件，并恢复 DLL 及环境变量。手动开发可运行 `./tools/build-plugins.ps1`，默认部署到 `target/debug/plugins`。可设 `LUCIDDESK_TEST_MONITOR_PLUGIN=1` 断言监控插件实际加载，或设为 `0` 验证缺失/无效插件；设 `LUCIDDESK_TEST_EXAMPLE_PLUGIN=1` 运行独立示例发现与路由断言。

最小示例的宿主集成测试可按以下步骤运行（在仓库根目录）：

```powershell
cargo build -p luciddesk-plugin-api --example minimal --locked --target-dir target/plugin-dev
New-Item -ItemType Directory -Path target/plugin-dev/debug/deps/plugins -Force | Out-Null
Copy-Item target/plugin-dev/debug/examples/minimal.dll target/plugin-dev/debug/deps/plugins/echo-example.dll -Force
$env:LUCIDDESK_TEST_EXAMPLE_PLUGIN = '1'
try {
    cargo test -p luciddesk --bin luciddesk plugins::tests:: --locked --target-dir target/plugin-dev -- --test-threads=1
} finally {
    Remove-Item Env:LUCIDDESK_TEST_EXAMPLE_PLUGIN
}
```

至少验证：有效入口；空/短表/版本不符；错误或重复清单；零次/多次回调；非法 JSON；结构化业务错误；有/无 DLL 的启动；重复 shutdown；真实面板绘图与设置动作；所有资源在退出前释放。接口测试不等同于各 Windows 构建、第三方插件或异常原生代码的兼容性保证。

出现问题先查看 `plugins` 诊断类别，记录 DLL 路径、插件 ID、版本、ABI/协议版本和完整错误。入口缺失通常是导出符号或文件错误；无法加载通常涉及位数或依赖；清单缺少字段则需要按当前 SDK 重新构建。不要通过忽略版本或吞掉错误把不兼容插件强行加载。

CI 正式发行使用 `tools/package.ps1 -All -InstallerFormat Exe -Offline -NoPlugins`。该参数禁止同时选择插件，并在生成 ZIP 和安装包前检查暂存清单与 DLL；插件测试产物不进入正式发行包。本地仍可显式使用 `-Plugins system-monitor` 构建开发预览包。

## 系统监控的进程隔离

系统监控是进程隔离插件的现有实例：主进程仅调用清单、设置描述和设备清单同步；实际窗口及 `client_new` / `drawing_new` 都运行于独立的 `luciddesk.exe --monitor-child`。原有 C ABI 不变，DLL 在子进程中仍通过标准宿主绘图上下文工作。`inventory` 操作接受三个设备列表（网络、磁盘、GPU），供主进程设置页使用，不触发本地采样。

该子进程入口与按行 JSON 管道属于同一构建的私有协议，并非通用的第三方插件 ABI。插件文件继续位于 EXE 同级 `plugins`，默认发行包不带插件，也不需要额外分发 helper EXE。
