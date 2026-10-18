// Standalone PDH retention probe: no plugin, graphics, DLL unloading or trimming.
#include <windows.h>
#include <pdh.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void check(const char* operation, PDH_STATUS status) {
    printf("%s=%lu\n", operation, status);
    if (status != ERROR_SUCCESS) exit(1);
}

void measure(const char* phase) {
    PROCESS_MEMORY_COUNTERS_EX memory = {};
    memory.cb = sizeof(memory);
    if (!GetProcessMemoryInfo(GetCurrentProcess(),
            reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory), sizeof(memory))) exit(1);
    HANDLE heaps[256];
    DWORD count = GetProcessHeaps(256, heaps);
    if (count == 0 || count > 256) exit(1);
    SIZE_T allocated = 0;
    for (DWORD i = 0; i < count; ++i) {
        if (!HeapLock(heaps[i])) exit(1);
        PROCESS_HEAP_ENTRY entry = {};
        while (HeapWalk(heaps[i], &entry)) {
            if (entry.wFlags & PROCESS_HEAP_ENTRY_BUSY) allocated += entry.cbData;
        }
        DWORD error = GetLastError();
        HeapUnlock(heaps[i]);
        if (error != ERROR_NO_MORE_ITEMS) exit(1);
    }
    printf("%s private_kib=%llu live_heap_kib=%llu\n", phase,
        static_cast<unsigned long long>(memory.PrivateUsage / 1024),
        static_cast<unsigned long long>(allocated / 1024));
    fflush(stdout);
}

int main(int argc, char** argv) {
    measure("baseline");
    PDH_HQUERY query = nullptr;
    PDH_HCOUNTER counter = nullptr;
    check("open", PdhOpenQueryW(nullptr, 0, &query));
    measure("query-open");
    const wchar_t* path = argc > 1 && strcmp(argv[1], "gpu") == 0
        ? L"\\GPU Engine(*)\\Utilization Percentage"
        : L"\\Processor(_Total)\\% Idle Time";
    // Optional third argument tests localized lookup on an English counter locale.
    check("add", argc > 2 ? PdhAddCounterW(query, path, 0, &counter)
                         : PdhAddEnglishCounterW(query, path, 0, &counter));
    measure("counter-added");
    for (int i = 0; i < 2; ++i) {
        check("collect", PdhCollectQueryData(query));
        Sleep(1100);
    }
    measure("sampled");
    check("remove", PdhRemoveCounter(counter));
    measure("counter-removed");
    check("close", PdhCloseQuery(query));
    measure("query-closed");
    Sleep(2000);
    measure("settled");
}
