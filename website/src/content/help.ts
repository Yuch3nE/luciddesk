import { manualZh } from './manual';
export interface HelpSection { id: string; title: string; html: string; }
export interface HelpPage { slug: string; title: string; description: string; label: string; sections: HelpSection[]; }
export const helpPages: HelpPage[] = [
  {
    slug: 'installation', title: '安装好，就开始整理', label: '安装与更新',
    description: '选择适合自己的版本，了解安装、更新和迁移的正确步骤。',
    sections: [
      {id:'choose', title:'选择安装方式', html:`<p>日常使用推荐 EXE 安装版；希望程序和设置放在一起，可以选择便携版。官网提供 Windows 10 / 11 x64 下载，主要验证与维护平台为 Windows 11 x64。</p><ul><li><strong>安装版：</strong>运行以 <code>setup.exe</code> 结尾的安装包，按向导选择安装位置。带有开始菜单和卸载入口，更新时保留配置。为所有用户安装需要管理员权限。</li><li><strong>便携版：</strong>选择以 <code>portable.zip</code> 结尾的压缩包，完整解压到有写入权限的文件夹。默认将设置保存在程序旁的 <code>data</code> 文件夹。</li><li><strong>普通 ZIP：</strong>免安装，但设置默认保存在本机用户数据目录，与便携版不同。</li></ul><p>下载入口使用 GitHub 托管发布包；安装和使用说明都可以在本站阅读。</p>`},
      {id:'start', title:'首次启动', html:`<ol><li>安装版按向导完成安装后启动；ZIP 版本先完整解压，不要直接从压缩包内运行。</li><li>保留同一版本的 <code>luciddesk.exe</code> 与 <code>luciddesk_explorer.dll</code>。便携版还需保留无扩展名的 <code>portable</code> 标记文件。</li><li>如果已经运行过 LucidDesk，先从托盘退出旧实例，再启动新版本。</li><li>在任务栏通知区域找到 LucidDesk，选择“新建面板”，将桌面图标拖入即可开始整理。</li></ol><p>关闭设置窗口不会退出程序。需要退出时，请使用托盘菜单中的“退出 LucidDesk”。</p><p>下一步：<a href="/guide/">了解面板、搜索和预览的用法</a>。</p>`},
      {id:'updates', title:'更新时保留你的布局', html:`<p>在“设置 → 关于”查看版本或手动检查更新。自动检查默认开启，启动时及之后每 24 小时检查一次；可在这里关闭。程序不会自动下载安装包或重启。</p><ol><li>更新前在“设置 → 备份与恢复”导出一份备份。</li><li>从托盘退出 LucidDesk，等待程序和桌面组件释放。</li><li>安装版运行新版安装包，沿用原安装方式与目录。</li><li>便携版用新版便携 ZIP 替换程序文件，保留原 <code>data</code> 文件夹和 <code>portable</code> 标记。主程序与 DLL 必须来自同一版本。</li><li>启动后检查版本号、面板和当前数据目录。</li></ol><p>EXE 与 MSI 不能相互覆盖升级。需要切换格式时，先卸载原版本并保留配置，再安装另一种格式。</p>`},
      {id:'move', title:'换电脑或移动便携目录', html:`<p>先退出程序，再复制整个便携目录。若开启了登录自启，请先关闭，移动后再从新位置开启。可通过托盘“打开配置目录”确认数据实际存在哪里；设置过自定义数据目录时，数据可能不在程序旁。</p><p>跨安装方式迁移时，在原实例导出备份，退出后启动目标版本，再从设置中恢复。恢复会替换对应设置和布局，并不会合并两套工作区。</p><p><strong>备份与便携目录不包含面板引用的原始文件。</strong>换电脑、盘符或目录后，需要另行迁移文件，并检查文件夹映射以及 Everything、预览工具的路径。</p>`},
      {id:'uninstall', title:'卸载与清理', html:`<p>安装版可从 Windows“设置 → 应用”卸载。EXE 卸载默认保留配置；取消“保留用户配置”可删除当前账户的默认数据目录。MSI 卸载保留用户配置。</p><p>便携版删除前先关闭登录自启并退出程序。删除包含 <code>data</code> 的整个目录，也会删除其中的设置与布局，操作前请备份。更多数据保留范围见<a href="/privacy/#delete">隐私说明</a>。</p>`},
    ],
  },
  manualZh,
  {
    slug:'agent', title:'说出想法，让 Agent 协助整理', label:'Agent 联动',
    description:'连接你使用的外部 Agent，先查看桌面、预览改动，再应用整理方案。',
    sections:[
      {id:'overview', title:'可以帮你做什么', html:`<p>Agent 可以通过配套 CLI 了解当前工作区，帮助调整面板位置、大小、文件归属和排列。你可以描述“工作文件放左边，常用工具放右边”，再检查它给出的具体方案。</p><p>这项功能需要外部 Agent。LucidDesk 本身不包含 AI 聊天服务，也不会仅因为开启 CLI 就自动整理桌面。</p>`},
      {id:'connect', title:'完成一次连接', html:`<ol><li>安装或更新 LucidDesk，启动主程序。使用同一发布版本附带的 CLI 和 Skill。</li><li>打开设置中的“Agent 与 CLI”，确认“允许 CLI 控制”已开启。</li><li>点击复制 Skill 安装提示词，将其粘贴给你使用的 Agent。提示词包含本机配套技能目录的位置。</li><li>让 Agent 按提示安装完整的 <code>luciddesk-control</code> 技能目录，再确认连接状态。</li></ol><p>复制按钮只复制提示词，不会自动安装技能或修改桌面。安装包也不会替你修改外部 Agent 的配置。</p>`},
      {id:'request', title:'这样描述需求更清楚', html:`<blockquote>先查看我的桌面布局，把工作文件整理到左侧面板，常用工具放到右侧。不要删除或移动原始文件，先给我预览方案，等我确认后再应用。</blockquote><p>告诉 Agent 哪些面板需要保留、哪些位置不能动，以及是否允许创建或合并面板。预览时检查受影响的面板、项目和位置，确认后再应用，完成后核对实际桌面。</p>`},
      {id:'control', title:'始终知道改了什么', html:`<ul><li><strong>先读取：</strong>根据当前工作区生成方案，不依赖过时的面板编号。</li><li><strong>先预览：</strong>预览不等于已经执行。检查改动后再确认应用。</li><li><strong>有冲突时重看：</strong>如果你手动调整了桌面，旧方案可能失效，应重新读取并预览。</li><li><strong>结果不明确时先核对：</strong>遇到超时或连接中断，先查状态和回执，不要直接重复执行。</li></ul><p>关闭“允许 CLI 控制”会拒绝新的在线查询、预览和提交，但不会撤销已经完成的改动。要恢复连接，请在应用设置中重新开启。</p>`},
      {id:'verify', title:'需要手动确认连接？', html:`<p>在安装目录打开 PowerShell，以下命令只读取状态，不修改工作区：</p><pre><code>.\\luciddesk-cli.exe status --json\n.\\luciddesk-cli.exe workspace get --json</code></pre><p>更多命令可以直接在本机查看：</p><pre><code>.\\luciddesk-cli.exe help\n.\\luciddesk-cli.exe help pane</code></pre><p>CLI 不会自动启动主程序。无法连接时，先检查应用是否已启动、两者版本是否匹配，以及 CLI 控制开关是否开启。升级或移动程序后，也要让 Agent 使用新目录中的 CLI 和 Skill。</p>`},
      {id:'privacy', title:'外部 Agent 与你的信息', html:`<p>Agent 读取工作区时可能接触到面板名称、文件名和路径。信息是否离开本机，取决于你选择的 Agent 及其设置。请在使用前了解它的数据处理方式。</p><p>LucidDesk 的本地数据、更新检查和第三方工具说明见<a href="/privacy/">隐私说明</a>。</p>`},
    ],
  },
];

