# Changelog

[简体中文](CHANGELOG.md) · English

Current version: **0.20.2**. Features and fixes by release.

Exit the app before upgrading a portable installation and preserve its `data` folder. See the [portable guide](docs/portable.md) (Chinese).

## Unreleased

### fix

- Statically link the VC++ runtime in Windows x64 builds so the installer preflight executable does not require preinstalled VCRUNTIME DLLs or block silent installation with a missing-DLL dialog.
- Check GUI, CLI and desktop DLL runtime imports before packaging to prevent external VC++ runtime dependencies from reappearing in EXE, MSI, MSIX or ZIP payloads.

## 0.20.2 · 2026-10-05

### fix

- Share sizing calculations between CLI content fitting and manual resizing, using rendered icon order and the last row's label height. Tab groups fit all members and retain whole icon columns.
- Preserve fitted small panel sizes through window updates, database saves and reloads instead of expanding them to the initial default minimum.
- Share edge positioning between CLI and drag snapping, using live window bounds and target-monitor DPI with a fixed 5-physical-pixel gap. Reject stale layout plans when window bounds or collapse state change after preview.

### docs

- Refine progressive Skill workflows: group and sort first, fit anchors, snap dependent panels, then verify. Clarify the default folder list view, asynchronous loading and conflict recovery.
- Document shared layout calculations, size persistence, WinGet manifest maintenance and the scope of installation checks.

### build

- Include the submitted 0.20.1 WinGet manifests and silent installation test records. Future manifests must be updated separately after the official EXE is published.

## 0.20.1 · 2026-10-04

### fix

- Unify default solid backgrounds to `#202020` for dark themes and `#F3F3F3` for light themes at 85% opacity. Initial selection, material previews and reset share the same defaults; existing custom colors remain unchanged.
- Point Skill installation prompts directly to the complete packaged directory for the Agent to copy and verify, avoiding PowerShell encoding conversion and truncated JSON output during installation.

### docs

- Add progressively disclosed workflows for grouping icons, fitting and snapping panes, sorting, tabs, folders, search, settings and startup, including discovery, preview, apply and verification steps.

### build

- Validate versions, complete Skill files and hashes in standalone EXE/MSI packaging; add checks for missing references, MSI installation paths and release assets.
- Keep CI output limited to EXE, regular ZIP, portable ZIP and their SHA256 files. MSI remains available through local packaging, with manifest checks that do not install anything.

## 0.20.0 · 2026-10-04

### feat

- Split the Skill into a compact entrypoint and task-specific references; export the complete multi-file bundle offline and include references in installation prompts and release packages.
- Add structured next-step hints for Agents: apply arguments after preview, receipt queries for pending or uncertain submissions, and state inspection for conflicts or unknown results. JSON help now describes the output contract.
- Improve help for users and Agents with command descriptions, relevant preview examples, value ranges and required fields, plus structured behavior flags, field metadata and example argument arrays.
- Add a CLI control switch in Settings, enabled by default. Disabling it rejects online queries and mutations while offline help, schema discovery and Skill export remain available.
- Add a copyable Skill installation prompt with the CLI path matching the running app and installation steps for an Agent.

### fix

- Reject missing CLI option values before they consume `--dry-run` or other options and inadvertently apply a change.
- Unify recovery output for direct and shortcut plan submissions, retaining generated request IDs and complete replay arguments including the data directory.
- Release failed graphics resources and automatically retry up to three times for desktop panes, folder panes, search and Settings, instead of exiting the entire app or repeatedly using invalid resources.
- Fix search submitting an old frame after rendering was deferred, which could cancel a pending redraw.

### perf

- Reuse base brushes in desktop and folder panes, search and Settings; update colors and rebuild resources when the graphics context changes. Cache the Settings app icon texture and release it when leaving the relevant page.
- Skip selection measurements for items that are neither selected nor hovered; avoid resizing unchanged title layouts and skip emoji analysis for plain text.
- Reduce repeated sorting, temporary allocations and monitor lookups in pane sorting and layout queries while preserving no-op write avoidance.

### changed

