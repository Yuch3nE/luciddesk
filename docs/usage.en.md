<!-- manual-version: 0.26.1 -->

# LucidDesk User Manual

[简体中文](usage.md) · English

Start with your first panel, then look up any task you need below.

## Start in five minutes

For LucidDesk 0.26.1. Requires **Windows 10 1809 (build 17763), x64, or later**. Mica and Mica Alt require Windows 11.

1. Install or fully extract LucidDesk and launch it. Find its tray icon near the Windows clock (expand hidden icons if needed).
2. Right-click the tray icon and choose **New panel**.
3. Drag desktop icons into the panel to organize them by task, study, or project.
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

You can drag files directly from a desktop panel into Windows Terminal and other apps (for example, to insert file paths). If you drag an icon over another window and return to a panel, it lands where you release it.

> **Safety note:** Collecting icons, removing them from a panel, or closing a panel does not delete files. Context-menu commands such as delete, rename, and cut affect real files, even when the panel is locked.

### Icons only

Choose **Icons only** under **View** to hide filenames and view full names on hover. When **Icons only** is active, an **Icon size** menu appears (**Small**, **Medium**, or **Large**). Each panel saves its own view and icon size; switch back to **Icons and names** to show filenames again.

### Automatic classification & desktop organization

Use the **Desktop organization** window to automatically sort newly added icons by rules or preview and organize existing desktop icons anytime—**original files always stay in place on disk**.

1. **Manage rules:** First create the desktop panels you want as destinations, then open **Settings → Automatic classification → Open desktop organization** and open the **Manage rules** tab:
   - Click **Add rule** to create a custom rule, or pick a built-in template (**Documents / Images / Archives / Shortcuts**) under **Rule templates**.
   - Choose a **Target group**, then configure **Icon type** (**File**, **Folder**, **Shortcut**, or **Any type**) along with **Extensions**, a **Keyword**, or a **Regular expression** (`Ctrl + A` selects all text inside editor fields).
   - The rules table displays a compact **Conditions** summary on each row; click a row to expand or collapse full details. Disabled rules appear dimmed for quick scanning.
   - Rules are matched from top to bottom and the first available match wins. Drag the grip handle (or focus a row and press `Alt + ↑` / `Alt + ↓`) to change priority. Deleting a rule asks for confirmation and can be undone via **Undo deletion** in the footer.
2. **Organize existing icons:** Choose **Organize now** from the tray menu (or switch to the **Organize now** tab) to preview where all unassigned desktop icons will go:
   - **Icons to organize** are grouped by destination with the matched rule number. **Skipped icons** start collapsed—click to expand and check why items were skipped (such as active downloads, temporary files still being written, virtual system icons, or no matching rule).
   - If you edit rules while a preview is open, a banner prompts you to click **Refresh preview**. If desktop items change before applying, LucidDesk refreshes the preview with a diff and asks you to confirm.
   - After applying, click **Undo last organization** anytime to revert.
3. **Automatically organize new icons:** Turn on **Automatically organize new icons** to classify newly added desktop icons automatically; existing icons remain managed via **Organize now**.

### Find or hide panels

- **Show all panels:** Click the tray icon or right-click and choose **Show panels**. You can also enable a global shortcut under **Settings → Panel layout → Show all panels** (preset: `Ctrl + Shift + D`).
- **Double-click to hide desktop:** Enable under **Settings → General → Desktop gestures**. Double-click empty desktop space to hide all icons and panels; double-click again or click the tray icon to restore them.

## Tabs

Combine related desktop panels into one window and switch between them using tabs.

- **Create:** Choose **New panel tab** from the panel menu or press `Ctrl + T`.
- **Merge:** Drag one desktop panel onto another’s header and release when the merge preview appears.
- **Detach:** Right-click a tab and choose **Detach as panel**.
- **Switch or close:** `Ctrl + Tab` moves to the next tab, `Ctrl + Shift + Tab` moves to the previous tab, and `Ctrl + W` closes the current tab.

Under **Settings → Panel layout → Tab alignment**, choose **Fill** (default) or **Left aligned**:
- **Left aligned:** Sizes tabs to their titles while keeping the collapse and menu buttons visible; when space gets tight, tabs shrink evenly and full titles appear on hover.
- **Fill:** Keeps the active tab visible on overflow; switch other tabs via the context menu or keyboard shortcuts.

> Only ordinary desktop panels support tabs; folder and search panels cannot merge. Closing a tab does not delete files, and closing the window closes all tabs inside it.

Selected tabs use a borderless highlight: solid backdrops adapt contrast to the chosen background color, while other materials follow the light or dark theme. The default light solid color is `#E9EDF2` (`#202020` in dark mode, `85%` opacity by default); upgrades preserve saved custom colors.

## Folder panels

