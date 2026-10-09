# 设置页公共组件

设置页使用 Rust / Direct2D 与现有合成管线，以导航、卡片和统一操作区组织设置。界面采用 WinUI 风格，但未引入 WinUI/XAML 运行时。首帧显示和合成资源生命周期见[绘图与绑定](rendering.md)，材质规则见[背景材质与主题](mica-materials.md)。

## 实现边界

| 文件 | 职责 |
| --- | --- |
| `app/src/pane/settings/mod.rs` | 窗口消息、Action 分派、焦点、编辑、滚动及场景刷新 |
| `app/src/pane/settings/layout.rs` | 导航与各页布局，将当前值绑定为组件和 Action |
| `app/src/pane/settings/components.rs` | Tokens、Palette、SettingsForm、文字测量、视口与裁剪 |
| `app/src/pane/settings/controls.rs` | 控件种类、Scene 控件构造、滑块几何与公共选项菜单 |
| `app/src/pane/settings/painter.rs` | 按 Scene 绘制文字、卡片、控件和状态 |
| `app/src/pane/settings/preview.rs` | 材质示意预览 |
| `app/src/pane/settings/tests.rs` | 布局、渲染、交互及原生编辑框回归测试 |

`Scene` 保存文本、卡片、控件、预览和视口几何；页面构造组件并绑定 `Action`，窗口事件处理执行操作，再按变化重建场景。公共组件不直接访问存储。新增设置应沿用这条路径，不把持久化放进绘制或命中代码。

## 布局与文本测量

“面板布局 → 图标排列”提供“对齐到网格”和“自动排列图标”。默认均开启；自动排列表示紧凑排列，不是按名称等属性排序。开启自动排列会开启对齐，关闭对齐会关闭自动排列，重新开启对齐则保持手动网格模式。`pane/layout_defaults.rs` 将三种合法模式保存在 `metadata.desktop_panel_layout`，只在菜单、标签页、首次启动或 CLI 创建普通面板时复制到面板；已有面板、文件夹和搜索面板不受影响。读取不写入，相同值保存不增加变更计数；重置面板布局会将新建默认模式恢复为紧凑排列。

布局坐标使用 DIP，由绘制和窗口输入路径处理 DPI 换算。

| 参数 | 当前规则 |
| --- | --- |
| 内容左边界 | `Tokens::content_x()` 根据导航标签实测宽度加 108 DIP，限制在 248–344 DIP |
| 内容宽度 | 窗口宽度减左边界及 24 DIP 右边距，最大 1000 DIP |
| 卡片 | 内边距 16 DIP、间距 8 DIP、圆角 8 DIP，普通卡片最小高度 72 DIP |
| 操作区 | 控件高 32 DIP，普通卡片尾列至少预留 232 DIP |
| 列表行 | 高 40 DIP |
| 快捷键 | 编辑区 232 DIP，间隔 8 DIP，恢复按钮 112 DIP |

卡片在强制堆叠、宽度小于操作列宽加 260 DIP，或说明超过 60 个字符时，将操作区放到文字下方。标题和说明高度使用文字测量结果；路径占据完整文字宽度，按钮独立排布，避免长路径挤占操作区。

文字测量使用当前字体、语言默认回退字体、字号和可用宽度。测量缓存键包含这些信息以及文本，达到 2048 项时清空，关闭设置时释放缓存容量。测量创建失败有估算路径，因此代码中的几何断言不能替代对长文本的实际视觉检查。

导航宽度随本地化变化，页面和命中代码应统一调用 Tokens，不能再次硬编码 248 DIP。新增文本、语言或字体时，同时检查换行、按钮宽度和窄窗口；资源维护见[多语言指南](localization.md)。

## 组件与页面组织

`SettingsForm` 提供分组、信息卡、开关、选项组、滑块、按钮、路径、快捷键、下拉选择、列表、分页、品牌卡及预览。公共 `Palette` 提供深浅主题颜色，导航选中和悬停颜色另外按材质、主题及强度计算。

主导航由 `layout::pages()` 定义，当前有主题与材质、面板布局、字体、文件夹面板、Everything 搜索、文件预览、备份与恢复、语言和关于九项。配色、备份记录和高级选项等通过页内操作进入。页面 ID 不连续，不应把导航数组下标直接当作页面 ID。

交互状态应保留以下语义：

- 禁用控件不能执行操作；选中状态反映当前值。
- 下拉菜单取消或返回未知命令时不产生新值。
- 备份执行期间禁止重复任务，但仍允许调整策略。
- 备份分页在首尾禁用不可用方向，翻页后回到内容顶部。
- 控件索引参与焦点、按下状态和开关动画；改变场景结构时同时检查索引状态的重置和对应关系。

## 滚动、裁剪与焦点

