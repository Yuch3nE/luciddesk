<!-- manual-version: 0.26.1 -->

# LucidDesk User Manual

[简体中文](usage.md) · English

Start with your first panel, then look up any task you need below.

## Start in five minutes

For LucidDesk 0.26.1. Requires **Windows 10 1809 (build 17763), x64, or later**. Mica and Mica Alt require Windows 11.

1. Install or fully extract LucidDesk and launch it. Find its tray icon near the Windows clock; expand hidden icons if needed.
2. Right-click the tray icon and choose **New panel**.
3. Drag desktop icons into the panel to group them by task or project.
4. Drag the header to move a panel and its edges to resize it. Drag icons back to the desktop to remove them.

Closing Settings does not exit the app. Use **Exit LucidDesk** in the tray menu to quit. Need to install it first? See [installation and updates](installer.md).

## Desktop panels

Click **…** in the panel header or right-click empty panel space to open the panel menu:

| Task | Action |
| --- | --- |
| Switch icon or list view | Open **View** and choose **Icons and names**, **Icons only**, or **List** |
| Sort by name, modified date, type, or size | Open **Sort by**; select the same field again to toggle ascending / descending |
| Pack icons without gaps | Enable **Auto arrange icons** in **View**; disable it to leave gaps |
| Position icons freely | Disable **Align to grid** in **View** (also disables auto arrange) |
| Temporarily hide panel contents | Choose **Collapse panel** (or enable **Auto-collapse**) |
| Fix the panel’s position, size, and name | Enable **Lock panel** (you can still organize icons, sort, or switch tabs) |
| Keep a panel above other windows | Enable **Always on top** |

You can drag files from a desktop panel into Windows Terminal and other apps (for example, to insert file paths). If you drag an icon over another window and return to a panel, it is placed where you release it.

**Collecting icons, removing them from a panel, or closing a panel does not delete files.** File-menu commands such as delete, rename, and cut affect real files, even when the panel is locked.

### Icons only

Choose **Icons only** under **View** to hide filenames; hover over an icon to read its full name. When **Icons only** is active, an **Icon size** menu appears where you can pick **Small**, **Medium**, or **Large**. Each panel saves its own view and icon size; switch back to **Icons and names** to show filenames again.

### Automatic classification

First create the desktop panels you want to use as destinations. Open **Settings → Automatic classification** and click **Open desktop organization**. On the **Manage rules** tab, click **Add rule** (or start from the built-in **Documents / Images / Archives / Shortcuts** rule templates), choose a target group, and set an icon type (**File**, **Folder**, **Shortcut**, or **Any type**) along with extensions, a keyword, or a regular expression. You can drag rules to reorder priority, toggle individual rules on or off, and undo deletions. Rules run from top to bottom; the first enabled match with an available group wins.

Choose **Organize now** from the tray menu (or switch to the **Organize now** tab in the **Desktop organization** window) to preview where unassigned icons will go. Expand **Skipped icons** to see why items were skipped (such as active downloads, temporary files, virtual system icons, or no matching rule), then refresh the preview or click **Apply** at the bottom. If desktop icons change before applying, LucidDesk refreshes the preview and asks you to review before confirming. After applying, you can click **Undo last organization** to revert. Enable **Automatically group new icons** to classify newly appearing unassigned icons automatically; original files always remain in place on disk.

### Find or hide panels

Click the tray icon (or right-click and choose **Show panels**) to raise all panels. You can also enable the global shortcut under **Settings → Panel layout → Show all panels**; the preset is `Ctrl + Shift + D`.

To clear the desktop temporarily, enable **Double-click to hide desktop** under **Settings → General → Desktop gestures**. Double-click empty desktop space to hide all icons and panels; double-click again or click the tray icon to restore them.

## Tabs

Combine related desktop panels into a single window and switch between tabs.

Under **Settings → Panel layout → Tab alignment**, choose **Fill** (default) or **Left aligned**. **Left aligned** sizes tabs to their titles while keeping the collapse and menu buttons visible; when space gets tight, tabs shrink evenly and long titles show in full on hover. **Fill** keeps the active tab visible on overflow; use the context menu or keyboard shortcuts to switch tabs.

- **Create:** choose **New panel tab** from the panel menu or press `Ctrl + T`.
- **Merge:** drag one desktop panel onto another’s header and release when the merge preview appears.
- **Detach:** right-click a tab and choose **Detach as panel**.
- **Switch or close:** `Ctrl + Tab` moves to the next tab, `Ctrl + Shift + Tab` moves to the previous tab, and `Ctrl + W` closes the current tab.

Only ordinary desktop panels support tabs; folder and search panels cannot merge. Closing a tab does not delete files, and closing the window closes all tabs inside it.

Selected tabs use a borderless highlight: with a solid backdrop, contrast adapts to the chosen background color; other materials follow the light or dark theme. The default light solid color is `#E9EDF2` (`#202020` in dark mode, `85%` opacity by default); upgrading preserves saved custom colors.

## Folder panels