1. Choose **New folder panel…** from the tray menu and select a directory.
2. Double-click (or press `Enter`) to open a subfolder; press `Alt + Left` to go back.
3. Under **View** in the panel menu, switch between **Icons and names**, **Icons only** (Small / Medium / Large), and **List** view. In list view, click column headers to sort or use **Columns** to customize visible columns.

Contents refresh automatically when files change; press `F5` to refresh manually. To open subfolders in File Explorer instead, change **Open subfolders in** under **Settings → Folder panels**.

> **File operation notice:** Dropping files in copies them to the current directory. Delete, cut, rename, and paste modify real files. Closing a folder panel does not delete the directory.

## Everything search

1. Install and run Everything separately (version 1.4 or newer with IPC enabled).
2. Open **Settings → Everything search** and enable **Enable search panel**. Browse for `Everything.exe` manually if it is not detected automatically.
3. Type a filename or query and double-click (or press `Enter`) to open a result. Press `Ctrl + Enter` to open the file location, or `Ctrl + Shift + C` to copy the full path.

Everything query syntax is supported—for example, `ext:pdf` for PDFs or `folder:` for folders. If no results appear, confirm Everything is running with IPC enabled and press `F5` to retry.

## File preview

1. Install PowerToys Peek or QuickLook separately.
2. Open **Settings → File preview**, select your preview app, and turn on **Enable file preview** (defaults to `Space`, customizable).
3. Select a file in a panel and press `Space` to preview it.

