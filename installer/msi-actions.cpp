// Native MSI actions: read-only preflight and post-commit startup cleanup.
// No .NET/PowerShell runtime is required on the user's computer.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <msiquery.h>
#include <tlhelp32.h>
#include <shlobj.h>
#include <cwchar>
#include <string>
#include <vector>

static std::wstring property(MSIHANDLE install, const wchar_t* name) {
    DWORD size = 0;
    MsiGetPropertyW(install, name, L"", &size);
    std::vector<wchar_t> buffer(size + 1);
    ++size; // The input capacity includes the terminating NUL.
    if (MsiGetPropertyW(install, name, buffer.data(), &size) != ERROR_SUCCESS) return {};
    return std::wstring(buffer.data(), size);
}

static UINT fail(MSIHANDLE install, const wchar_t* message) {
    MSIHANDLE record = MsiCreateRecord(0);
    MsiRecordSetStringW(record, 0, message);
    MsiProcessMessage(install, INSTALLMESSAGE_ERROR, record);
    MsiCloseHandle(record);
    return ERROR_INSTALL_FAILURE;
}

static bool exists(const std::wstring& path) {
    return GetFileAttributesW(path.c_str()) != INVALID_FILE_ATTRIBUTES;
}

static bool legacy_install() {
    const wchar_t* key = L"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{A059751B-F1E3-4C4A-AB35-A03FB70C3CF4}_is1";
    for (HKEY hive : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {
        for (REGSAM view : {KEY_WOW64_32KEY, KEY_WOW64_64KEY}) {
            HKEY opened;
            if (RegOpenKeyExW(hive, key, 0, KEY_READ | view, &opened) == ERROR_SUCCESS) {
                RegCloseKey(opened);
                return true;
            }
        }
    }
    return false;
}

// 0 = released, 1 = mapped, 2 = inspection failed. Keep mapped files intact.
static int mapped_component(DWORD pid) {
    HANDLE snapshot = INVALID_HANDLE_VALUE;
    for (int attempt = 0; attempt < 10; ++attempt) {
        snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if (snapshot != INVALID_HANDLE_VALUE || GetLastError() != ERROR_BAD_LENGTH) break;
        Sleep(10);
    }
    if (snapshot == INVALID_HANDLE_VALUE) return 2;
    MODULEENTRY32W entry = {sizeof(entry)};
    BOOL available = Module32FirstW(snapshot, &entry);
    int result = 0;
    while (available) {
        if (_wcsicmp(entry.szModule, L"luciddesk_explorer.dll") == 0 ||
            _wcsicmp(entry.szModule, L"luciddesk_desktop.dll") == 0) {
            result = 1;
            break;
        }
        available = Module32NextW(snapshot, &entry);
    }
    if (!result && GetLastError() != ERROR_NO_MORE_FILES) result = 2;
    CloseHandle(snapshot);
    return result;
}

extern "C" __declspec(dllexport) UINT __stdcall Preflight(MSIHANDLE install) {
    // MSI's compatibility layer also shims version APIs in custom-action DLLs.
    DWORD major = 0, size = sizeof(major);
    const auto version_result = RegGetValueW(HKEY_LOCAL_MACHINE,
        L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", L"CurrentMajorVersionNumber",
        RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY, nullptr, &major, &size);
    wchar_t build_text[32] = {};
    DWORD build_size = sizeof(build_text);
    const auto build_result = RegGetValueW(HKEY_LOCAL_MACHINE,
        L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", L"CurrentBuildNumber",
        RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY, nullptr, build_text, &build_size);
    wchar_t* build_end = nullptr;
    const auto build = wcstoul(build_text, &build_end, 10);
    if (property(install, L"REMOVE") != L"ALL" &&
        (version_result != ERROR_SUCCESS || major < 10 ||
         (major == 10 && (build_result != ERROR_SUCCESS || build_end == build_text ||
                          *build_end != L'\0' || build < 17763))))
        return fail(install, L"Requires Windows 10 1809 (build 17763) or later, 64-bit. / 需要 64 位 Windows 10 1809（构建 17763）或更新系统。");
    const auto folder = property(install, L"INSTALLFOLDER");
    if (folder.empty()) return fail(install, L"The installation directory is unavailable.");
    if (exists(folder + L"portable") || exists(folder + L"portable.marker"))
        return fail(install, L"This folder contains a portable installation. Choose another folder. / 此目录包含便携版，请选择其他目录。");
#ifndef LUCIDDESK_INSTALLER_FIXTURE
    if (property(install, L"Installed").empty() && legacy_install())
        return fail(install, L"Uninstall the previous EXE/Inno edition first and keep your settings, then install this MSI. / 请先卸载旧 EXE 安装版并保留配置，再安装 MSI。");
    const wchar_t* window_name = L"LucidDesk Tray";
#else
    const auto fixture_name = property(install, L"ProductName");
    const wchar_t* window_name = fixture_name.c_str();
#endif
    HWND window = FindWindowW(L"windows-window.Window", window_name);
    if (window) {
        if (property(install, L"CLOSEAPP") == L"0")
            return fail(install, L"Exit LucidDesk before continuing. / 请先退出 LucidDesk。");
        DWORD pid = 0;
        GetWindowThreadProcessId(window, &pid);
        HANDLE process = OpenProcess(SYNCHRONIZE, FALSE, pid);
        if (!process) return fail(install, L"Cannot wait for LucidDesk to exit. Close it manually. / 请手动退出 LucidDesk。");
        DWORD current_pid = 0;
        GetWindowThreadProcessId(window, &current_pid);
        BOOL sent = current_pid == pid && PostMessageW(window, WM_CLOSE, 0, 0);
        DWORD waited = sent ? WaitForSingleObject(process, 20000) : WAIT_FAILED;
        CloseHandle(process);
        if (waited != WAIT_OBJECT_0 || FindWindowW(L"windows-window.Window", window_name))
            return fail(install, L"LucidDesk has not exited. No application files were changed. / LucidDesk 尚未退出，未修改程序文件。");
    }
#ifndef LUCIDDESK_INSTALLER_FIXTURE
    HANDLE mutex = OpenMutexW(SYNCHRONIZE, FALSE, L"Local\\LucidDesk.DesktopSession");
    if (mutex) {
        CloseHandle(mutex);
        return fail(install, L"LucidDesk is still starting or exiting. Retry after it has exited. / LucidDesk 尚未退出，请稍后重试。");
    }
    HANDLE processes = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (processes == INVALID_HANDLE_VALUE) return fail(install, L"Cannot inspect Explorer. / 无法检查资源管理器。");
    PROCESSENTRY32W entry = {sizeof(entry)};
    BOOL available = Process32FirstW(processes, &entry);
    int status = 0;
    while (available) {
        if (_wcsicmp(entry.szExeFile, L"explorer.exe") == 0) {
            for (int attempt = 0; attempt < 40; ++attempt) {
                status = mapped_component(entry.th32ProcessID);
                if (status != 1) break;
                Sleep(250);
            }
            if (status) break;
        }
        available = Process32NextW(processes, &entry);
    }
    if (!status && GetLastError() != ERROR_NO_MORE_FILES) status = 2;
    CloseHandle(processes);
    if (status) return fail(install, L"Explorer still has a desktop component loaded, or could not be inspected. Exit LucidDesk; if necessary restart Explorer, then retry. / Explorer 桌面组件尚未释放或无法检查，请退出 LucidDesk，必要时重启资源管理器后重试。");
#endif
    for (const wchar_t* file : {L"luciddesk.exe", L"luciddesk_explorer.dll", L"luciddesk_desktop.dll"}) {
        const auto path = folder + file;
        if (!exists(path)) continue;
        HANDLE handle = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ,
            nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (handle == INVALID_HANDLE_VALUE)
            return fail(install, L"An application file is in use or not writable. No files were changed. / 程序文件被占用或不可写，未修改文件。");
        CloseHandle(handle);
    }
    return ERROR_SUCCESS;
}

extern "C" __declspec(dllexport) UINT __stdcall CleanupStartup(MSIHANDLE install) {
    // Runs only after a successful final uninstall, in the initiating user's context.
    // Compare the full command so another installation's startup entry survives.
    const auto folder = property(install, L"CustomActionData");
    if (folder.empty()) return ERROR_SUCCESS;
    HKEY key;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        0, KEY_QUERY_VALUE | KEY_SET_VALUE, &key) == ERROR_SUCCESS) {
        wchar_t value[32768];
        DWORD size = sizeof(value), type = 0;
        if (RegQueryValueExW(key, L"LucidDesk", nullptr, &type, reinterpret_cast<BYTE*>(value), &size) == ERROR_SUCCESS
            && type == REG_SZ && size >= sizeof(wchar_t) && size <= sizeof(value)) {
            value[32767] = 0;
            const auto expected = L"\"" + folder + L"luciddesk.exe\" --startup";
            if (_wcsicmp(value, expected.c_str()) == 0) RegDeleteValueW(key, L"LucidDesk");
        }
        RegCloseKey(key);
    }
    SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_PATHW, folder.c_str(), nullptr);
    return ERROR_SUCCESS;
}