1. Choose **New folder panel…** from the tray menu and pick a directory.
2. Double-click (or press `Enter`) to open a subfolder; press `Alt + Left` to go back.
3. Under **View** in the panel menu, switch between **Icons and names**, **Icons only** (Small / Medium / Large), and **List** view. In list view, click column headers to sort or use **Columns** to choose visible columns.

Contents refresh automatically when files change; press `F5` to refresh manually. To open subfolders in File Explorer instead, change **Open subfolders in** under **Settings → Folder panels**.

**Dropping files in copies them to the current directory. Delete, cut, rename, and paste modify real files.** Closing a folder panel does not delete the directory.

## Everything search

1. Install and run Everything separately (version 1.4 or newer with IPC enabled).
2. Open **Settings → Everything search** and turn on **Enable search panel**. Browse for `Everything.exe` manually if it is not detected automatically.
3. Type a filename or query and double-click (or press `Enter`) to open a result. Press `Ctrl + Enter` to open the file location, or `Ctrl + Shift + C` to copy the full path.

Everything query syntax is supported — for example, `ext:pdf` for PDFs or `folder:` for folders. If results are missing, verify that Everything is running with IPC enabled and press `F5` to retry.

## File preview

1. Install PowerToys Peek or QuickLook separately.
2. Open **Settings → File preview**, choose your preview app, and turn on **Enable file preview** (the shortcut defaults to Space and can be customized).
3. Select a file in a panel and press Space to preview it.