- Consolidate diagnostics in the independent library and unify application, lower-level module and explicitly enabled rendering trace formats. ERROR remains the default; normal interactions do not add continuous log writes.
- Group pane module entry points by responsibility, separate folder and rendering tests, and expand CLI/Skill discovery, installation, capability lookup and error recovery guidance.

## 0.19.1 · 2026-10-04

### changed

- Use an explicit EXE installer payload, omitting general guides, changelogs and maintenance scripts while retaining runtime components, CLI/Agent files, the license and build metadata.

### docs

- Update the Chinese and English feature illustrations with AI Agent integration, CLI + Skill, and the inspect, plan, preview and apply workflow.

### ci

- Validate the EXE installer file list and installation paths; reject wildcard inclusion, extra documentation and missing CLI/Agent files.

<a id="v0190"></a>

## 0.19.0 · 2026-10-04

### feat

- Add offline CLI help for the overview, resource groups and individual commands, with `-h`, `--help` and JSON output. Generate mutation field descriptions from the protocol and add actionable error hints.
- Add `luciddesk-cli` and an Agent skill to query the running app, preview organization plans and apply them. Support JSON output, offline schema and skill discovery, request receipts and timeout recovery. Packaging includes the CLI, schema and skill files.
- Add content fitting, right-side column arrangement and relative snapping commands. Choose icon columns, a target panel, side and alignment; snapping retains the fixed 5-physical-pixel gap.
- Add folder panel fitting and explicit refresh. List view preserves its width, supports a visible-row limit and checks folder snapshot readiness.
- Add ascending and descending name sorting for desktop panels through the CLI and unlocked panel context menus. Reapplying the same order does not write to the database.

### fix

- Fix an application exit caused by model borrow reentry when sorting from a desktop panel menu.
- Fix state borrow reentry during failed search-enable saves, settings error dialogs and font editor destruction.

### changed

- Separate settings windows, panel events, native menus, input handling and task scheduling by responsibility.
- Separate content measurement, fitting, snapping and arrangement algorithms behind a read-only layout context, preserving persistence and rollback in the control layer.
- Expand CLI/Agent documentation, module boundaries and native window test isolation notes. Real-machine acceptance for multiple monitors, another Windows login session and an installed MSIX remains pending.

### ci

- Add CLI command and control protocol tests, with layout plan and task scheduling reentry tests isolated in separate processes. Check GUI/CLI version and lockfile consistency in the metadata job.

## 0.18.1 · 2026-10-03

### changed

- Improve 16–64 px icons for File Explorer and property dialogs by aligning the three panels and their gaps to whole pixels. Larger icon frames remain unchanged.

- Raise the minimum Rust version to 1.99 and enable raw-pointer borrow checks for FFI. Simplify native interface arguments and string conversion in diagnostic tools.
- Separate material runtime, brush fallback and window visual-tree code while preserving palettes, strength settings, system policy and fallback behavior.
- Extract shared native menu themes and frames into `luciddesk-menu`, replacing implicit cross-crate source references in production code with explicit dependencies. Separate monitor-layout storage from backup recovery and clarify Shell entry and tunable-material type names.
- Rename `desktop-hook` to `luciddesk-explorer` and its DLL to `luciddesk_explorer.dll`, updating loading and packaging references. Upgrade checks still detect the old DLL when loaded. Replace the complete app package when upgrading to keep the EXE and DLL from the same build.

### perf

- Declare Windows API features per crate to reduce modules included in individual builds. Reuse unchanged application resources instead of recompiling icons and version resources after documentation edits.
- Generate only line-table debug information for CI builds and tests to reduce build artifact size. Include build-configuration environment variables in cache keys.

## 0.18.0 · 2026-10-03

### feat

- Support EXE and MSI installers. Releases include EXE, standard ZIP and portable ZIP by default; MSI can be built from source when needed. Uninstall the previous installer edition and keep settings before switching formats.

### changed

- Adjust app icon panel spacing and proportions to form an overall square silhouette while preserving the gradient colors.
- Upgrade build tools to Rust 1.99.0 and Inno Setup 7.1.0 x64, and update CI components.

