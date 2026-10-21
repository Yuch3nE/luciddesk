# LucidDesk 官网

线上地址：[luciddesk.bbkingdom365.com](https://luciddesk.bbkingdom365.com/)。

Astro 7 + Tailwind CSS 4 + shadcn/ui + Lucide 的静态官网。React 通过 Astro islands 承载交互组件，无需服务端运行时。

## 开发与发布

以下命令均在 `website/` 目录执行：

- `npm ci`：按锁文件安装依赖（Node.js >= 22.22.3）。
- `npm run dev`：开发服务器，默认 http://127.0.0.1:8787。
- `npm run check`：Astro / TypeScript 类型检查。
- `npm run build`：生成 `dist/`。
- `npm run preview`：预览生产构建。
- `npm run deploy`：检查、构建，再部署 Cloudflare Workers。

首次发布需安装 Wrangler 并完成 `wrangler login`。Worker 为 `luciddesk-website`，域名为 `luciddesk.bbkingdom365.com`，部署目录为 `dist/`。安全响应头与 robots 来自 `public/`；sitemap 由 `src/pages/sitemap.xml.ts` 生成。

## 页面结构

- `src/pages/index.astro`：首页路由；`src/components/pages/Home.astro` 组合首页与功能、FAQ 数据。
- `src/layouts/PageLayout.astro`：SEO、主题脚本、页头与页尾。
- `src/components/widgets/`：Hero、Features、FAQs，以及 LucidDesk 的 Agent、Downloads、Header、Footer。
- `src/components/ui/`：shadcn/ui Button、Accordion、Select、Card、Badge 源码，以及 Astro 布局组件。
- `src/components/FAQAccordion.tsx`：FAQ 交互岛屿；`ThemeToggle.tsx` 使用 shadcn Button 图标按钮，默认跟随系统，点击直接切换明暗并保存偏好。按钮、卡片、标签和 Lucide 图标在构建时渲染。
- `components.json`：shadcn/ui 后续添加组件的配置。
- `scripts/finalize-build.mjs`：为 Astro 岛屿引导脚本生成 CSP 精确哈希；允许 Radix 测量高度所需的样式属性。
- `src/components/WorkspaceIllustration.astro`：内联 SVG 宣传图，随主题切换；其中所有图标均引用 lucide-react，不维护自绘图标路径。
- `src/styles.css`：站点配色、shadcn/ui 语义变量和动效。
- `public/theme.js`：浅色、深色和系统模式，沿用 `luciddesk-theme` 偏好存储并同步标签页。

页面使用真实产品说明和 SVG 功能示意，不以实际截图作为首屏。下载入口前往 GitHub Releases，按包名选择安装版或便携版。没有复制模板的演示博客、定价、客户背书和第三方统计。

## 组件来源

shadcn/ui 源码来自官方 `new-york-v4` 注册表，许可见 `SHADCN-LICENSE.md`。组件使用本项目配色；图标统一为 `lucide-react`。

## AstroWind 布局来源

来源：https://github.com/arthelokyo/astrowind

基于提交 `14e1a691f80548dcc36370847b1a02c0d0b12821` 的 Hero、Features、Headline、WidgetWrapper 及网格工具函数。保留 MIT 许可于 `ASTROWIND-LICENSE.md`。

适配：Hero 图片适配本项目的 SVG 插槽；移除未使用的模板类型和素材；使用本项目 SEO 和系统字体，不增加远程字体构建请求。部署仍是 Cloudflare Workers Static Assets，无需 SSR 适配器。

## 站内帮助

帮助页共用侧栏、章节目录和正文排版。`HelpPageLinks.tsx` 使用 shadcn Button 与 Lucide 图标显示文档导航；`HelpLayout.astro` 渲染章节，样式限定在帮助正文内，不影响更新日志。

`src/content/help.ts` 保存面向用户的安装、使用、Agent 和隐私内容，由 `[slug].astro` 与 `HelpLayout.astro` 渲染。更新应用时核对 `docs/usage.md`、`docs/installer.md`、`docs/portable.md`、`docs/cli.md` 和 `PRIVACY.md`，同步版本与政策日期。文档入口指向本站；发布包下载、源码及问题反馈仍使用 GitHub。

## 仓库信息

首页下载按钮下方显示当前版本号，点击可打开站内更新日志。导航和页脚展示 `src/data/repository.json` 中的仓库名、正式发布版本和 Star 数。每次构建通过 GitHub 公共 API 刷新；网络失败时保留上次快照并输出日期提示。页面不是实时计数器，悬停入口可查看数据日期。

## 中英文页面

中英文共用根路径；语言按钮通过 `public/language.js` 就地替换内容，保留主题、阅读位置与展开状态，并以 `luciddesk-language` 记住选择。构建时将英文路由转换为语言数据文件，旧 `/en/` 地址重定向至对应的公共地址；首页共用 `src/components/pages/Home.astro`。`src/content/translations.json` 保存英文界面文案，`copy.zh.json` 保存优化后的中文文案，`help.en.ts` 保存英文帮助文档。新增文案需同步两种语言。

语言入口为 `EN / 中` 按钮，不依赖浏览器自动翻译。两种语言共用 canonical 和 sitemap 地址，不生成独立语言的 hreflang。`src/pages/en/` 仅作为构建输入：`scripts/build-locales.mjs` 提取两种语言的内容至 `dist/_languages/`，移除英文 HTML 并生成旧链接重定向。

验证语言切换时先执行 `npm run build`，再运行 `npm run preview`。开发服务器不生成上述语言数据文件；Cloudflare 的 `_headers` 和 `_redirects` 规则需在部署环境验证。

## 更新日志

`/changelog/` 的中英文内容自动读取仓库根目录的 `CHANGELOG.md`、`CHANGELOG.en.md`。列表每页展示 8 个版本，使用 shadcn Accordion 就地展开完整日志，首页默认展开最新版本，其余收起；保留旧详情地址兼容已有链接；功能基线标明未单独发布，不伪造日期。分类名将 feat/fix/perf 等转为用户可读名称，保留原始更新条目。

每次版本更新同步两份 Markdown 后运行 `npm run deploy`，无需在官网重复录入。中英文版本集合不一致会阻止构建。新增版本详情、分页和 sitemap 会在构建时自动生成。官网页面调整不冒充应用版本更新。

更新类型标记由 `src/lib/change-types.ts` 统一归类，`ChangeBadge.tsx` 使用 shadcn Badge 和 Lucide 图标显示新增、优化、修复、性能、文档、发布、移除与维护。列表摘要、版本类型计数与详情逐条复用同一规则；明确的类型分组优先，主题分组按条目动作归类。
