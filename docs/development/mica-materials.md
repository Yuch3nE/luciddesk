# 背景材质与主题

设置页提供纯色、亚克力、Mica 和 Mica Alt。材质由自定义 Windows Composition 视觉树实现，配合应用内容交换链；不依赖 XAML 窗口或原生 Mica 控制器。本文说明背景来源、参数与回退，内容绘制和首帧流程见[绘图与绑定](rendering.md)。

## 实现职责

以下路径相对 `app/src/pane/`。

| 模块 | 职责 |
| --- | --- |
| `acrylic/mod.rs` | 窗口视觉树、内容挂接、圆角与透明度，以及系统策略判断 |
| `acrylic/runtime.rs` | 线程内合成器、效果工厂缓存与调度队列生命周期 |
| `acrylic/material.rs` | 纯色画刷、效果画刷复用和有序回退 |
| `acrylic/effects.rs` | 亮度与染色混合、主题配方、强度调整和效果工厂 |
| `acrylic/transition.rs` | 背景画刷 200ms 交叉淡化、完成后释放旧背景 |
| `acrylic/host.rs` | HostBackdrop 能力探测、兼容入口及释放 |
| `acrylic/tests.rs` | 材质回退回归测试与视觉树断言 |
| `composition.rs` | 内容交换链、材质状态缓存、窗口透明度与圆角裁剪 |
| `theme.rs` | 面板边框、标签及卡片的局部配色 |
| `settings/preview.rs`、`settings/components.rs` | 材质示意预览、导航选中和悬停配色 |

材质类型与参数定义在 `crates/luciddesk-core/src/appearance.rs`。配方以源码为准，修改效果时同步预览和测试，避免另存一套容易漂移的颜色参数。

## 四种材质与参数

| 材质 | 背景来源 | 可调内容 |
| --- | --- | --- |
| 纯色 | `CompositionColorBrush` | RGB 与背景不透明度 |
| 亚克力 | 系统已模糊的 HostBackdrop | 效果强度 |
| Mica | 系统壁纸画刷，加亮度与染色混合 | 效果强度 |
| Mica Alt | 壁纸画刷与另一组主题配方 | 固定预设 |

Mica 类材质反映壁纸来源，不能当作对窗口后方应用的实时模糊。背景画刷和混合由系统合成，不通过应用采集桌面像素到 CPU。

亚克力和 Mica 的强度保存为 0–100，50 为默认配方，界面以相对默认值显示。强度调整的是亮度和染色层的 alpha，不等同于模糊半径、背景不透明度或整个窗口透明度。Mica Alt 不提供同类强度参数。

纯色保存 `#RRGGBB` 和 0–1 的背景不透明度，不创建背景采样画刷。切到纯色时释放已有材质画刷和 HostBackdrop 状态，复用或创建颜色画刷；图标和文字不会随背景不透明度一起淡化。窗口淡入则作用于内容与材质，是另一套透明度控制。

全局默认值与面板独立覆盖分别持久化；材质实际回退不改写所选值。设置窗口自身的材质效果与被编辑面板的参数分别处理，见[设置组件](settings-components.md)与[存储](storage.md)。

## 模拟系统材质的配方

Mica 与 Mica Alt 保留各自的壁纸亮度/染色配方；亚克力采用 WinUI 2.8 的 `AcrylicBackgroundFillColorDefaultBrush`，不再沿用旧的 `120/255` 浓度：

| 主题 | 亚克力染色 | 染色不透明度 | 亮度不透明度 | 回退颜色 |
| --- | --- | --- | --- | --- |
| 深色 | `#2C2C2C` | 0.15 | 0.96 | `#2C2C2C` |
| 浅色 | `#FCFCFC` | 0 | 0.85 | `#F9F9F9` |