> Everything and preview tools are not bundled with LucidDesk and are off by default. Check each tool’s own OS requirements (such as [PowerToys](https://learn.microsoft.com/windows/powertoys/install)), as LucidDesk’s Windows 10 1809 minimum does not guarantee third-party compatibility.

## Theme, materials and appearance

Configure rendering mode, app theme, and window material in **Settings → Theme & materials**. Adjust corners, borders, tabs, text appearance, and snapping in **Settings → Panel layout** (Mica and Mica Alt require Windows 11).

- **Solid color:** Choose a preset or fine-tune RGB / HEX values and panel opacity; click **Reset solid color** to restore defaults.
- **Acrylic and Mica:** Adjust **Effect strength** (center is Default). Mica Alt uses a fixed effect.
- **Rendering mode:** **Custom rendering** (default) supports an adjustable corner radius and border toggle without system shadows. **System rendering** uses system shadows and provides **Square corners**, **Small corners**, or **Standard corners** under **Corner style**.
- **Edge snapping & snap gap:** When snapping is enabled, set the pixel gap between snapped panels and screen edges under **Moving and snapping** (`0` leaves no gap).

### Defaults and existing panels

Options marked **New panels only** (such as default view, list columns, default icons-only mode, and default icon arrangement) apply only to newly created panels. Use an existing panel’s own menu to change its view, arrangement, or icon size.

**Overall scale** (`50%–200%`) applies only to icons with filenames and does not stack with **Icons only** sizes.

### Fonts and language

Search and choose a font supporting the active language in **Settings → Fonts**. Switch among 7 interface languages in **Settings → General → Language**. Both changes apply immediately.

### Common switches

After sign-in, LucidDesk waits for the Windows desktop to become ready and retries automatically. Connection warnings are deferred for the first 30 seconds, so there is no need to launch the app repeatedly.

- **Start at sign-in:** Toggle in **Settings → General → Startup**.
- **Use Windows 11 context menus:** Available on Windows 11 under **Settings → Panel layout** to toggle compact context menus for regular desktop panel items.

If an option is missing, verify your version and OS details in **Settings → About**.

## Backup and restore

### Save a backup

Open **Settings → Backup & restore** and click **Back up now** to create a manual snapshot anytime. **Automatic backups** are enabled by default whenever settings or layouts change (unchanged contents are skipped); the default minimum interval is 5 minutes and the default retention is 10 snapshots, both configurable.

Click **Manage backups** to view, restore, export, or delete history. Before moving to a new PC, export a backup copy and transfer your original files separately.

### Restore a backup

1. Click **Restore from file…** (or pick a snapshot in **Manage backups**) and select a backup file.
2. Review the integrity check and confirm. LucidDesk automatically saves a **Before restore** snapshot before applying the backup.
3. Verify your panels, folder paths, and Everything / preview tool paths.
4. To revert, click **Undo this restore** or restore the **Before restore** snapshot from history.

> **Note:** Restoring replaces current settings and layouts without merging. Backups do not include original files, individual plugin settings, or external tools.

Editing settings while a restore is being prepared cancels that restore; stop editing and select the backup again. When exporting, choose a new filename—you cannot overwrite the active database or use `workspace.db` or `config.toml`.

<details>
<summary>Where are backups saved? How do I export one?</summary>

Choose **Open configuration folder** from the tray menu to open your active directory; backups live in its `backups` subfolder. Installed and standard ZIP builds default to `%LOCALAPPDATA%\LucidDesk`; portable builds default to `data` beside the app.

Under **Settings → Backup & restore → Advanced options**, you can open the configuration folder, reload configuration from disk, or **Export configuration**. Automatic backups keep 10 snapshots by default (configurable to 10, 20, or 50); UI restores keep the last 5 pre-restore snapshots. Manage manual backups and exported copies yourself.

</details>

### Update LucidDesk

Save or export a backup in **Backup & restore** before updating. When a signed update is available, EXE, standard ZIP, and portable ZIP builds can click **Download and update** in **Settings → About**; LucidDesk verifies the package locally and updates in place after you confirm restart. Automatic update checks run at startup and every 24 hours by default and can be disabled anytime.

For manual updates, choose **Exit LucidDesk** from the tray first and use the same package format. Keep the `data` folder and extensionless `portable` marker when replacing portable files. See [installation and updates](installer.md) for details.

## Agents and the command line

To let an external agent help organize your workspace:

1. Open **Settings → General → Automation** and confirm **Allow CLI and Agent control** is enabled (on by default).
2. Click **Copy prompt** under **Set up Agent skills** and send it to your agent (copying the prompt does not install anything or alter your desktop).
3. Have the agent inspect your current layout and preview its plan before applying changes.

Good practices:
- **Review diffs:** Preview summaries let you review changes quickly; if a summary is truncated or mentions additional changes, ask the agent to inspect the full diff before confirming.
- **Concurrency & timeouts:** If a preview reports waiting for a save or another backup task, wait per the returned guidance and preview again. If a submission times out, check its receipt before retrying to avoid duplicate actions.
- **Backup commands:** The CLI can list, create, export, and restore backups (exports never overwrite existing files; restores save a pre-restore snapshot first). See the [CLI guide](cli.md#english-quick-start), or use **Backup & restore** directly.
- **Privacy:** External agents may receive queried filenames, paths, and layout details—choose a provider matching your privacy needs. Turning off **Allow CLI and Agent control** blocks future connections without undoing submitted changes.

## Keyboard and file operations

Click a panel to focus it first. Common shortcuts:

| Shortcut | Action |
| --- | --- |
| `Enter` | Open selected items (enters subfolders in folder panels by default) |
| `Space` | Preview selected file (when enabled; customizable) |
| `F2` | Rename one item |
| `F5` | Refresh the current panel |
| `Ctrl + A` | Select all items in the panel (or all text inside rule editor inputs) |
| `Ctrl + C` / `Ctrl + X` / `Ctrl + V` | Copy / cut / paste |
| `Delete` | Delete real files |

<details>
<summary>More selection, navigation, search, and organization shortcuts</summary>

- `Ctrl + click`: toggle individual items; `Shift + click`: select a contiguous range.
- Arrow keys, `Home / End`, and `PageUp / PageDown`: move selection.
- Hold `Shift` while navigating to extend selection; hold `Ctrl` to move focus only (`Ctrl + Space` toggles selection on the focused item).
- `Shift + F10` or Menu key: open the context menu; `Esc`: cancel dragging or clear selection.
- While renaming: `Enter` confirms and `Esc` cancels.
- Folder panels: `Alt + Left` returns to the parent folder.
- Search panel: `Ctrl + Enter` opens the file location; `Ctrl + Shift + C` copies the full path.
- Desktop organization window: `Ctrl + Tab` / `Ctrl + Shift + Tab` switches tabs; focus a rule row and press `Alt + ↑` / `Alt + ↓` to reorder priority.

</details>

## Troubleshooting

| Problem | Try this |
| --- | --- |
| Missing panels | Expand hidden tray icons and click the LucidDesk tray icon to show all panels |
| Cannot collect desktop icons | Check **Desktop connection** in **Settings → About** and click **Reconnect**; avoid running multiple desktop organizers |
| Slow updates after dragging | Regular panels only update icon membership—check connection status in **About**; dropping large files into a folder panel copies real files and depends on disk speed |
| Panels have not connected at startup | LucidDesk retries automatically and defers warnings for 30 seconds; copy diagnostics from **About** if it remains disconnected |
| Desktop and panel Recycle Bin icons disagree | Report the preceding file action—there is no need to empty the Recycle Bin |
| Search or folder panel reports worker stopped | Reopen the panel as prompted; if it persists, copy diagnostics instead of repeatedly dropping files |
| Search or preview unavailable | Confirm the tool is installed, its path is valid, and the feature toggle is enabled |
| Files do not open after moving computers | Verify that original files were copied and that drive letters or folder paths still match |
| Restore fails | Keep your current data and backup file, read the error details, and do not delete config files directly |

Still stuck? Copy diagnostics from **Settings → About** and [report an issue](https://github.com/Yuch3nE/luciddesk/issues). Remove private paths and filenames before sharing.

Want an agent to help organize your desktop? See the [CLI guide](cli.md#english-quick-start).
