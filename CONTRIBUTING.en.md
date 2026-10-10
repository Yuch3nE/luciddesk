# Contributing

[简体中文](CONTRIBUTING.md) · English

Bug reports, feature proposals, translations, and code contributions are welcome. Check existing [issues](https://github.com/Yuch3nE/luciddesk/issues) and [pull requests](https://github.com/Yuch3nE/luciddesk/pulls) before starting. For usage questions, consult the [user guide](docs/usage.md); for code changes, start with the [development documentation](docs/development/README.md). The linked detailed guides are currently in Chinese.

## Reports and proposals

A useful bug report lets another contributor reproduce the problem. Include:

- **Environment:** Windows build number, LucidDesk version, and distribution type: installer, standard ZIP, portable ZIP, or MSIX.
- **Steps:** a minimal reproduction, expected and actual behavior, and whether the issue occurs consistently.
- **Evidence:** complete error messages and relevant screenshots. For visual issues, include monitor layout, DPI, theme, and material.
- **Related conditions:** external programs or Shell extensions involved in search, preview, or menus. For upgrade issues, include both versions and the installation method.

**Settings → About → Copy diagnostics** provides environment details; review them before sharing. For data or crash issues, prefer reproducible test data and relevant logs. Do not upload complete private workspace databases, configuration containing personal paths, unredacted dumps, sensitive screenshots, or credentials. See the [privacy policy](PRIVACY.en.md).

For feature requests, describe the use case, the difficulty with the current workflow, and the desired behavior. Discuss larger features, architecture changes, or compatibility changes in an issue first. Small fixes and documentation improvements can go directly into a PR.

## Prepare the development environment

Use Windows x64, PowerShell 7, rustup, Visual Studio C++ Build Tools, and the Windows SDK. `rust-toolchain.toml` specifies the Rust version; the repository script selects the Windows build environment. See the [build guide](docs/development/build.md) for full requirements.

Clone the repository, then run the build commands from its root:

```powershell
git clone https://github.com/Yuch3nE/luciddesk.git
cd luciddesk
.\tools\use-windows-toolchain.ps1
cargo build -p luciddesk -p luciddesk-explorer --locked
```

After a successful build, exit any running LucidDesk instance and start the development build with a separate data directory:

```powershell
$previousDataDirectory = $env:LUCIDDESK_DATA_DIR
try {
    $env:LUCIDDESK_DATA_DIR = Join-Path $PWD 'target\dev-data'
    Start-Process -FilePath '.\target\debug\luciddesk.exe' -WindowStyle Hidden -Wait
} finally {
    $env:LUCIDDESK_DATA_DIR = $previousDataDirectory
}
```

This command waits for the application to exit before restoring the terminal's previous environment variable. A separate data directory isolates configuration, but not Explorer or real file operations. A second launch may still activate the existing instance; data isolation does not enable independent application instances.

Keep the EXE and `luciddesk_explorer.dll` from the same build in the same directory. Native desktop tests affect the real Explorer session, so use an environment where interruption is acceptable. Check files, layouts, and running instances before testing, then restore any state that needs to be preserved.

## Find the relevant implementation

| Change | Read first |
| --- | --- |
| Module or process coordination | [Architecture](docs/development/architecture.md), [directory structure](docs/development/structure.md) |
| Native handles, COM, or shutdown | [Rust API and resource lifetimes](docs/development/rust-api-review.md) |
| Configuration, databases, or recovery | [Configuration and workspace storage](docs/development/storage.md) |
| Translation, languages, or fonts | [Localization](docs/development/localization.md) |
| UI, materials, or rendering | [Settings components](docs/development/settings-components.md), [rendering](docs/development/rendering.md) |

Follow nearby code conventions and the owning module's validation rules. Update generated bindings through their generator rather than editing generated output. Keep diagnostic and production artifacts in their respective output directories to avoid mixing executables, DLLs, and symbols.

## Validate changes

Choose checks relevant to the change. Documentation edits do not require a full application build. These are common entry points, not a mandatory sequence for every PR:

```powershell
cargo check --workspace --all-targets --locked
cargo test -p luciddesk-core -p luciddesk-storage --lib --locked
python tools/validation/check-locales.py
cargo test -p luciddesk --bin luciddesk i18n::tests --locked
```

| Scope | Focus |
| --- | --- |
| Documentation | Agreement with implementation, correct commands and paths, valid local links and anchors |
| Models and storage | Valid and invalid input, state preservation, failed saves, and compatibility boundaries |
| Translations and settings | Resource keys and arguments, language switching, long text, font fallback, and layout |
| UI and Explorer | Actual input, cancellation, focus, third-party extensions, and final file operations |
| Lifecycles | Normal exit, reconnection, recovery after abnormal termination, and DLL file release |
| Distribution and installation | Package provenance, markers, signing, upgrades, removal, and data retention |

Run native window tests serially or separately as described in the relevant guide. Select ignored interactive tests explicitly. Verify that a test filter actually selects tests: a successful run with zero tests does not validate behavior. CI does not cover all desktop interactions, and the current Build CI does not run automatically for ordinary PRs. An absent failure status is not evidence of passing checks.

See the [validation guide](docs/development/validation.md) for the full workflow. Report commands executed, results, and uncovered scenarios; distinguish environment blockers, test failures, and checks not run. Offscreen images cannot replace actual first-frame and input checks, and forcibly terminating the main process does not validate a real crash path.

## Submit a pull request

Keep each PR focused on one problem and describe the final change:

1. Explain the user-facing problem or concrete trigger.
2. Describe the resulting behavior and relevant design tradeoffs.
3. List validation performed, results, and remaining limitations; include screenshots for UI changes.
4. Link related issues and note data compatibility or migration implications.

Exclude unrelated formatting, temporary logs, test artifacts, personal data, certificates, and private keys. Ensure you have permission to contribute code and assets under the project's [MIT License](LICENSE) and applicable third-party licenses.

Follow the localization guide for new strings, resource keys, and placeholders. When adding a language, check registration and test coverage as well. Update user documentation for visible behavior changes and the relevant development guide for implementation constraints. Keep temporary investigation history out of long-term implementation documentation.

Changelog entries should cover application changes and follow the existing Chinese and English structure. Put unreleased changes in the unreleased section; documentation-only cleanup does not need an application release-note entry. Follow the [build guide](docs/development/build.md) for versioning and release rules rather than incrementing the version for each commit.

Conventional Commit types such as `feat`, `fix`, `docs`, `test`, `build`, and `ci` are recommended. Respond to review feedback with reproduction details, explanations, or changes, and update the PR description and validation results accordingly. Keep discussions respectful and focused on the problem and evidence.