参考 [WinUI 主题资源](https://github.com/microsoft/microsoft-ui-xaml/blob/v2.8.0/dev/Materials/Acrylic/AcrylicBrush_19h1_themeresources.xaml) 与 [亮度参数初始化](https://github.com/microsoft/microsoft-ui-xaml/blob/v2.8.0/dev/dll/XamlControlsResources.cpp)。颜色 alpha 在 Composition 边界量化为 8 位。

亚克力的效果链为 HostBackdrop → 亮度混合 → 染色混合，不额外模糊背景。为保持平滑观感，实际材质和设置预览均不叠加噪声纹理；各材质共用效果工厂，采用不同背景来源和配方。

同类材质的颜色参数更新复用画刷并进行短暂颜色动画。材质/纯色/回退画刷改变时仅对背景做 200ms CrossFade，内容层不参与。快速切换最多保留一组过渡，完成后换成目标画刷以释放旧背景；首次显示、关闭动画、关闭高级效果或诊断禁用背景时直接应用。保持桌面面板原有的非激活策略，不因失焦强制转纯色。

## 主题、局部控件与预览

面板标题与内容使用连续背景。标签、设置卡片和导航项绘制局部填充，避免额外的不透明整面覆盖遮住材质。导航分别处理选中、悬停及选中后悬停，深浅主题与材质变化时仍需保持文字和指示条对比度。

设置预览复用材质配方，但使用固定示例背景。它展示参数关系，不是用户桌面截图，也不能证明实际 DWM、壁纸采样或驱动上的最终效果。主题或强度变更后，应同时检查面板、菜单和设置窗口。

## 系统策略与回退

### 非纯色材质的选择顺序

1. 检查系统高级效果与节电状态。高级效果关闭或节电模式生效时，背景效果路径不可用，使用不透明主题底色。
2. 允许效果时尝试所选画刷。Mica 或 Mica Alt 的壁纸效果失败后，尝试亚克力；Mica 保留请求的强度，Mica Alt 使用默认强度。
3. 亚克力或效果创建仍失败时，清理效果画刷和 HostBackdrop 状态，尝试不透明颜色画刷：Mica 类深色 `#202020`、浅色 `#F3F3F3`；亚克力深色 `#2C2C2C`、浅色 `#F9F9F9`。

纯色在上述效果路径之前直接处理，保留用户设置的 RGB 和背景不透明度；不能把“关闭高级效果后所有背景都变成不透明”当作实现规则。颜色画刷或底层合成资源本身创建失败仍可能返回错误，回退不保证所有图形故障都可恢复。

`composition::Surface` 同时比较材质和效果策略状态，主题变化也会使材质状态失效。策略恢复后可重新应用所选效果，无需覆盖用户配置；参数未变时也不能忽略策略变化。

### HostBackdrop 与 Windows 版本

`HostBackdrop::enable` 优先设置公开的 `DWMWA_USE_HOSTBACKDROPBRUSH`。仅在返回 `E_INVALIDARG` 或 `E_NOTIMPL` 时，动态查找兼容入口 `SetWindowCompositionAttribute`；其他错误直接交给上层回退。兼容状态在守卫释放时撤销，避免隐藏或切换材质后留下窗口级效果。

Windows 11 是优先维护平台。Windows 10 设置页只显示纯色和亚克力；读取 Mica 类配置时按能力回退，不因此修改保存值。历史实机反馈不能替代目标 Windows 构建、驱动和系统策略组合的验收。

## 圆角与资源生命周期

内容和背景使用一致的圆角裁剪；Windows 10 另用缓存窗口区域处理外轮廓。尺寸、DPI 和圆角变化必须同步更新内容与材质，不能只调整其中一层。

图标字体优先选择可用且覆盖所需字符的字体；MDL2 回退按字形边界与文字中线对齐。这属于局部内容绘制，与背景来源分别验证。

窗口释放后，清理线程内合成运行时和图形设备缓存，再退出 OLE/COM。HostBackdrop、材质画刷和窗口目标应随对应对象释放，不能依靠进程退出时的 TLS 清理。顺序见[架构说明](architecture.md#退出与异常终止)。

## 验证

先准备[构建环境](build.md)，再按修改范围选择测试。以下命令分别检查配方及系统策略缓存：

```powershell
cargo test -p luciddesk --bin luciddesk material_palettes_and_blend_contract --locked --offline -- --test-threads=1
cargo test -p luciddesk --bin luciddesk system_policy_changes_invalidate_cached_material_and_restore_effects --locked --offline -- --test-threads=1
```

壁纸不可用回退的原生窗口测试单独运行：

```powershell
cargo test -p luciddesk --bin luciddesk missing_wallpaper_uses_acrylic_then_opaque_color_without_hiding_content --locked --offline -- --test-threads=1
```

运行前核对测试标记和交互环境，确认过滤命令实际执行了目标测试。自动检查通过后，按下表进行实机验收：

| 场景 | 检查内容 |
| --- | --- |
| 深浅主题与四种材质 | 背景来源、文字对比度、标签与导航选中状态 |
| 强度和不透明度端点 | 强度配方变化正确；纯色滑块不淡化内容 |
| 关闭高级效果、节电及恢复 | 非纯色回退可读，恢复后重建效果，保存值不变 |
| 调整尺寸、DPI、圆角 | 内容与背景裁剪一致，无边缘漏色 |
| 切换纯色与采样材质 | 旧效果正确释放，内容保持显示 |
| 设置与菜单首次显示、关闭 | 首帧、淡入和退出资源清理正常 |
| Windows 10 | 选项可见性、兼容入口及 Mica 配置回退 |

离屏截图和模拟策略测试不能代替真实系统设置、驱动及跨显示器验证。报告区分请求的材质、实际回退路径和所用环境，检查边界见[验证指南](validation.md)。