Everything and preview tools are not bundled with LucidDesk, and both features are off by default. Third-party tools have their own OS requirements — check the requirements for [PowerToys](https://learn.microsoft.com/windows/powertoys/install) or your chosen tool, as LucidDesk’s Windows 10 1809 minimum does not mean every preview tool supports 1809.

## Theme, materials and appearance

Choose the rendering mode, app theme, and window material in **Settings → Theme & materials**. Adjust corners, borders, tabs, text appearance, and snapping in **Settings → Panel layout**. Mica and Mica Alt require Windows 11.

- **Solid color:** choose a preset or adjust RGB / HEX values and panel opacity; click **Reset solid color** to restore defaults.
- **Acrylic and Mica:** adjust **Effect strength** (center is Default). Mica Alt uses a fixed effect.
- **Rendering mode:** switchable in **Theme & materials**. **Custom rendering** (default) supports an adjustable corner radius and border toggle in **Panel layout** without system shadows. **System rendering** uses system shadows and offers **Square corners**, **Small corners**, or **Standard corners** under **Corner style** in **Panel layout**.
- **Edge snapping & snap gap:** when snapping is enabled, set the pixel spacing between snapped panels and screen edges under **Moving and snapping** (`0` leaves no gap).

### Defaults and existing panels

Options marked **New panels only** (such as default view, list columns, default icons-only mode, and default icon arrangement) do not modify existing panels. Use an existing panel’s own menu to change its view, arrangement, or icon size.

**Overall scale** (`50%–200%`) applies to icons with filenames and does not stack with **Icons only** sizes.

### Fonts and language

Search and choose an interface font in **Settings → Fonts** (only fonts supporting the active language are listed). Switch among 7 interface languages in **Settings → General → Language**; changes apply immediately.

### Common switches

After sign-in, LucidDesk waits for the Windows desktop to become ready and retries automatically. Connection warnings are deferred for the first 30 seconds — this is a grace period, not a connection deadline, so there is no need to launch the app repeatedly.

- **Start at sign-in:** toggle in **Settings → General → Startup**.
- **Use Windows 11 context menus:** available on Windows 11 under **Settings → Panel layout** to toggle compact context menus for regular desktop panel items.

If an option is missing, check your version and OS details in **Settings → About**.

## Backup and restore

### Save a backup

Open **Settings → Backup & restore** and click **Back up now** to create a manual snapshot anytime. **Automatic backups** are enabled by default and save whenever settings or layouts change (unchanged contents are not backed up repeatedly); the default minimum interval is 5 minutes and the default retention is the last 10 automatic backups, both configurable.

Click **Manage backups** to view, restore, export, or delete saved snapshots. Before moving to another computer, export a backup copy and transfer your original files separately.

### Restore a backup

1. Click **Restore from file…** (or pick a record in **Manage backups**) and choose a backup file.
2. Review the integrity check and confirm. LucidDesk automatically saves a **Before restore** backup before applying the selected snapshot.
3. Verify your panels, folder paths, and search / preview tool paths.
4. To go back, click **Undo this restore** or restore the **Before restore** snapshot from history.

**Restoring replaces current settings and layouts; it does not merge them. Backups do not include original files, individual plugin settings, or external tools.**

Editing settings while a restore is being prepared cancels that restore; stop editing and select the backup again. When exporting, choose a new file name — you cannot overwrite the active database or use `workspace.db` or `config.toml` as the filename.

<details>
<summary>Where are backups saved? How do I export one?</summary>

Choose **Open configuration folder** from the tray menu to locate your active directory; backups live in its `backups` subfolder. Installed and standard ZIP builds default to `%LOCALAPPDATA%\LucidDesk`; portable builds default to `data` beside the app.

Under **Settings → Backup & restore → Advanced options**, you can open the configuration folder, reload configuration from disk, or **Export configuration**. Automatic backups keep the last 10 snapshots by default (configurable to 10, 20, or 50). UI restores keep the last 5 pre-restore backups by default. Manage manual backups and exported copies yourself.

</details>

### Update LucidDesk

Save or export a backup in **Backup & restore** before updating. When a signed update is available, EXE, standard ZIP, and portable ZIP builds can click **Download and update** in **Settings → About**; LucidDesk verifies the package locally and updates in place once you confirm the restart. Automatic update checks are on by default (at startup and every 24 hours) and can be turned off anytime.

For a manual update, choose **Exit LucidDesk** from the tray first and use the same package format. Keep the `data` folder and extensionless `portable` marker when replacing portable program files. See [installation and updates](installer.md) for details.

## Agents and the command line

To connect an external agent, open **Settings → General → Automation**, confirm that **Allow CLI and Agent control** is on (enabled by default), and click **Copy prompt** under **Set up Agent skills** to paste into your agent. Copying the prompt does not install anything or alter your desktop. Have the agent inspect your current layout and preview its plan before applying changes.

Preview summaries help you review changes quickly. If a summary is truncated or mentions additional items, have the agent inspect the full diff before you approve it.

If a preview reports that it is waiting for a save or another backup task, have the agent follow the returned guidance and preview again once ready. If a submission times out, check its receipt before retrying to avoid duplicating an operation.

**Backup commands:** the CLI can list, create, export, and restore backups. Exports never overwrite existing files; restoring replaces current configuration after saving a pre-restore backup. See the [CLI guide](cli.md#english-quick-start). Without an agent or CLI, use the **Backup & restore** page directly.

External agents may receive queried filenames, paths, and layout details — choose a provider matching your privacy requirements. Turning off **Allow CLI and Agent control** blocks future connections but does not undo already-submitted changes.

## Keyboard and file operations

Click the panel you want to focus first. Common shortcuts:

| Shortcut | Action |
| --- | --- |
| `Enter` | Open selected items (enters subfolders in folder panels by default) |
| `Space` | Preview selected file (when enabled; customizable) |
| `F2` | Rename one item |
| `F5` | Refresh the current panel |
| `Ctrl + A` | Select all items in the current panel |
| `Ctrl + C` / `Ctrl + X` / `Ctrl + V` | Copy / cut / paste |
| `Delete` | Delete real files |

<details>
<summary>More selection, navigation, and search shortcuts</summary>

- `Ctrl + click`: toggle individual items; `Shift + click`: select a contiguous range.
- Arrow keys, `Home / End`, and `PageUp / PageDown`: move selection.
- Hold `Shift` while navigating to extend selection; hold `Ctrl` to move focus only (use `Ctrl + Space` to toggle selection on the focused item).
- `Shift + F10` or Menu key: open the context menu; `Esc`: cancel dragging or clear selection.
- While renaming: `Enter` confirms and `Esc` cancels.
- Folder panels: `Alt + Left` returns to the parent folder.
- Search panel: `Ctrl + Enter` opens the file location; `Ctrl + Shift + C` copies the full path.

</details>

## Troubleshooting

| Problem | Try this |
| --- | --- |
| Missing panels | Expand hidden tray icons and click the LucidDesk tray icon to show all panels |
| Cannot collect desktop icons | Check **Desktop connection** in **Settings → About** and click **Reconnect**; avoid running multiple desktop organizers |
| Icons update late after dragging | Confirm you are on 0.25.3 or later; check connection status in **About** and record drag direction, panel type, and delay |
| Panels have not connected at startup | The app retries automatically and defers warnings for 30 seconds; copy diagnostics from **About** if it remains disconnected |
| Desktop and panel Recycle Bin icons disagree | Confirm you are on 0.25.3 or later; if it persists, report the preceding file action. Emptying the Recycle Bin is not needed |
| Search or folder panel reports worker stopped | Reopen the panel as prompted; if it persists, copy diagnostics instead of repeatedly dropping files |
| Search or preview unavailable | Confirm the tool is installed, its path is valid, and the feature toggle is enabled |
| Files do not open after moving computers | Verify that original files were copied and that drive letters or folder paths still match |
| Restore fails | Keep your current data and backup file, read the error details, and do not delete config files directly |

Collecting or releasing icons in a desktop panel only changes membership. Dropping files into a folder panel copies real files to that directory, so file size and disk speed still affect wait time. Version 0.25.3 includes the delayed drag-and-drop refresh fix, though copying large files naturally takes time.

Still stuck? Copy diagnostics from **Settings → About** and [report an issue](https://github.com/Yuch3nE/luciddesk/issues). Remove private paths and filenames before sharing.

Want an agent to help organize your desktop? See the [CLI guide](cli.md#english-quick-start).
