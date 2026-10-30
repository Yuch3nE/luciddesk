<!-- manual-version: 0.25.0 -->

# LucidDesk User Manual

[简体中文](usage.md) · English

Start with your first panel, then find the task you need below.

## Start in five minutes

For LucidDesk 0.25.0.

1. Install and start LucidDesk. Find its tray icon near the Windows clock; expand hidden icons if needed.
2. Right-click the tray icon and choose New panel.
3. Drag desktop icons into the panel to group them by task or project.
4. Drag the header to move the panel and its edges to resize it. Drag icons back to the desktop to remove them.

Closing Settings does not exit the app. Use Exit LucidDesk in the tray menu. Need to install it first? See [installation and updates](installer.md).

## Desktop panels

Open the header's “…” menu or right-click empty panel space.

| Task | Action |
| --- | --- |
| Switch icons or list view | Choose a view in View |
| Sort by name, type, date or size | Choose Sort by; select the same field again to reverse direction |
| Keep icons together | Enable Auto arrange icons; disable it to leave gaps |
| Position icons freely | Disable Align icons to grid |
| Temporarily hide panel contents | Collapse the panel, then expand it when needed |
| Prevent accidental movement | Check Lock panel; resizing remains available |
| Keep a panel above other windows | Enable Always on top |

**Collecting icons, removing them from a panel or closing a panel does not delete files.** File-menu commands such as delete, rename and cut affect real files, even when the panel is locked.

### Icons only

Choose Icons only in View to hide filenames; hover to read the full name. Choose small, medium or large under Icon size. Each panel saves its own choice. Choose Icons with names to show filenames again.

### Find or hide panels

Click the tray icon to show panels. You can also enable the Show all panels shortcut in Settings → Panel layout; its preset is `Ctrl + Shift + D`.

To clear the desktop temporarily, enable Double-click to hide desktop in General. Double-click empty desktop space to hide icons and panels; double-click again to restore them.

## Tabs

Keep related panels in one window and switch between tabs.

Under Settings → Panel layout → Tab alignment, choose Fill or Left aligned. Fill is the default. Left aligned sizes tabs to their titles and leaves unused space on the right. Left aligned keeps the collapse and menu buttons visible and shrinks all tabs when space is limited. Long titles are truncated; hover to see the full title. Fill keeps the active tab visible when tabs overflow; use the context menu or keyboard shortcuts to switch to other tabs.

- **Create:** use the panel menu or press `Ctrl + T`.
- **Merge:** drag one panel onto another's header and release when the merge preview appears.
- **Detach:** right-click a tab and choose Detach as separate panel.
- **Switch or close:** `Ctrl + Tab` switches tabs; `Ctrl + W` closes the current tab.

Only ordinary desktop panels support tabs. Folder and search panels cannot merge. Closing a tab does not delete files; closing the window closes all its tabs.

Selected tabs use a borderless background. With a solid material, tab contrast follows the selected background color; other materials follow the light or dark theme. The default light solid color is `#E9EDF2`; upgrading preserves saved colors.

## Folder panels

1. Choose New folder panel from the tray and select a directory.
2. Double-click a subfolder to enter it; press `Alt + Left` to return.
3. Choose icons or list view from the panel menu. Icon view supports icons only and small, medium or large icons; list headers let you sort and choose columns.

Changes refresh automatically; press `F5` to refresh manually. To open subfolders in File Explorer instead, change the opening behavior in Settings → Folder panels.

**Dropping files in copies them to the current directory. Delete, cut, rename and paste affect real files.** Closing the panel does not delete the directory.

## Everything search

1. Install and run Everything separately.
2. Open Settings → Everything search and enable the search panel. Select `Everything.exe` manually if needed.
3. Type a filename and double-click a result to open it. `Ctrl + Enter` locates it in File Explorer.

Use `ext:pdf` to find PDFs or `folder:` to find folders. If results are missing, check that Everything is running and press `F5` to retry.

## File preview

1. Install PowerToys Peek or QuickLook separately.
2. Select the program in Settings → File preview and enable preview.
3. Select a file and press Space.

Everything and preview tools are not bundled with LucidDesk. Search and preview are off by default.

## Theme, materials and appearance

Choose a theme and solid, acrylic or mica background in Theme & material. Use Panel layout for text, corners and snapping. Mica requires Windows 11.