普通页面固定侧栏和窗口标题栏，内容按共享视口滚动。`Scene::scroll_to` 统一移动内容几何，`ContentClip` 控制绘制裁剪，`accepts_pointer` 限制命中范围。关于页图标和材质预览也参与内容滚动，不能单独留在未滚动的位置。

滚动量限制在实际内容范围内；不溢出时不显示滚动条。窗口事件处理支持滚轮、滚动条拖动、PageUp/PageDown，以及键盘导航时将焦点控件滚入可见区域。页面切换重置滚动位置；视口外的控件不能穿过标题栏响应点击。

### 字体页的独立列表

字体页通过 `Action::FontSearch` 标识独立列表布局，仅滚动字体候选行，当前字体、恢复操作和搜索区保持固定。字体选择使用搜索与列表滚动，不使用备份页的分页方式。

候选字体按 GDI 元数据筛选普通字重（400）、非斜体的可缩放字体，按当前界面语言校验代表字符覆盖，并按名称去重；不通过删除名称后缀合并不同字体家族。

搜索编辑使用原生编辑窗口，需要同步布局、DPI、焦点与页面生命周期。修改此路径时检查 Unicode 输入、清空查询、切换页面和关闭窗口后的资源释放，不能只验证静态 Scene。

## 材质预览与实际窗口

材质预览用带标题栏、内容层和图标的微型面板表示当前材质及强度，并复用相关配色参数。Mica 使用柔和中性底色，Mica Alt 的底色更深、壁纸色调更明显。

预览使用固定示例背景，是效果示意，不是 DWM 或用户桌面的实时截图。预览渲染通过不能证明实际窗口材质、系统回退或首帧显示正确，这些需要在真实桌面验证。

## 新增或修改设置

1. 在 `layout.rs` 选用现有组件，传入本地化标签、当前值和 Action；共用几何规则放入组件层。
2. 在窗口事件处理中处理 Action，沿用对应功能的校验、保存和错误显示路径。
3. 值或页面结构变化时检查场景刷新、滚动范围、焦点和动画状态；涉及原生编辑框时同步更新其位置与生命期。
4. 增加能验证行为的针对性检查，覆盖窄窗口、长文案、禁用状态和键盘操作；按需扩展渲染测试中的页面集合。

拖动预览与最终保存并非同一步骤，新增滑块应核对鼠标释放和键盘操作的提交路径。窗口或组件布局变化也应检查标题栏拖动、缩放边缘和内容命中是否仍隔离。

## 验证

先按[构建与验证](build.md)准备环境。设置测试包含 Windows 图形和原生窗口操作，应串行执行：

```powershell
cargo test -p luciddesk --bin luciddesk pane::settings --locked --offline -- --test-threads=1
```

重点测试入口：

| 测试 | 覆盖内容 |
| --- | --- |
| `settings_layout_and_rendering_at_multiple_scales` | 11 个页面、深浅主题、100%/150%/200% DPI、800×560 和 940×620 DIP、顶部与底部 |
| `all_languages_layout_and_render_without_control_overflow` | 当前 7 个语言索引、包含语言页的 12 个页面及多 DPI 布局渲染 |
| `settings_cards_align_and_long_paths_do_not_overlap_actions` | 卡片操作区、长路径及不同宽度 |
| `setting_cards_scroll_without_moving_navigation_or_hitting_caption` | 滚动、固定导航、可达性和标题栏命中隔离 |
| `pagination_disables_unavailable_directions_and_short_pages_do_not_scroll` | 分页边界和短页无多余滚动 |
| `font_list_scroll_keeps_search_and_current_font_fixed` | 字体候选滚动与固定搜索区 |
| `native_font_search_tracks_window_and_handles_clear_and_page_leave` | 原生搜索框的位置、清空和离页行为 |

测试中的页面及语言集合是显式枚举，新增页面或语言不会自动获得这些测试的完整覆盖。尺寸及命中检查另外覆盖 1440 DIP 宽度。

需要人工查看离屏快照时，在 PowerShell 中单独运行导出测试：

```powershell
$previousSnapshots = $env:LUCIDDESK_TEST_EXPORT_SNAPSHOTS
try {
    $env:LUCIDDESK_TEST_EXPORT_SNAPSHOTS = '1'
    cargo test -p luciddesk --bin luciddesk settings_layout_and_rendering_at_multiple_scales --locked --offline -- --test-threads=1
} finally {
    $env:LUCIDDESK_TEST_EXPORT_SNAPSHOTS = $previousSnapshots
}
```

导出文件位于仓库 `target/settings-*.bmp`，仅导出该测试的 100% DPI 图像；800 DIP 宽度和底部截图分别带 `-800`、`-bottom` 后缀。其他 DPI 仍参与测试，不由此命令生成对应快照。

离屏检查之外，还需实际打开设置验证 Tab 导航、滚轮、滑块、下拉取消、快捷键录入、字体搜索和窗口关闭。验收时区分测试断言、人工快照检查与实际桌面结果，详见[验证与兼容边界](validation.md)。