### perf

- Use fast compression for EXE installers by default, with higher compression levels available. Reuse MSI installation-check components and compression caches to reduce repeated builds.
- Build the app once for standard ZIP, portable ZIP and installers. Cache build dependencies and the Inno compiler in CI, and run language-resource and release-note checks in parallel.

## 0.17.0 · 2026-10-03

### feat

- Add rubber-band selection from empty space in regular panes. Ctrl toggles selection, Shift adds to it, and Esc cancels the current selection gesture.
- Support color emoji in panel and tab titles, aligning emoji and text by their visible glyph centers.
- Add a Color icon rendering switch under Text & icons in settings, enabled by default. Turning it off renders title emoji in monochrome. Changes apply immediately and persist.

### fix

- Use PNG frames at every app icon size to avoid gray edges from small DIB transparency differences and extra alpha premultiplication. Notify Shell to refresh icons after installation.
- Fix a settings-window crash when reentrant painting reads shortcut or language preferences during desktop synchronization.
- Fix files in regular group panes failing to drag into an open File Explorer folder window.
- Preserve pane icon sizes, file names, and multi-selection layouts when dragging files into File Explorer instead of switching to the default large drag preview.

### test

- Simplify duplicate tests and shared fixtures while retaining key regression scenarios. Update settings layout and slider interaction tests, and cover reentrant settings-window painting.

### ci

- Add selection rules, drag image alpha conversion, WARP color emoji and Canvas offscreen rendering, and workspace documentation example tests.

## 0.16.2 · 2026-10-01

### fix

- Fix icons dragged into a pane remaining as placeholders with their desktop originals still visible during background refresh. Resume pending icon loads when the refresh finishes, even if its pixels have not changed.

### perf

- Clear previous loading failures after a successful icon refresh to prevent repeated retry wakeups.
- Suspend ineffective retry timers while a load or refresh is running, resume processing on completion, and respect the icon scan interval when scheduling retries.

## 0.16.1 · 2026-10-01

### fix

- Fix collected icons briefly reappearing on the native desktop after login startup: check membership before painting even without change notifications, and observe parent list notifications.
- Unify redraw protection across filtering entry points to block Shell-reentrant painting while preserving Peek and rename transaction boundaries.

### changed

- Standardize resource names, internal communication identifiers and environment variables on LucidDesk. The default data directory is `%LOCALAPPDATA%\LucidDesk`, with `LUCIDDESK_DATA_DIR` as the only override.
- Remove compatibility with previous-brand data directories, environment variables and shortcut names. Old settings are not migrated automatically. Exit the old app before upgrading; users of the previous-brand data directory should export a backup and restore it in the new version.

## 0.16.0 · 2026-10-01

### feat

- Add a startup toggle in Settings → General and detect when Windows disables startup.
- Add MSIX packaging and desktop component caching; MSIX startup uses StartupTask and preserves its state across updates.

### fix

- Remove this installation's startup entry during standard uninstall and let the installer manage Start menu shortcuts.
- Improve slider and scrollbar colors and contrast in light settings.

### docs

- Add bilingual contribution guidelines, privacy policies, and usage, installation and development documentation.

## 0.15.1 · 2026-09-30

### fix

