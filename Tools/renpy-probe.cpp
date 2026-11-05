// Run from a clean Windows native process; never create a module owner or session.
#include "shinki_emu.h"
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

namespace fs = std::filesystem;

struct Fixtures {
    fs::path root;
    explicit Fixtures(const fs::path &path) : root(fs::absolute(path)) {
        if (!fs::create_directory(root))
            throw std::runtime_error("Fixture directory must not already exist");
    }
    ~Fixtures() {
        std::error_code ignored;
        fs::remove_all(root, ignored);
    }
    fs::path game(const fs::path &name, const char *file, const char *contents) {
        const auto path = root / name;
        fs::create_directories(path / "game");
        std::ofstream output(path / "game" / file, std::ios::binary);
        output << contents;
        output.close();
        if (!output) throw std::runtime_error("Cannot write fixture");
        return path;
    }
};

void emit(void *, const ShinkiEmuLogRecord *) {}

int wmain(int argc, wchar_t **argv) {
    if (argc != 3) {
        std::cerr << "Usage: renpy-probe.exe CORE_DLL NEW_FIXTURE_DIRECTORY\n";
        return 2;
    }
    try {
        const auto dll = fs::absolute(argv[1]);
        HMODULE library = LoadLibraryExW(dll.c_str(), nullptr,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if (!library) throw std::runtime_error("LoadLibraryExW failed: " + std::to_string(GetLastError()));
        const auto query = reinterpret_cast<ShinkiEmuQuery>(GetProcAddress(library, "shinki_emu_query"));
        const auto python = GetModuleHandleW(L"libpython3.12.dll");
        const auto initialized = python ? reinterpret_cast<int (*)()>(GetProcAddress(python, "Py_IsInitialized")) : nullptr;
        if (!query || !initialized || initialized())
            throw std::runtime_error("Expected a loaded, uninitialized Ren'Py CPython runtime");
        const ShinkiEmuCoreApi *api = nullptr;
        if (query(ShinkiEmuABI_VERSION, &api) != ShinkiEmuOK || !api ||
            !api->initialize || !api->probe)
            throw std::runtime_error("Cannot query Ren'Py ABI");
        ShinkiEmuDiagnostics diagnostics{{sizeof(ShinkiEmuDiagnostics), ShinkiEmuABI_VERSION}, nullptr, emit};
        ShinkiEmuError error{};
        if (api->initialize(&diagnostics, &error) != ShinkiEmuOK || initialized())
            throw std::runtime_error("Core initialization must not start CPython");
        Fixtures fixtures(argv[2]);
        size_t checks = 0;
        auto check = [&](ShinkiEmuBytes path, ShinkiEmuStatus expected, const char *format = nullptr) {
            ShinkiEmuProbeResult result{};
            const auto status = api->probe(path, &result, &error);
            if (status != expected || initialized())
                throw std::runtime_error("Unexpected probe status or CPython initialization at case " + std::to_string(checks));
            if (format && (result.format.len != std::strlen(format) ||
                std::memcmp(result.format.data, format, result.format.len) != 0 ||
                result.confidence_permyriad != 10000 || !result.game_id.len))
                throw std::runtime_error("Incorrect Ren'Py detection result");
            ++checks;
        };
        auto check_path = [&](const fs::path &path, ShinkiEmuStatus expected, const char *format = nullptr) {
            const auto utf8 = path.u8string();
            check({reinterpret_cast<const uint8_t *>(utf8.data()), utf8.size()}, expected, format);
        };
        const auto script = fixtures.game("script", "script.rpy", "label start:\n    return\n");
        check_path(script, ShinkiEmuOK, "rpy");
        check_path(fixtures.game(fs::u8path(u8"游戏-\U0001f600"), "script.rpy", "label start:\n    return\n"), ShinkiEmuOK, "rpy");
        check_path(fixtures.game(fs::u8path("scalar-\xc2\x80\xdf\xbf\xe0\xa0\x80\xed\x9f\xbf\xee\x80\x80\xef\xbf\xbf\xf0\x90\x80\x80\xf4\x8f\xbf\xbf"), "script.rpy", "label start:\n    return\n"), ShinkiEmuOK, "rpy");
        check_path(fixtures.game("compiled", "script.rpyc", "RENPY RPC2payload"), ShinkiEmuOK, "rpyc");
        check_path(fixtures.game("archive", "script.rpa", "RPA-3.0 payload"), ShinkiEmuOK, "rpa");
        check_path(fixtures.game("invalid-archive", "script.rpa", "not an archive"), ShinkiEmuNO_MATCH);
        check_path(fixtures.game("empty-script", "script.rpy", ""), ShinkiEmuNO_MATCH);
        // A loose .rpy outside a game/ directory remains insufficient evidence.
        check_path(script / "game", ShinkiEmuNO_MATCH);
        check_path(fixtures.root / "missing", ShinkiEmuINVALID_ARGUMENT);
        check_path("relative", ShinkiEmuINVALID_ARGUMENT);
        check({nullptr, 0}, ShinkiEmuINVALID_ARGUMENT);
        check({nullptr, 1}, ShinkiEmuINVALID_ARGUMENT);
        const uint8_t byte = 'x';
        check({&byte, 131073}, ShinkiEmuINVALID_ARGUMENT);
        const char nul[] = {'a', 0, 'b'};
        check({reinterpret_cast<const uint8_t *>(nul), sizeof(nul)}, ShinkiEmuINVALID_ARGUMENT);
        const char *invalid[] = {
            "\x80", "\xbf", "\xc0\x80", "\xc1\xbf", "\xc2", "\xc2\x7f",
            "\xe0\x9f\xbf", "\xe0\xa0", "\xe1\x80", "\xe1\x7f\x80", "\xe1\x80\x7f",
            "\xed\xa0\x80", "\xed\xbf\xbf", "\xef\xbf",
            "\xf0\x8f\xbf\xbf", "\xf0\x90\x80", "\xf1\x80\x80", "\xf1\x7f\x80\x80",
            "\xf1\x80\x7f\x80", "\xf1\x80\x80\x7f", "\xf4\x90\x80\x80", "\xf5\x80\x80\x80", "\xff"
        };
        for (const auto *value : invalid)
            check({reinterpret_cast<const uint8_t *>(value), std::strlen(value)}, ShinkiEmuINVALID_ARGUMENT);
        const auto utf8 = script.u8string();
        if (api->probe({reinterpret_cast<const uint8_t *>(utf8.data()), utf8.size()}, nullptr, &error) != ShinkiEmuINVALID_ARGUMENT || initialized())
            throw std::runtime_error("Null probe output was not rejected safely");
        ++checks;
        std::cout << "{\"checks\":" << checks << ",\"python_initialized\":false,\"session_opened\":false}\n";
        FreeLibrary(library);
        return 0;
    } catch (const std::exception &error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