helpPages.push({
  slug:'privacy', title:'你的桌面数据，如何被使用', label:'隐私说明',
  description:'根据应用隐私政策（2026 年 10 月 20 日）整理，说明本地数据、网络访问和删除方式。维护者：Yuchen95。',
  sections:[
    {id:'scope', title:'适用范围', html:`<p>本说明适用于 LucidDesk 官方安装版、普通 ZIP 和便携版。第三方修改版可能采用不同的数据处理方式。应用无需注册账号，目前没有内置广告、使用统计或自动上传崩溃报告的功能。</p><p>桌面整理、文件夹浏览和布局保存主要在本机完成。本站仅将应用政策改写为便于阅读的说明，不改变应用的数据处理方式。</p>`},
    {id:'local', title:'本机会保存哪些信息', html:`<ul><li><strong>设置：</strong>界面语言、外观、快捷键和第三方工具路径，用于记住你的偏好。</li><li><strong>工作区：</strong>面板名称、位置、布局、文件夹路径和桌面项目引用，用于恢复桌面。</li><li><strong>文件信息：</strong>文件名、属性、图标及预览所需内容，用于浏览、搜索和预览。</li><li><strong>诊断信息：</strong>Windows、应用和桌面连接状态，用于排查故障，由你决定是否分享。</li><li><strong>备份：</strong>设置与布局快照，不包含引用的原始文件。</li></ul><p>默认保存在 <code>%LOCALAPPDATA%\\LucidDesk</code>；便携版默认保存在程序旁的 <code>data</code>。数据目录环境变量可以指定其他位置，托盘“打开配置目录”可确认实际路径。这些文件可能包含个人路径，不会自动上传给维护者。</p><p>桌面收纳保存引用，不会为整理而移动原文件。你主动执行删除、重命名、剪切、粘贴等文件命令时，会影响真实文件。</p>`},
    {id:'startup', title:'登录自启会做什么', html:`<p>普通安装版和便携版开启登录自启时，会在当前用户的 Windows 注册表 Run 项中记录程序路径和启动参数。关闭时删除属于当前程序的登记。应用读取系统的启动禁用状态，不修改系统禁用记录；这些信息在本机处理，不上传。</p><p>卸载器清理属于当前安装的当前用户自启登记。便携版移动或删除目录前，请先关闭自启。MSIX 版使用 Windows StartupTask，由系统管理包的自启与卸载清理，不写入普通版 Run 登记。</p>`},
    {id:'network', title:'什么时候会联网', html:`<p><strong>检查更新：</strong>默认在启动时及之后每 24 小时通过 HTTPS 访问 GitHub Releases API，也可手动触发。可以在“设置 → 关于”关闭自动检查。</p><p>请求包含应用名称与版本的 User-Agent。GitHub 及网络服务可能处理 IP 地址、请求时间和请求信息；应用不会在这个请求中发送工作区、配置、搜索词或个人文件内容。检查结果在本机比较，应用不会自动下载安装包。</p><p><strong>打开外部链接：</strong>下载发布包或打开其他网站时，将使用浏览器访问对应服务，受该网站及浏览器的隐私设置约束。GitHub 的数据处理见其<a href="https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement">隐私声明（外部网站）</a>。</p>`},
    {id:'tools', title:'搜索、预览和外部服务', html:`<p>启用 Everything 或 Peek、QuickLook 时，会向所选本机工具传递搜索条件或文件路径，由它们按各自设置处理。Windows Shell、第三方菜单扩展、网络目录和云同步文件也可能交由系统或相关服务处理。</p><p>使用外部 Agent 时，应另行了解其数据处理设置；参见<a href="/agent/#privacy">Agent 与信息访问</a>。通过 Microsoft Store 安装或更新应用时，商店相关处理遵循<a href="https://privacy.microsoft.com/privacystatement">Microsoft 隐私声明（外部网站）</a>。本说明不替代第三方的隐私政策。</p>`},
    {id:'diagnostics', title:'日志与主动分享', html:`<p>应用可能在本地记录错误日志，并在本次运行内保留数量有限的近期诊断记录。提高日志级别或开启追踪会记录更多信息；异常日志可能含有错误位置、堆栈和个人文件路径。应用不会自动发送这些日志。</p><p>你主动提交的问题反馈、截图、配置或日志可能公开可见。分享前请移除用户名、个人路径、敏感文件名及其他不希望公开的信息。</p>`},
    {id:'delete', title:'保留、备份与删除', html:`<p>本地数据由你管理。自动备份默认保留最近 10 份快照，手动导出的备份由你自行保管。退出应用后可以删除相应数据目录来清除本地设置，删除前请先保存需要保留的信息。</p><ul><li><strong>EXE 安装版：</strong>卸载默认保留配置；取消“保留用户配置”可清理当前账户默认数据目录。</li><li><strong>MSI 安装版：</strong>卸载始终保留用户配置，彻底清理需自行删除默认数据目录。</li><li><strong>便携或自定义位置：</strong>便携数据、自定义目录、其他账户数据和手动导出的备份需要分别处理。</li></ul><p>Windows 的应用历史、缓存和备份，以及第三方服务持有的数据，不由 LucidDesk 卸载器统一删除。</p>`},
    {id:'website', title:'本官网的显示偏好', html:`<p>为了记住你选择的浅色、深色或跟随系统模式，官网在当前浏览器的本地存储中保存 <code>luciddesk-theme</code>。语言选择以 <code>luciddesk-language</code> 保存在本地，切换语言无需刷新页面。清除本站的网站数据即可移除这些偏好。官网托管于 Cloudflare，访问页面和下载发布包会向相应服务发起网络请求。</p>`},
    {id:'contact', title:'问题与政策更新', html:`<p>有关数据处理的问题，可通过<a href="https://github.com/Yuch3nE/luciddesk/issues">问题反馈（GitHub）</a>联系维护者。请勿在公开讨论中附上敏感个人数据。隐私说明将随应用实际功能变化更新。</p>`},
  ],
});