- Use a consistent Windows application identity for the app and its Start menu and desktop shortcuts, independent of version and installation path. ([ca1e551](https://github.com/Yuch3nE/luciddesk/commit/ca1e551c87a848e1135c428b081ef40881bb9b6b))
- Remove renamed, copied and legacy-name shortcuts targeting this installation during uninstall, preserving links to other installations and applications. ([ca1e551](https://github.com/Yuch3nE/luciddesk/commit/ca1e551c87a848e1135c428b081ef40881bb9b6b))
- Notify Windows to refresh shortcuts and the application list after uninstall. ([ca1e551](https://github.com/Yuch3nE/luciddesk/commit/ca1e551c87a848e1135c428b081ef40881bb9b6b))

## 0.15.0 · 2026-09-30

### feat

- Add an installer with per-user installation or all-users installation in Program Files.
- Add Check for updates and Open update page in About; release packages are downloaded manually.
- Offer graceful app exit during installation and uninstall; keep user settings by default with a checkbox in the uninstall confirmation.

### fix

- Release the desktop DLL on exit so upgrades and uninstall can proceed.
- Check for old components in Explorer before installation and attachment to prevent crashes from mixed DLL builds.
- Correct uninstall messages and wait for the app and desktop component to exit before continuing.

### docs

- Simplify both READMEs and organize screenshots, package choices, upgrades and build instructions.

### ci

- Publish the installer, standard ZIP, portable ZIP and SHA256 checksums from tag builds.

## 0.14.0 · 2026-09-30

### feat

- Use the current folder's classic Explorer background menu, including native commands and extensions.

### fix

- Follow the app language for folder type labels.
- Avoid reloading panes when dismissing file menus.

### docs

- Complete screenshots in both READMEs and show the illustration expanded and centered.

### ci

- Build and publish only on tag pushes; the `v` prefix is optional for version tags.

## 0.13.0 · 2026-09-30

- Add tray entries for file search, refreshing all panels and opening the configuration folder, with clearer menu grouping.
- Add a Windows 11 context-menu toggle for regular panels, enabled by default; apply system light/dark styling and DPI-aware spacing to classic fallback menus.
- Reorganize refresh, rename, view and creation commands in panel menus, and show collapse or expand according to the current state.
- Skip automatic backups when content is unchanged, prioritize manual actions and backup history, and unify back buttons on settings subpages.
- Introduce the green three-panel icon with 15 ICO sizes and an updated About icon; verify embedded resources during packaging and add actual screenshots to both READMEs.
- Remove startup DLL copying and load `luciddesk_desktop.dll` directly beside the executable. Extract the complete new package when upgrading to avoid mixing DLL versions.
- Adopt the MIT License with attribution to Yuchen95, and synchronize license information in About and release packages.
- Fix language changes incorrectly closing panels, and require an explicit close request before starting the close animation.
- Refresh language-dependent text and fonts in place, preserving panel items, selection, scroll positions and font search text without explicitly cancelling renames.
- Stop the font-loading timer when cancelling a previous language's task; retry language-dependent settings renderer refreshes and clear interaction state tied to the old layout.
- Forward the file menu wrapper's site interface to address the null host used while enumerating Open with commands, which caused Explorer crashes. Live compact-menu regression testing remains pending.

## 0.12.0 · 2026-09-30

- Add font-name search with multiple keywords, full-width Latin letters and digits, and common separators. Replace candidate previews with a continuously scrolling list that marks the current and default fonts.
- Enumerate fonts and check language coverage in the background on the first visit to the font page, keeping settings interactive. Reuse the list within the open settings window, release it on close, and cancel unfinished loading without a process-wide candidate cache.
- Improve the font search field's light and dark colors, placeholder contrast, focus feedback, and text cursor. Support a clear button and Escape; keep the current font and search field fixed above the scrolling list.
- Align settings control spacing and selection states, reduce temporary font enumeration resources, and validate only the selected family when saving instead of rescanning every candidate.
- Fix font handle cleanup when closing settings and release the text measurement cache, including its allocated capacity, with the settings renderer.

## 0.11.2 · 2026-09-30

- Simplify the Change folder menu wording and save the updated panel title when switching folders; reselecting the mapped root now leaves the current subfolder.
- Refine folder column sorting: modification time defaults to newest first and size to largest first, with name ordering for ties and unknown values last within their group.
- Reduce path allocations, unnecessary type sorting and temporary sort memory during folder icon updates, and improve selection matching when refreshing large selections.
- Reload previously loaded search pages before replacing results, restoring selection, focus and the range-selection anchor by path; prevent false double-clicks when results change.

## 0.11.1 · 2026-09-30

- Improve pending icons in desktop and folder panels with scalable file and folder outlines, aligned with loaded icons and adapted to light and dark themes.
- Keep placeholders independent of icon fonts and free of continuous animations; retain recognizable outlines if icon extraction fails.

## 0.11.0 · 2026-09-30

- Add an optional Show all panels global shortcut, disabled by default with `Ctrl + Shift + D` as the preset, with the same behavior as the tray action, customizable keys and conflict reporting.
- Switch interface languages immediately and follow changes to the effective Windows display language in system mode.
- Preview fonts in the current language, show the default fallback font, and refresh candidates when the language changes. Unify translucent accent states across settings materials and light/dark themes.
- Update About to use the public project website and add links to downloads, the changelog and issue reporting.
- Use “panel” consistently throughout the interface and both READMEs, preserving user-defined names.

## 0.10.6 · 2026-09-30

- Add Simplified Chinese, Traditional Chinese, English, Japanese, Korean, German and Russian interfaces. Follow the system language or choose one manually; changes take effect after restarting.
- Adapt default fonts to the selected language and improve the settings sidebar, descriptions and option layouts for longer text.
- Add an English README, bilingual feature illustrations and privacy policy, and bilingual changelogs.
- Fix compilation of the icon diagnostic example. CI now produces standard and portable ZIPs with SHA256 checksums and generates Release notes from the matching Chinese and English changelog sections.

## 0.10.5 · 2026-09-29

- Fix delays, failed drops and temporarily missing icons when moving batches of items into or out of desktop groups; reduce latency while waiting for desktop synchronization.
- Fix other panels unexpectedly covering the current panel when opening or closing menus, and reduce unnecessary stacking changes and flicker on clicks.
- Keep the search input at the same stacking level and topmost state as its panel. Clicking raises it only among desktop panels, and the search shortcut preserves desktop stacking.
- Improve background fallback when Windows disables Acrylic, Mica or Mica Alt, and improve sidebar selection and hover contrast in light and dark themes across all four materials.
- Improve panel and menu corners, icon font fallback and icon alignment on Windows 10; hide unsupported Mica and Mica Alt options.
- Fix a DXGI crash while releasing graphics caches during shutdown, and improve rendering logs and crash dump collection in optional diagnostic packages.

Windows 11 x64 is the primary maintenance platform. Windows 10 has been tested on a user's device; see the [validation notes](docs/development/validation.md) (Chinese) for coverage.

## 0.10.4 · 2026-09-29

- Update the underlying configuration reading and writing components.
- Reorganize the README and changelog to make startup, upgrades, backups and features easier to find.

## 0.10.3 · 2026-09-28

- Reduce duplicate memory use when multiple desktop groups and folder panels display the same icons.
- Improve icon cache memory management while preserving icon updates and refresh behavior.

Actual memory savings depend on the number of repeated icons.

## 0.10.2 · 2026-09-28

- Rename the tray entry to “Show panels” to make panels hidden behind other windows easier to find.
- Simplify the About page, bringing together the product version, developer, system information and desktop connection status.
- Improve executable file properties and portable package documentation.

## 0.10.1 · 2026-09-28

- Rename the product to **LucidDesk**, retaining compatibility with the previous data directory.
- Add portable mode to keep configuration and layouts beside the application.
- Raise all panels with a tray click without changing their topmost settings.
- Default the first group to 3 columns × 4 rows in the upper-right corner of the primary work area; existing layouts are unchanged.
- Use “New group” consistently for new groups and group tabs.

## 0.10.0 · 2026-09-28

- Add desktop group tabs with switching, reordering, renaming, closing and keyboard shortcuts.
- Support detaching tabs into separate panels and dragging panels together to merge tabs; folder and search panels remain independent.
- Add global font and title separator settings, and unify text, borders and menus across materials.
- Improve search input and results with clearing, full-path hints, result counts and error retries.
- Let folder panels open subfolders inside the panel or in File Explorer.
- Disable search and preview by default until enabled in Settings, while preserving existing explicit choices.
- Improve stacking, dragging and recovery after desktop disconnection; fix brief window flashes.
- Reduce repeated background checks and icon loading to improve idle resource use.
- Fall back from unavailable Mica effects to Acrylic or solid backgrounds while preserving the material selection.

## 0.9.1 · 2026-09-14

- Improve backup management, default folder columns and settings interactions.
- Scale icons, text, rename boxes and drag previews together; adjust grid scaling to 50%–200%.
- Reduce repeated memory allocations during folder browsing, search and drawing.
- Improve folder refresh, desktop dragging and layout saving.

## 0.9.0 · 2026-09-14

- Add a list view for desktop groups and adjustable icon grid sizes.
- Add a size column to folder lists with resizable, automatically saved column widths.
- Improve sorting by name, type and size, with natural ordering for numbers in file names.
- Add Back and Home buttons and fix missing folder contents in some cases.
- Prioritize thumbnails for visible files and reduce repeated waits when returning to visited folders.
- Adapt file menus to the system light or dark theme and add “Open file location”.

## 0.8.1 · 2026-09-13

- Fix empty desktop groups remaining on “Reading desktop items…”.
- Expand documentation for the three panel types, shortcuts, appearance and backups.

## 0.8.0 · 2026-09-13

- Store global settings separately from panel layouts; back up and restore both.
- Add the app icon to the About page and simplify version information.

Starting with this version, the old experimental `hook-desktop.db` database is no longer read. Automatic migration from that format is not provided.

## 0.7.1 · 2026-09-13

- Reduce repeated idle checks and folder refreshes to lower background resource use.
- Make panel borders respond to background opacity and material strength.
- Adjust the About page layout and fix clipped settings content.

## 0.7.0 · 2026-09-13

- Adapt panel text to the background automatically, with manual light or dark text options.
- Add an independent text backdrop protection option, disabled by default.
- Support fractional corner radii and fine adjustments with arrow keys.
- Adjust dark borders and settings page spacing.

## 0.6.1 · 2026-09-13

- Unify application, window and tray icons across display scaling levels.
- Add rounded corners, icons and app theme support to tray menus.
- Fix a possible crash when clicking the tray repeatedly.

## 0.6.0 · 2026-09-13

- Show the system version and allow copying diagnostic information on the About page.
- Restrict enabling search or preview when the required program is not detected.
- Improve panel collapsing, settings toggles and menu animations; fix occasional settings window flashes.
- Improve folder sorting and settings saving.

## 0.5.0 · 2026-09-13

- Allow choosing PowerToys Peek or QuickLook for file preview, with separate paths and shortcuts.
- Support QuickLook previews for files, folders and system items such as the Recycle Bin.
- Add search panel width adjustment, saved dimensions and edge snapping.
- Improve search result text, icons and context menu layouts.
- Simplify settings pages and allow resetting panel layout defaults.

## 0.4.2 · 2026-09-13

- Reorganize settings categories to make appearance, search, preview and backup options easier to find.
- Remove repeated explanations and clarify shortcut scope and error states.

## 0.4.1 · 2026-09-13

- Fix multi-selection drag previews showing only the item under the pointer; previews now show all selected items.
- Preserve relative positions of selected items and improve drag visuals across display scaling levels.

## 0.4.0 · 2026-09-13

- Add a color page with presets, RGB adjustments and live previews.
- Allow adjusting Acrylic and Mica strength and remember their settings independently.
- Use a fixed Mica Alt effect without a separate strength control.
- Configure panel color and opacity independently while retaining the default theme background in Settings.

## 0.3.0 · 2026-09-12

- Add solid backgrounds with custom colors, HEX input and 0–100% opacity.
- Keep text and icons clear while adjusting background opacity.
- Add live slider previews, keyboard fine-tuning and percentage input; remember the previous color when switching materials.

## 0.2.2 · 2026-09-12

- Restore the settings window fade-in animation and respect the Windows animation setting.
- Reduce flashes when opening windows.

## 0.2.1 · 2026-09-12

- Fix settings content and its background appearing out of sync.
- Improve rendering during material changes and window resizing.
- Improve the About page version details, feature introduction and project homepage link.

## 0.2.0 · Feature baseline, not released separately

- Desktop groups, tray menus, file renaming and keyboard multi-selection.
- Folder panels, directory navigation, list sorting and Everything search.
- File preview, configuration backups, monitor layout persistence and desktop connection recovery.
- Panel moving, resizing and edge snapping.
