# LucidDesk Privacy Policy

[简体中文](PRIVACY.md) · English

Last updated: October 20, 2026. Maintainer: Yuchen95.

This policy describes data handling by the current official installer, standard ZIP and portable editions of LucidDesk. Third-party modified builds may behave differently.

## Local data

LucidDesk does not require an account. It currently includes no advertising, usage analytics or automatic crash-report uploads. Desktop organization, folder browsing and layout storage primarily take place on your computer.

To provide its features, the application reads or stores:

| Information | Purpose |
| --- | --- |
| Language, appearance, shortcuts and external tool paths | Save preferences |
| Panel names, positions, layouts, folder paths, desktop item identities and path references | Restore the workspace |
| File names, properties, icons and content needed for previews | Browse, search and display files |
| Windows, application and desktop connection diagnostics | Troubleshooting; you choose whether to copy or share them |
| Configuration and layout backups | Restore settings and layouts; referenced original files are not included |

Settings and workspace data normally reside in `%LOCALAPPDATA%\LucidDesk`. Portable builds use `data` next to the program; environment variables can select a different data directory. See the [storage guide](docs/development/storage.md). These files and backups may contain personal file paths. LucidDesk does not automatically upload them to the maintainer.

Organizing desktop panels stores references without moving original files. Explicit file operations such as delete, rename, cut, paste and folder operations affect actual files.

Enabling login startup in the standard installer or portable edition registers the executable path and startup arguments under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`. Disabling it removes the registration belonging to the current executable. The app only reads Windows `StartupApproved\Run` status and does not change Windows disable records. These details stay on the device. Uninstall removes the current installation's registration for the current user; disable startup before manually deleting or moving a portable copy.

The MSIX edition uses Windows StartupTask to manage login startup for its package, identified by both package identity and the regular `msix` marker beside the executable. State queries and changes stay on the device, do not create an unpackaged Run registration, and are not uploaded. Windows manages the package startup registration and its removal during uninstall.

## Network access and third-party features

Automatic update checks are enabled by default and can be disabled in **Settings → About**. While enabled, LucidDesk checks at startup and every 24 hours. You can also click **Check for updates** manually. These checks request release metadata from the GitHub Releases API over HTTPS. Its User-Agent includes the application name and version. GitHub and network services may process your IP address, request time and request information. The request does not transmit your workspace, configuration, search terms or personal file contents. Version comparison is local; LucidDesk does not automatically download installers.

Opening the update page or another external link uses your browser. The destination website and browser settings govern that visit. See the [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

If you enable Everything search or Peek/QuickLook previews, LucidDesk passes search terms or file paths to the selected local tool, which processes them under its own settings. Windows Shell, third-party menu extensions, network folders and cloud-synced files may also involve their respective services. This policy does not replace their policies. Installation and updates through Microsoft Store are subject to the [Microsoft Privacy Statement](https://privacy.microsoft.com/privacystatement).

## Diagnostics and information you share

Diagnostics and debugging information are handled locally. Diagnostic builds or tracing options may create local logs; LucidDesk does not automatically send them to the maintainer. Diagnostics, screenshots, logs or settings that you post to GitHub issues may be public. Remove usernames, personal paths, sensitive file names and other information you do not want to disclose before sharing.

## Retention and deletion

You control local settings and workspace data. Automatic backups retain the latest 10 snapshots; manually exported copies remain under your control. After closing LucidDesk, you may delete its data directory to remove local settings. Back up anything you want to keep first.

EXE uninstall keeps settings by default; unchecking **Keep user settings** removes the current account's default data directory. MSI uninstall always preserves user settings. To remove them, delete `%LOCALAPPDATA%\LucidDesk` manually. Custom directories, portable data, other accounts' data and manually exported backups require separate removal. Windows application history, caches and backup records, and data held by third-party services, are not collectively removed by the LucidDesk uninstaller.

## Contact and changes

Contact the maintainer through [project issues](https://github.com/Yuch3nE/luciddesk/issues) with questions about this policy. Do not include sensitive personal information in public discussions. This policy is updated to reflect application changes; the latest text and revision history are maintained in this repository.
