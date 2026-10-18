# 原生桌面逐项过滤接口排查

2026-10-18。产品边界：Explorer 继续绘制、排列及处理未收纳项目；LucidDesk 管理分组。不得以隐藏整个原生视图并覆盖自有视图的方式实现逐项过滤。

## 撤回结果

撤回 `--results-desktop`、自有桌面宿主、结果集合组件及专用恢复进程、验收脚本。正式应用恢复直接使用 Explorer `FilterSession`。工作区数据不迁移、不删除。先停止替代实例，独立恢复进程恢复原生视图，快照确认 `icons_hidden=false`、原生项目数 79。

## 本机接口实测

运行现有 `target/debug/examples/desktop_filter_probe.exe --in-process`，不传移除或刷新参数。该模式只在 Explorer 桌面线程查询接口与计数，退出时卸载探针 Hook。报告位于 `target/desktop-filter-inprocess.log`。

| 查询 | 结果 |
| --- | --- |
| `IShellFolderView` | 成功 |
| `GetObjectCount` | 79，与 Shell 计数一致 |
| 视图 `IFolderFilterSite` | `E_NOINTERFACE (0x80004002)` |
| 浏览器 `IFolderFilterSite` | `E_NOINTERFACE (0x80004002)` |
| 桌面文件夹 `IResultsFolder` | `E_NOINTERFACE (0x80004002)` |

跨进程查询 `IShellFolderView` 返回 `E_NOINTERFACE`，进程内查询成功。因此外部查询失败不足以判定 Explorer 内部实现不存在，排查必须区分接口暴露和跨进程封送。上述结果仅描述当前机器，不能外推所有 Windows 构建。

## 公开接口的适用边界

- [`IFolderView2`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifolderview2) 的公开成员没有按项目隐藏或移除视图成员的方法。选择、位置和属性缓存不能等同于成员过滤。
- [`SetCurrentFolderFlags`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifolderview2-setcurrentfolderflags) 设置视图级选项，没有 PIDL 或项目名单参数。[`FWF_NOICONS`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-folderflags) 不是逐项过滤功能。
- [`IFolderFilter`](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifolderfilter) 依赖宿主暴露 `IFolderFilterSite`。本机进程内实测未通过此前提，不能直接安装过滤器。
- `IResultsFolder` 管理结果集合。本机原生桌面文件夹不提供它；创建新集合不会改变原生桌面的成员。
- 自有宿主的 `ICommDlgBrowser::IncludeObject` 等回调不意味着能替换现有 Explorer 桌面的宿主。不能以重新承载桌面作为本产品的迁移方案。

## 后续实施方向

当前保留原有 `IShellFolderView::RemoveObject/AddObject` 路径，明确其不受支持的兼容性风险；不能宣称已完成新接口迁移。已补齐阶段诊断、成员结果校验、存在性错误分类及恢复退避，具体行为和验证方式见[桌面同步说明](hybrid-desktop.md#接口失败与恢复)。后续仍需在目标 Windows 构建重复进程内能力探测。

若未来构建提供 `IFolderFilterSite`，先在隔离测试账户验证过滤器安装、ShouldShow 回调、刷新后成员集合、释放恢复、Explorer 重建及异常退出，再决定是否接入。查询成功本身不构成功能验证。

本轮尚未找到同时满足“不移动文件、不改变文件隐藏属性、不替换原生桌面、只隐藏指定成员”的受支持公开新接口。修改文件 Hidden 属性会影响其他文件视图且受显示隐藏文件设置影响；直接删 ListView 行或把图标移出屏幕也不是可靠的 Shell 成员过滤，不作为正式迁移方案。
