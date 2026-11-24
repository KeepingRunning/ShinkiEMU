#include "shinki_emu.h"
#include <SDL.h>
#include <algorithm>
#include <filesystem>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>

namespace fs = std::filesystem;
extern "C" int shinki_emu_query(uint32_t, const ShinkiEmuCoreApi **) noexcept;
#if defined(__APPLE__)
std::string documents_directory();
#endif

static ShinkiEmuBytes bytes(std::string_view value) {
    return {reinterpret_cast<const uint8_t *>(value.data()), value.size()};
}
static void check(int status, const ShinkiEmuError &error) {
    if (status == ShinkiEmuOK) return;
    std::string message(reinterpret_cast<const char *>(error.message), error.message_len);
    throw std::runtime_error(message.empty() ? "Leaf core status " + std::to_string(status) : message);
}
static void sdl_check(bool ok) {
    if (!ok) throw std::runtime_error(SDL_GetError());
}
struct Session {
    const ShinkiEmuCoreApi *api;
    ShinkiEmuOpenResult opened{};
    ~Session() {
        if (opened.session) {
            api->cancel(opened.session);
            api->close(opened.session, nullptr);
        }
    }
};

static int play(SDL_Window *window, const fs::path &root, bool special) {
    const ShinkiEmuCoreApi *api = nullptr;
    ShinkiEmuError error{};
    check(shinki_emu_query(3, &api), error);
    ShinkiEmuDiagnostics diagnostics{};
    check(api->initialize(&diagnostics, &error), error);
    auto utf8 = root.u8string();
    std::string path(utf8.begin(), utf8.end());
    ShinkiEmuProbeResult probe{};
    if (api->probe(bytes(path), &probe, &error) != ShinkiEmuOK)
        throw std::runtime_error("No readable Leaf game data in " + path);
    ShinkiEmuConfigEntry profile[] = {{bytes("entry_scene"), {ShinkiEmuVALUE_STRING, 0, 0, 0, bytes("4000")}},
                                     {bytes("script_bank"), {ShinkiEmuVALUE_STRING, 0, 0, 0, bytes("script_cn")}}};
    ShinkiEmuOpenRequest request{};
    request.header = {sizeof(request), 3};
    request.game_path = bytes(path);
    request.session_epoch = 1;
    request.host.header = {sizeof(request.host), 3};
    request.host.cancellation.is_cancelled = [](void *, uint64_t) -> uint32_t {return 0;};
    if (special) {
        request.configuration = profile;
        request.configuration_count = 2;
    }
    Session session{api};
    check(api->open(&request, &session.opened, &error), error);
    const auto handle = session.opened.session;
    const auto size = session.opened.surface.raster;
    using Renderer = std::unique_ptr<SDL_Renderer, decltype(&SDL_DestroyRenderer)>;
    Renderer renderer(SDL_CreateRenderer(window, -1, SDL_RENDERER_PRESENTVSYNC), SDL_DestroyRenderer);
    if (!renderer) renderer.reset(SDL_CreateRenderer(window, -1, 0));
    sdl_check(bool(renderer));
    sdl_check(SDL_RenderSetLogicalSize(renderer.get(), size.width, size.height) == 0);
    using Texture = std::unique_ptr<SDL_Texture, decltype(&SDL_DestroyTexture)>;
    Texture texture(nullptr, SDL_DestroyTexture);
    ShinkiEmuFrameIdentity last{};
    bool background = false;
    for (;;) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            ShinkiEmuInputEvent input{};
            input.displayed = last;
            switch (event.type) {
            case SDL_QUIT: case SDL_APP_TERMINATING: return 0;
            case SDL_APP_WILLENTERBACKGROUND:
            case SDL_APP_DIDENTERFOREGROUND:
                background = event.type == SDL_APP_WILLENTERBACKGROUND;
                check(api->control(handle, background ? ShinkiEmuCONTROL_PAUSE : ShinkiEmuCONTROL_RESUME, &error), error);
                continue;
            case SDL_RENDER_DEVICE_RESET: case SDL_RENDER_TARGETS_RESET:
                texture.reset(); last = {}; continue;
            case SDL_MOUSEBUTTONDOWN: case SDL_MOUSEBUTTONUP:
                input.kind = ShinkiEmuINPUT_POINTER_BUTTON;
                input.button = event.button.button == SDL_BUTTON_RIGHT ? ShinkiEmuBUTTON_SECONDARY : ShinkiEmuBUTTON_PRIMARY;
                input.pressed = event.type == SDL_MOUSEBUTTONDOWN;
                input.x = event.button.x; input.y = event.button.y; break;
            case SDL_MOUSEMOTION:
                input.kind = ShinkiEmuINPUT_POINTER_MOVE;
                input.x = event.motion.x; input.y = event.motion.y; break;
            case SDL_KEYDOWN: case SDL_KEYUP:
                if (event.key.keysym.scancode == SDL_SCANCODE_AC_BACK) return 0;
                input.kind = ShinkiEmuINPUT_KEY;
                input.physical_key = bytes(event.key.keysym.sym == SDLK_RETURN ? "Enter" : SDL_GetKeyName(event.key.keysym.sym));
                input.logical_key = input.physical_key;
                input.repeat = event.key.repeat;
                input.pressed = event.type == SDL_KEYDOWN;
                break;
            default: continue;
            }
            check(api->input(handle, &input, 1, &error), error);
        }
        ShinkiEmuSessionState state{};
        check(api->state(handle, &state, &error), error);
        if (state.state == ShinkiEmuSESSION_FAILED) check(ShinkiEmuCORE_ERROR, state.error);
        if (state.state == ShinkiEmuSESSION_FINISHED) return 0;
        if (background) {SDL_Delay(50); continue;}
        ShinkiEmuFrameSurface frame{};
        check(api->frame(handle, last, &frame, &error), error);
        if (frame.state == ShinkiEmuFRAME_READY) {
            struct Release { ShinkiEmuCpuLease lease; ~Release(){lease.release(lease.owner);} } release{frame.cpu};
            if (!texture) texture.reset(SDL_CreateTexture(renderer.get(), SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STREAMING, size.width, size.height));
            sdl_check(bool(texture));
            sdl_check(SDL_UpdateTexture(texture.get(), nullptr, frame.cpu.pixels.data, int(frame.info.stride)) == 0);
            last = frame.identity;
        }
        if (texture) {
            sdl_check(SDL_RenderClear(renderer.get()) == 0);
            sdl_check(SDL_RenderCopy(renderer.get(), texture.get(), nullptr, nullptr) == 0);
            SDL_RenderPresent(renderer.get());
        }
        SDL_Delay(4);
    }
}