- **Solid color:** edit the color and opacity, or reset them to defaults.
- **Acrylic and Mica:** adjust effect strength; the center is Default. Mica Alt uses a fixed effect.
- **System rendering:** uses system shadows with square, small-rounded or rounded corners. Choose custom rendering for an adjustable corner radius.
- **Snap gap:** adjusts the space between panels after they snap together.

### Defaults and existing panels

Settings tagged for new panels only do not change existing panels. Use a panel's own menu to adjust its view, arrangement or icon size.

Overall scaling applies to icons with filenames and does not stack with icon-only sizes.

### Fonts and language

Search and choose a font in Fonts. Select an interface language in Settings → General → Language; it applies immediately.

### Common switches

General offers sign-in startup. Panel layout offers the Windows 11-style context menu. If an option is missing, check your version in About.

## Backup and restore

### Save a backup

Open Settings → Backup & restore and choose Back up now. Automatic backups save changed settings and layouts; you can adjust the interval and number kept.

Manage backups lets you restore, export or delete saved snapshots. Before moving computers, export a copy and transfer your original files separately.

### Restore a backup

1. Choose Restore from file and select a backup.
2. Confirm the restore. LucidDesk saves a pre-restore backup before applying your selection.
3. Check panels, folder paths, search and preview tools.
4. To go back, use Undo this restore or select the pre-restore backup from history.

**Restoring replaces current settings and layout; it does not merge them. Backups do not contain original files, plugins' own settings or external tools.**

Editing settings while a restore is being prepared cancels that restore. Stop editing and select the backup again. Choose a new export file to avoid overwriting active configuration or database files.

<details>
<summary>Where are backups saved? How do I export one?</summary>

Use Open configuration folder from the tray to find the active location. Backups are in its `backups` folder. Installed versions normally use `%LOCALAPPDATA%\LucidDesk`; portable versions use `data` beside the app.

Advanced options in Backup & restore lets you export configuration or open its folder. Automatic backups keep ten snapshots by default; this is configurable. Restoring through the UI keeps five pre-restore snapshots by default. Manage manual backups and exported copies yourself.

</details>

### Update LucidDesk

Back up first, then exit the old app and install the update. For portable updates, preserve `data` and `portable`. Check for updates in About; LucidDesk does not automatically install or restart.

## Agents and the command line

To connect an agent, copy the Skill installation prompt from Settings → General → Agent & CLI and give it to your chosen agent. Copying the prompt does not install anything or change your desktop. Have the agent read the current layout and preview the changes you want before applying them.

**Backup commands:** the CLI can list, create, export and restore backups. Exports do not overwrite existing files. Restoring replaces current configuration and first saves a pre-restore backup. See the [CLI guide](cli.md#english-quick-start). You can continue using the Backup & restore settings page without an agent or command line.

Your external agent may receive queried file names, paths and layout information. Choose its service according to your privacy needs. Disabling command-line control blocks subsequent connections but does not undo submitted operations.

## Keyboard and file operations

Click the panel you want to use first.

| Shortcut | Action |
| --- | --- |
| Enter | Open selected items |
| Space | Preview, when enabled |
| F2 | Rename one item |
| F5 | Refresh |
| Ctrl+A | Select all in the current panel |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Delete | Delete real files |

<details>
<summary>More selection and navigation shortcuts</summary>

- `Ctrl + click` toggles individual selection; `Shift + click` selects a range.
- Arrow keys, `Home / End` and `PageUp / PageDown` move selection.
- Hold `Shift` while navigating to extend selection; hold `Ctrl` to move focus only.
- `Shift + F10` opens the context menu; `Esc` cancels dragging or clears selection.
- When renaming, `Enter` confirms and `Esc` cancels.

</details>

## Troubleshooting

| Problem | Try this |
| --- | --- |
| Missing panels | Expand hidden tray icons and click LucidDesk to show panels |
| Cannot collect desktop icons | Check connection status in About and reconnect; avoid running multiple desktop organizers |
| Search or preview unavailable | Check that the tool is installed, its path is correct and the feature is enabled |
| Files do not open after moving computers | Check that original files were transferred and drive letters or folder paths still match |
| Restore fails | Keep your current data and backup; read the error instead of deleting configuration files |

Still stuck? Copy diagnostics from About and [report an issue](https://github.com/Yuch3nE/luciddesk/issues). Hide private paths and filenames before sharing.

Want an agent to help organize your desktop? See the [Agent and CLI guide](cli.md).
