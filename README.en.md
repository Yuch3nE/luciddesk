<div align="center">

<img src="docs/images/app-icon.png" width="128" height="128" alt="LucidDesk" />

# LucidDesk

[简体中文](README.md) · English

**Turn your Windows desktop into a workspace you can organize yourself or with an agent.**

Desktop panels · Auto-classify · Folder panels · Everything search · Spacebar preview · Agent integration

[![License: MIT](https://img.shields.io/badge/License-MIT-2EA44F?style=flat-square&labelColor=374151)](LICENSE)
[![CI status](https://img.shields.io/github/actions/workflow/status/Yuch3nE/luciddesk/build.yml?style=flat-square&label=CI&labelColor=374151)](https://github.com/Yuch3nE/luciddesk/actions/workflows/build.yml)
[![Version: 0.26.1](https://img.shields.io/badge/Version-0.26.1-8250DF?style=flat-square&labelColor=374151)](CHANGELOG.en.md)
[![Windows: 10 / 11](https://img.shields.io/badge/Windows-10%20%2F%2011-0078D4?style=flat-square&labelColor=374151)](#compatibility)
[![Arch: x64](https://img.shields.io/badge/Arch-x64-64748B?style=flat-square&labelColor=374151)](docs/portable.md)

[🌐 Website](https://luciddesk.bbkingdom365.com/) · [⬇️ Download](https://github.com/Yuch3nE/luciddesk/releases/latest) · [✨ Explore](https://luciddesk.bbkingdom365.com/features/) · [📖 User guide](https://luciddesk.bbkingdom365.com/guide/) · [📋 Changelog](CHANGELOG.en.md)

</div>

LucidDesk is a free, open-source native desktop organizer for Windows. Group files and shortcuts into panels, sort clutter with automatic classification rules, keep frequently used folders within reach, and search or preview files right from your desktop. Its bundled CLI and Skill also let an external agent help arrange your workspace.

![LucidDesk feature illustration: panel tabs, folder browsing, search and Agent integration](docs/images/overview.en.svg)

<sub>Feature illustration. See screenshots below for the actual interface.</sub>

[Get started](#getting-started) · [Features](#features) · [Screenshots](#screenshots) · [Upgrading](#upgrading-and-backups) · [FAQ](#troubleshooting)

**Version 0.26.1** adds automatic desktop icon classification, one-click organization, and signed in-app updates. Drag files from desktop panels into Windows Terminal and other apps, and enjoy more reliable panel behavior during Win+D and tab merging. See the [changelog](CHANGELOG.en.md#0261--2026-11-15).

<a id="getting-started"></a>

## 🚀 Get started

[Download from GitHub Releases](https://github.com/Yuch3nE/luciddesk/releases/latest). The **installer** is recommended for everyday use. All three packages share the exact same features and support signed in-app updates.

| Package | Choose it if you want… | Settings location |
| --- | --- | --- |
| Installer (`*-setup.exe`) | A setup wizard, Start menu shortcut, and uninstall entry | `%LOCALAPPDATA%\LucidDesk` |
| Standard ZIP (without `portable` in its name) | No installation, keeping settings separate from program files | `%LOCALAPPDATA%\LucidDesk` |
| Portable ZIP (`portable` in its name) | Settings carried inside the app folder | The `data` folder beside the app |

1. Install or fully extract the package and launch `luciddesk.exe`. When using a ZIP build, keep all bundled files together and do not run the app directly inside the archive.
2. Right-click the tray icon, choose **New panel**, and drag desktop icons into it.
3. Drag the header to move a panel or its edges to resize it. Right-click empty space in the panel (or click **…** in the header) to change view, sorting, and arrangement.

To browse a directory, choose **New folder panel…** from the tray menu. Closing Settings does not exit the app; use **Exit LucidDesk** in the tray menu to quit.

See the [user manual](docs/usage.en.md) for everyday tasks and the [online installation guide](https://luciddesk.bbkingdom365.com/installation/) for package and migration details.

<a id="features"></a>

## ✨ Features

| Feature | What you can do |
| --- | --- |
| Desktop panels and tabs | Group icons by project, merge or detach tabs, and choose a filled or left-aligned tab bar |
| Automatic classification | Route unassigned icons by extension, keyword, regex, or icon type; includes templates, preview-first **Organize now**, and undo |
| Flexible arrangement | Auto-arrange, align to grid, or position freely; sort by name, modified date, type, or size |
| Per-panel display | Choose **Icons and names**, **Icons only** (Small / Medium / Large), or **List** view independently for each desktop or folder panel |
| Folder panels | Browse directories and subfolders directly on the desktop, with automatic refresh when files change |
| Search and preview | Search local files with Everything and press Space to preview a selected file |
| Appearance and focus | Customize themes, materials, rendering mode, corners, fonts, and snap gap; collapse, auto-collapse, lock, or pin panels on top |
| Double-click to hide | Enable in **Settings → General**, then double-click empty desktop space to hide or restore icons and panels |
| Backup and restore | Back up settings and layouts automatically or manually, export copies, restore snapshots, and undo a restore |
| Agents and CLI | Let an external agent inspect the desktop, preview and apply changes, or script panel and backup workflows |

> [!NOTE]
> **Desktop panels and automatic classification store icon references only — they never move your original files.** Drag icons back to the desktop to remove them; closing a panel does not delete its files. File commands such as delete, rename, and cut, as well as dropping or pasting into a folder panel, affect real files.

### Search and preview

Both features are off by default and require separately installed software:

| Feature | Setup |
| --- | --- |
| File search | Install and run Everything (1.4 or newer with IPC enabled), then enable it in **Settings → Everything search** |
| Spacebar preview | Install PowerToys Peek or QuickLook, then choose it and enable preview in **Settings → File preview** |

These tools are not bundled with LucidDesk and may have higher OS requirements. For example, current [PowerToys](https://learn.microsoft.com/windows/powertoys/install) requires Windows 10 2004 (build 19041) or later; LucidDesk’s 1809 minimum does not imply Peek support on 1809.

### Agents and the command line

Open **Settings → General → Automation**, verify that **Allow CLI and Agent control** is on (enabled by default), and click **Copy prompt** under **Set up Agent skills** to give the prompt to your agent. Keep the app, CLI, and Skill from the same release package. Copying the prompt does not install anything or alter your desktop.

With control enabled, run read-only queries from the application directory:

```powershell
.\luciddesk-cli.exe status --json
.\luciddesk-cli.exe pane list --json
```

Use a three-step workflow: inspect existing panels and items, preview changes, then apply after review. This command previews a new panel without saving it:

```powershell
.\luciddesk-cli.exe pane create --title "Work" --dry-run --json
```

Review the returned summary and diff, then follow `data.next_step`. **Mutation commands without `--dry-run` preview and submit immediately.** If a submission times out, query its original receipt before retrying to avoid duplicating the operation. If a backup task is busy, follow the returned wait condition and preview again once idle.

Structured JSON output and optional diagnostic logs support agents and scripts. LucidDesk does not include a language model or require a model API key. See the [CLI guide](docs/cli.md#english-quick-start) for commands and recovery, or the [bundled Skill](skills/luciddesk-control/SKILL.md) for agent integration. External agents may send filenames and paths to their providers; see the [privacy policy](PRIVACY.en.md).

<a id="screenshots"></a>

## 📸 Screenshots

| Desktop panel | Theme & materials |
| :---: | :---: |
| <a href="screenshot/en/pane.png"><img src="screenshot/en/pane.png" height="220" alt="Desktop panel in English" /></a> | <a href="screenshot/en/setting.png"><img src="screenshot/en/setting.png" height="220" alt="Theme and material settings in English" /></a> |
| Drag in icons to organize your desktop | Customize the theme and background |
| **Folder panel** | **About & status** |
| <a href="screenshot/en/folder.png"><img src="screenshot/en/folder.png" height="220" alt="Folder panel list view in English" /></a> | <a href="screenshot/en/about.png"><img src="screenshot/en/about.png" height="220" alt="About page and runtime status in English" /></a> |
| Browse folders with type, modified date, and size columns | View version, project links, and runtime status |

Click a screenshot to view the original. These show dark Acrylic; appearance varies with your wallpaper and system settings. Mica and Mica Alt require Windows 11.

<a id="upgrading-and-backups"></a>

## 🔄 Upgrading and backups

Check for updates manually or automatically in **Settings → About**. When a signed update is available, click **Download and update**; LucidDesk verifies the package locally and updates in place once you confirm the restart. Save a backup in **Settings → Backup & restore** before upgrading.

- **Installer:** exit the app and run the new installer using the same installation scope and directory.
- **Standard ZIP:** exit the app and replace all program files while keeping your settings directory.
- **Portable:** exit the app and replace program files while preserving the `data` folder and extensionless `portable` marker.

**Backups contain settings and layouts, not your original files.** When moving to another computer, transfer the files referenced by panels separately. See [backup and restore](docs/usage.en.md#backup-and-restore).

<a id="languages"></a>
<a id="compatibility"></a>

## 🖥️ System and languages

| Environment | Support |
| --- | --- |
| Windows 11 x64 | Primary platform; solid color, Acrylic, Mica, and Mica Alt |
| Windows 10 1809+ x64 | Tested by users; solid color and Acrylic |
| ARM64 / Remote Desktop | Not fully validated |

Supports 7 interface languages: 简体中文, 繁體中文, English, 日本語, 한국어, Deutsch, and Русский. It follows the Windows display language by default and falls back to English for unsupported locales. Switch anytime in **Settings → General → Language**; changes apply immediately.

<a id="troubleshooting"></a>

## 💬 FAQ

<details>
<summary>Panels take a long time to appear after sign-in</summary>

LucidDesk checks desktop interfaces in the background and connects as soon as they are ready, retrying automatically after Explorer restarts. Connection warnings are deferred for the first 30 seconds after startup or disconnection; persistent failures still show a warning. If the app is running but panels have not appeared, check **Desktop connection** and copy diagnostics in **Settings → About**. If the tray icon has not appeared either, check Task Manager to see whether the app has launched: Windows may delay startup apps, and internal readiness checks do not remove that OS delay.

</details>

<details>
<summary>Icons take a long time to update after dragging</summary>

Check your version in **Settings → About**. Version 0.25.2 fixes delayed updates caused by lost notifications. A desktop panel only updates icon membership, whereas dropping into a folder panel copies real files, so large transfers still depend on file size and disk speed. If an unexpected delay persists after upgrading, record the drag direction, panel type, and waiting time, and copy diagnostics for your report.

</details>

<details>
<summary>Icons and panels disappeared</summary>

Click the tray icon to show panels. If **Double-click to hide desktop** is enabled, double-click an empty desktop area again to restore them.

</details>

<details>
<summary>Desktop panels cannot connect</summary>

Check the connection status in **Settings → About** and click **Reconnect**, and make sure no other desktop organizer is running. The app retries automatically after Explorer restarts; if the issue persists, copy diagnostics for your report.

</details>

<details>
<summary>How do I report a problem?</summary>

Open an [issue](https://github.com/Yuch3nE/luciddesk/issues) with reproduction steps, expected and actual behavior, and the output from **Settings → About → Copy diagnostics**. Include a screenshot and display scaling for UI issues, and remove personal paths or sensitive filenames before sharing.

</details>

<a id="documentation"></a>

## 📚 More help

[User manual](docs/usage.en.md) · [CLI quick start](docs/cli.md#english-quick-start) · [Changelog](CHANGELOG.en.md) · [Privacy policy](PRIVACY.en.md) · [Documentation index](docs/README.md) (Chinese)

<a id="build-from-source"></a>
<a id="contributing"></a>

## 🤝 Development and contributions

Bug reports, feature suggestions, translation improvements, and pull requests are welcome. Read the [contribution guide](CONTRIBUTING.en.md) before getting started.

LucidDesk is built with Rust, Win32, and Direct2D. See the [build guide](docs/development/build.md) for compilation and packaging, [development documentation](docs/development/README.md) for implementation and validation, and [website documentation](website/README.md) for site maintenance. These technical guides are in Chinese.

Author: **Yuchen95**.

<a id="friends"></a>

## 🔗 Friends

[Linux DO](https://linux.do/)

<a id="license"></a>

## 📄 License

LucidDesk is licensed under the [MIT License](LICENSE). Third-party dependencies retain their respective licenses.