int main(int argc, char **argv) {
    SDL_SetHint(SDL_HINT_TOUCH_MOUSE_EVENTS, "1");
    SDL_SetHint(SDL_HINT_RENDER_SCALE_QUALITY, "linear");
    if (SDL_Init(SDL_INIT_VIDEO | SDL_INIT_EVENTS) != 0) return 1;
    struct Lifetime { ~Lifetime(){SDL_Quit();} } lifetime;
    using Window = std::unique_ptr<SDL_Window, decltype(&SDL_DestroyWindow)>;
    Window window(SDL_CreateWindow("Leaf", SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED,
        1280, 720, SDL_WINDOW_FULLSCREEN_DESKTOP | SDL_WINDOW_ALLOW_HIGHDPI), SDL_DestroyWindow);
    if (!window) return 1;
    try {
#ifdef __ANDROID__
        if (argc != 3) throw std::runtime_error("Open a game from the library.");
        return play(window.get(), fs::path(argv[1]), std::string_view(argv[2]) == "special");
#else
        const fs::path docs = documents_directory();
        fs::create_directories(docs / "Main");
        fs::create_directories(docs / "Special");
        SDL_MessageBoxButtonData buttons[] = {{0, 0, "Main"}, {0, 1, "Special Contents"}, {0, 2, "Close"}};
        SDL_MessageBoxData dialog{SDL_MESSAGEBOX_INFORMATION, window.get(), "WHITE ALBUM2",
            "In Files, copy the extracted game files into Leaf / Main or Leaf / Special.", 3, buttons, nullptr};
        int selected = -1;
        sdl_check(SDL_ShowMessageBox(&dialog, &selected) == 0);
        if (selected < 0 || selected == 2) return 0;
        return play(window.get(), docs / (selected == 1 ? "Special" : "Main"), selected == 1);
#endif
    } catch (const std::exception &error) {
        SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, "Leaf", error.what(), window.get());
        return 1;
    }
}
