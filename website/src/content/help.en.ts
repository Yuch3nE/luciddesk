import type { HelpPage } from './help';
export const helpPagesEn: HelpPage[] = [
  {
    "slug": "installation",
    "label": "Installation & updates",
    "title": "Set up your desktop.",
    "description": "Choose a package, install LucidDesk, and keep your layout when you update or move to another computer.",
    "sections": [
      {
        "id": "choose",
        "title": "Choose a package",
        "html": "<p>Downloads are available for Windows 10 / 11 x64. Windows 11 x64 is the primary tested and maintained platform.</p><ul><li><strong>Installer:</strong> choose the file ending in <code>setup.exe</code>. It includes Start menu and uninstall entries and preserves settings when you update. Installing for all users requires administrator permission.</li><li><strong>Portable:</strong> extract the <code>portable.zip</code> package to a writable folder. Settings are stored in the <code>data</code> folder beside the app by default.</li><li><strong>Standard ZIP:</strong> runs without installation, but stores settings in your local app data folder rather than beside the executable.</li></ul><p>Release packages are hosted on GitHub. All installation and usage instructions are available here on this website.</p>"
      },
      {
        "id": "start",
        "title": "Start for the first time",
        "html": "<ol><li>Run the installer, or fully extract the ZIP. Do not run the app from inside the archive.</li><li>Keep <code>luciddesk.exe</code> and <code>luciddesk_explorer.dll</code> from the same release together. Portable packages also need the extensionless <code>portable</code> marker.</li><li>If an older instance is running, exit it from the system tray first.</li><li>Find LucidDesk in the system tray, choose the command to create a panel, and drag desktop items into it.</li></ol><p>Closing Settings does not exit the app. Use the exit command in the tray menu. Next: <a href=\"/guide/\">learn about panels, search, and previews</a>.</p>"
      },
      {
        "id": "updates",
        "title": "Update without losing your layout",
        "html": "<p>Check your version and updates in Settings → About. Automatic checks are enabled by default at startup and every 24 hours; you can turn them off. The app does not automatically download installers or restart itself.</p><ol><li>Export a backup in Settings → Backup and restore.</li><li>Exit LucidDesk from the tray.</li><li>For an installed copy, run the new installer using the same installation type and location.</li><li>For a portable copy, replace program files with the new portable package. Keep your <code>data</code> folder and <code>portable</code> marker. The executable and DLL must come from the same release.</li><li>Restart and check the version, panels, and active data directory.</li></ol><p>EXE and MSI installations cannot directly upgrade each other. To change formats, uninstall the old copy while keeping its settings, then install the other format.</p>"
      },
      {
        "id": "move",
        "title": "Move to a new folder or computer",
        "html": "<p>Exit the app before copying the entire portable folder. If launch at login is enabled, disable it before the move and enable it again from the new location. Use the tray command to open the configuration directory to confirm where your data is stored; a custom directory may be elsewhere.</p><p>To change installation types, export a backup from the old copy, exit it, then restore the backup in the new copy. Restore replaces the corresponding settings and layout; it does not merge workspaces.</p><p><strong>Backups and portable folders do not include the original files referenced by your panels.</strong> Move those separately and check folder mappings and third-party tool paths if drive letters or locations change.</p>"
      },
      {
        "id": "uninstall",
        "title": "Uninstall",
        "html": "<p>Use Windows Settings → Apps for installed copies. The EXE uninstaller keeps settings by default; clearing the keep-settings option removes the current account’s default data directory. MSI uninstall keeps user settings.</p><p>For portable copies, disable launch at login and exit before deleting the folder. Deleting its <code>data</code> folder also removes your settings and layout. See <a href=\"/privacy/#delete\">data retention and deletion</a>.</p>"
      }
    ]
  },
  {
    "slug": "guide",
    "label": "User guide",
    "title": "A desktop that works your way.",
    "description": "Create your first panel, find files, preview documents, and back up your layout. This guide covers version 0.22.2.",
    "sections": [
      {
        "id": "first-panel",
        "title": "Create your first panel",
        "html": "<ol><li>Start LucidDesk and find its icon in the Windows system tray. Expand hidden icons if necessary.</li><li>Choose the tray command to create a new panel.</li><li>Drag desktop items into it. Try one panel for work and another for personal projects.</li><li>Drag the title area to move a panel and its edges to resize it. Drag items back to the desktop to remove them from the panel.</li></ol><p><strong>Organizing or closing a desktop panel only changes references; it does not move or delete the original files.</strong> File commands such as delete, rename, and cut do affect the actual files.</p><p>Not installed yet? See <a href=\"/installation/\">installation and updates</a>.</p>"
      },
      {
        "id": "panels",
        "title": "Pick the right panel",
        "html": "<ul><li><strong>Desktop panels:</strong> group references to desktop files, folders, and shortcuts.</li><li><strong>Folder panels:</strong> choose a directory to browse its contents directly. Changes refresh automatically. Closing the panel only removes the mapping.</li><li><strong>Search panels:</strong> connect to a separately installed copy of Everything to find files on your computer.</li></ul><p>By default, double-clicking a folder in a folder panel opens it inside the panel. Use <kbd>Alt</kbd> + <kbd>←</kbd> to go back. Settings → Folder panels lets you open folders in File Explorer instead.</p><p>Dragging files into a folder panel copies them to that directory. Delete, cut, rename, and paste act on real files.</p>"
      },
      {
        "id": "organize",
        "title": "Arrange panels and tabs",
        "html": "<p>The title-bar ellipsis and background context menu provide view, sorting, locking, and tab controls. Locking prevents moving or renaming a panel, but you can still resize it.</p><ul><li><strong>Icon or list view:</strong> each panel remembers its own choice.</li><li><strong>Auto-arrange:</strong> desktop panels pack icons together. Disable it to keep empty spaces; disable grid alignment for free placement.</li><li><strong>Tabs:</strong> create tabs in desktop panels, or drag one desktop panel onto another’s title bar and release after the merge preview appears.</li><li><strong>Tab shortcuts:</strong> <kbd>Ctrl</kbd> + <kbd>T</kbd> creates a tab, <kbd>Ctrl</kbd> + <kbd>Tab</kbd> switches tabs, and <kbd>Ctrl</kbd> + <kbd>W</kbd> closes the current tab.</li></ul><p>Folder and search panels cannot merge into tabs. Closing tabs does not delete original files. Closing the whole panel closes all its tabs.</p>"
      },
      {
        "id": "search",
        "title": "Enable file search",
        "html": "<ol><li>Install and start Everything 1.4 or later separately.</li><li>Enable search panels in LucidDesk’s Everything search settings. Select <code>Everything.exe</code> if it is not found automatically.</li><li>Enter a filename. If the connection fails, check that Everything is running and press <kbd>F5</kbd> to retry.</li></ol><p>Everything query syntax is supported: try <code>ext:pdf</code> or <code>folder:</code>. Use <kbd>Ctrl</kbd> + <kbd>L</kbd> to focus the query, <kbd>↓</kbd> to enter results, <kbd>Enter</kbd> to open an item, and <kbd>Ctrl</kbd> + <kbd>Enter</kbd> to reveal it in File Explorer.</p><p>Deleting a result deletes the real file. Closing the search panel does not delete files. Query text is not kept for the next launch.</p>"
      },
      {
        "id": "preview",
        "title": "Preview with the space bar",
        "html": "<p>Install PowerToys Peek or QuickLook separately, then enable file previews in LucidDesk settings and choose the tool. Search and previews are off by default; neither tool is bundled.</p><p>Select a file and press the preview shortcut, which defaults to Space. If nothing opens, check the tool, the integration setting, and your selection. While renaming a file, Space still types a space.</p>"
      },
      {
        "id": "shortcuts",
        "title": "Everyday shortcuts",
        "html": "<ul><li><kbd>Enter</kbd>: open the selected item.</li><li><kbd>F2</kbd>: rename; <kbd>Enter</kbd> confirms and <kbd>Esc</kbd> cancels.</li><li><kbd>Ctrl</kbd> + <kbd>A</kbd>: select all items in the panel.</li><li><kbd>Ctrl</kbd> + click: toggle selection; <kbd>Shift</kbd> + click: select a range.</li><li><kbd>Ctrl</kbd> + <kbd>C</kbd> / <kbd>X</kbd>: copy / cut selected files.</li></ul><p>Shortcuts act on the focused panel. Click a panel or item first.</p><p>Enable double-click to hide the desktop in Settings → General → Desktop actions. Double-click an empty desktop area to hide it, and again to restore it. You can also show panels from the tray menu.</p>"
      },
      {
        "id": "backup",
        "title": "Back up settings and layouts",
        "html": "<p>Export or restore a backup in Settings → Backup and restore. Restore asks for confirmation and creates a pre-restore backup before replacing the corresponding state.</p><p>Backups include settings, layout, item references, and folder mappings, <strong>not the contents of your files</strong>. Automatic backups default to a five-minute interval, keep the latest 10 copies, and are created only after settings or layout changes. These settings are adjustable.</p><p>Standard copies use <code>%LOCALAPPDATA%\\LucidDesk</code> by default; portable copies use <code>data</code> beside the program. Open the configuration directory from the tray to confirm. See <a href=\"/privacy/\">data and privacy</a>.</p>"
      },
      {
        "id": "troubleshooting",
        "title": "If something is not working",
        "html": "<ul><li><strong>Missing panels:</strong> find the tray icon and use its show-panels command.</li><li><strong>Missing desktop DLL:</strong> extract the full package and keep the executable and DLL from the same release.</li><li><strong>Old version still running:</strong> exit the old tray instance before starting the new one.</li><li><strong>Desktop connection issue:</strong> check that Explorer is running, read the status in Settings → About, and try reconnecting. Avoid multiple apps taking over the desktop at once.</li><li><strong>Configuration or database error:</strong> keep the data and error details. Check the version before deleting or replacing anything.</li></ul><p>You can <a href=\"https://github.com/Yuch3nE/luciddesk/issues\">report an issue on GitHub</a>. Remove personal paths and sensitive filenames from logs or screenshots before sharing.</p>"
      }
    ]
  },
  {
    "slug": "agent",
    "label": "Agent integration",
    "title": "Describe your workspace. Let your agent help.",
    "description": "Connect an external agent to inspect your desktop, preview a plan, and apply the changes you approve.",
    "sections": [
      {
        "id": "overview",
        "title": "What your agent can do",
        "html": "<p>The included CLI lets an agent inspect your workspace and help change panel positions, sizes, item assignments, and arrangement. Ask for work files on the left and everyday tools on the right, then review the proposed changes.</p><p>This requires an external agent. LucidDesk does not include an AI chat service, and enabling CLI control does not automatically reorganize your desktop.</p>"
      },
      {
        "id": "connect",
        "title": "Connect your agent",
        "html": "<ol><li>Install or update LucidDesk and start the main app. Use the CLI and skill included in the same release.</li><li>Open Settings → Agent and CLI and enable CLI control.</li><li>Copy the skill-installation prompt and paste it into your agent. It contains the location of the bundled skill on your computer.</li><li>Ask the agent to install the complete <code>luciddesk-control</code> skill directory and check the connection.</li></ol><p>The copy button only copies a prompt. It does not install the skill or change your workspace. The installer does not configure external agents for you.</p>"
      },
      {
        "id": "request",
        "title": "Make your request specific",
        "html": "<blockquote>Inspect my desktop first. Put work files in a panel on the left and everyday tools on the right. Do not delete or move the original files. Preview the plan and wait for my approval before applying it.</blockquote><p>Say which panels and positions should stay, and whether the agent may create or merge panels. Review affected items, panels, and positions before approval, then check the actual desktop afterward.</p>"
      },
      {
        "id": "control",
        "title": "Keep changes reviewable",
        "html": "<ul><li><strong>Read first:</strong> build plans from the current workspace, not old panel identifiers.</li><li><strong>Preview first:</strong> a preview is not an applied change. Review it before proceeding.</li><li><strong>Refresh after conflicts:</strong> manual changes can invalidate a plan. Read the workspace and preview again.</li><li><strong>Verify uncertain results:</strong> after a timeout or disconnection, check status and receipts before retrying.</li></ul><p>Disabling CLI control rejects new online queries, previews, and submissions. It does not undo completed changes. Re-enable it in app settings to reconnect.</p>"
      },
      {
        "id": "verify",
        "title": "Check the connection yourself",
        "html": "<p>Open PowerShell in the installation directory. These commands read state without changing the workspace:</p><pre><code>.\\luciddesk-cli.exe status --json\n.\\luciddesk-cli.exe workspace get --json</code></pre><p>Read command help locally:</p><pre><code>.\\luciddesk-cli.exe help\n.\\luciddesk-cli.exe help pane</code></pre><p>The CLI does not start the main app. If it cannot connect, check that LucidDesk is running, versions match, and CLI control is enabled. After upgrading or moving the app, point the agent to the new CLI and skill directory.</p>"
      },
      {
        "id": "privacy",
        "title": "Your external agent and your data",
        "html": "<p>Reading a workspace can expose panel names, filenames, and paths to your agent. Whether that information leaves your computer depends on the agent and its settings. Review its data handling before use.</p><p>See the <a href=\"/privacy/\">privacy guide</a> for LucidDesk’s local storage, update checks, and third-party tools.</p>"
      }
    ]
  },
  {
    "slug": "privacy",
    "label": "Privacy",
    "title": "Understand how your desktop data is used.",
    "description": "A reader-friendly summary of the app privacy policy dated October 20, 2026. Maintainer: Yuchen95.",
    "sections": [
      {
        "id": "scope",
        "title": "Scope",
        "html": "<p>This guide covers official LucidDesk installer, standard ZIP, and portable releases. Modified third-party versions may handle data differently. No account is required. The app currently has no built-in advertising, usage analytics, or automatic crash-report uploads.</p><p>Desktop organization, folder browsing, and layout storage primarily happen on your computer. This guide explains the application policy; it does not change how the app handles data.</p>"
      },
      {
        "id": "local",
        "title": "What is stored locally",
        "html": "<ul><li><strong>Preferences:</strong> language, appearance, shortcuts, and tool paths.</li><li><strong>Workspace:</strong> panel names, positions, layouts, folder paths, and desktop item references.</li><li><strong>File information:</strong> names, attributes, icons, and content needed for browsing, search, or preview.</li><li><strong>Diagnostics:</strong> Windows, app, and desktop connection status to help troubleshoot; you choose whether to share it.</li><li><strong>Backups:</strong> settings and layout snapshots, not the original files.</li></ul><p>The default directory is <code>%LOCALAPPDATA%\\LucidDesk</code>; portable copies use a nearby <code>data</code> folder. An environment variable can override the data directory. Open the configuration directory from the tray to check the actual location. These files can contain personal paths and are not automatically uploaded to the maintainer.</p><p>Organizing desktop references does not move originals. Explicit file commands such as delete, rename, cut, and paste affect real files.</p>"
      },
      {
        "id": "startup",
        "title": "Launch at login",
        "html": "<p>When enabled, standard and portable copies record the app path and launch arguments in the current user’s Windows registry Run key. Disabling the feature removes the entry belonging to the current app. LucidDesk reads Windows’ startup-disabled state without modifying those system records. This information stays local.</p><p>The uninstaller cleans up the current installation’s startup entry for the current user. Disable startup before moving or deleting a portable copy. MSIX packages use Windows StartupTask instead of the standard Run entry; Windows manages their startup and uninstall cleanup.</p>"
      },
      {
        "id": "network",
        "title": "When the app connects to the internet",
        "html": "<p><strong>Update checks:</strong> by default, at startup and every 24 hours, the app contacts the GitHub Releases API over HTTPS. Checks can also be manual. Disable automatic checks in Settings → About.</p><p>The request uses a User-Agent with the app name and version. GitHub and network providers may process your IP address, request time, and request metadata. The app does not send workspace data, settings, search terms, or personal file contents in this request. Versions are compared locally, and installers are not downloaded automatically.</p><p><strong>External links:</strong> downloads and other websites open through your browser and follow those services’ privacy practices. Read the <a href=\"https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement\">GitHub privacy statement</a>.</p>"
      },
      {
        "id": "tools",
        "title": "Search, previews, and external services",
        "html": "<p>Everything, PowerToys Peek, and QuickLook receive queries or file paths locally when you enable them. Their handling follows their own settings. Windows Shell, context-menu extensions, network folders, and cloud-synced files may also involve Windows or other services.</p><p>Review your external agent’s settings separately; see <a href=\"/agent/#privacy\">agent data access</a>. Microsoft Store installation and updates follow the <a href=\"https://privacy.microsoft.com/privacystatement\">Microsoft privacy statement</a>. This guide does not replace third-party policies.</p>"
      },
      {
        "id": "diagnostics",
        "title": "Logs and information you share",
        "html": "<p>The app may write local error logs and keep a limited set of recent diagnostics during a session. Higher log levels or tracing record more detail. Logs may include error locations, stack traces, and personal file paths. They are not sent automatically.</p><p>Issues, screenshots, settings, and logs that you submit may be public. Remove usernames, personal paths, sensitive filenames, and anything else you do not want to share.</p>"
      },
      {
        "id": "delete",
        "title": "Retention, backups, and deletion",
        "html": "<p>You control local data. Automatic backups keep the latest 10 snapshots by default; exported backups remain wherever you saved them. Exit the app before deleting data directories and save anything you want to keep first.</p><ul><li><strong>EXE installer:</strong> uninstall keeps settings by default. Clear the keep-settings option to remove the current account’s default data directory.</li><li><strong>MSI installer:</strong> uninstall retains user settings; remove the default data directory yourself for full cleanup.</li><li><strong>Portable or custom locations:</strong> clean up portable data, custom directories, other users’ data, and exported backups separately.</li></ul><p>The uninstaller does not remove all Windows history, caches, system backups, or data held by third-party services.</p>"
      },
      {
        "id": "website",
        "title": "Website preferences",
        "html": "<p>This website stores your light, dark, or system theme preference in your browser’s local storage as <code>luciddesk-theme</code>. Your language choice is stored as <code>luciddesk-language</code> and switches in place without reloading the page. Clear this site’s data to remove these preferences. The website is hosted on Cloudflare; loading pages and downloading releases sends requests to the relevant hosting services.</p>"
      },
      {
        "id": "contact",
        "title": "Questions and policy updates",
        "html": "<p>Contact the maintainer through <a href=\"https://github.com/Yuch3nE/luciddesk/issues\">GitHub issues</a> with questions. Do not include sensitive personal data in public discussions. This guide will be updated as the app’s behavior changes.</p>"
      }
    ]
  }
];
