#include "file_io.hpp"
#include "native_desktop.hpp"
#include "sdl_application.hpp"
#include "shinki_emu.h"
#include <SDL.h>
#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <thread>

namespace fs = std::filesystem;
namespace platform = wa2::platform;
using namespace std::chrono_literals;
using Clock = std::chrono::steady_clock;
using Pixels = std::vector<uint8_t>;
using Lease = std::shared_ptr<const Pixels>;
static constexpr int width = 1280, height = 720;

static ShinkiEmuBytes bytes(std::string_view value) {
  return {reinterpret_cast<const uint8_t *>(value.data()), value.size()};
}
static std::string string_of(ShinkiEmuBytes value) {
  if (!value.len)
    return {};
  if (!value.data)
    throw std::invalid_argument("Null string");
  return {reinterpret_cast<const char *>(value.data), value.len};
}
static int error(ShinkiEmuError *out, const char *code, const char *message,
                 int status) noexcept {
  if (out) {
    *out = {};
    out->code_len = std::min(strlen(code), sizeof(out->code));
    out->message_len = std::min(strlen(message), sizeof(out->message));
    memcpy(out->code, code, out->code_len);
    memcpy(out->message, message, out->message_len);
  }
  return status;
}
static ShinkiEmuSurfaceInfo surface_info() {
  return {ShinkiEmuSURFACE_CPU,   ShinkiEmuPIXEL_RGBA8,
          ShinkiEmuCOLOR_SRGB,    ShinkiEmuALPHA_IGNORED,
          ShinkiEmuROWS_TOP_DOWN, {width, height},
          {width, height},        width * 4};
}
static bool leaf_scripts(const fs::path &root) {
  if (!fs::is_directory(root))
    return false;
  for (const auto &entry : fs::directory_iterator(root)) {
    if (!entry.is_regular_file())
      continue;
    std::array<char, 4> magic{};
    std::ifstream input(entry.path(), std::ios::binary);
    if (!input.read(magic.data(), 4) ||
        (memcmp(magic.data(), "KCAP", 4) && memcmp(magic.data(), "LAC\0", 4)))
      continue;
    auto bank = wa2::resource::Archive::open(
        platform::open_resource_file(entry.path()));
    for (const auto &member : bank.members()) {
      const auto dot = member.name.rfind('.');
      if (dot != std::string::npos &&
          wa2::resource::compare_names(std::string_view(member.name).substr(dot), ".bnr") == 0 &&
          bank.find(member.name.substr(0, dot) + ".txt") != bank.missing)
        return true;
    }
  }
  return false;
}

struct TrialOptions {
  std::string scene;
  std::string script_bank;
};
static ShinkiEmuConfigField field(std::string_view id, std::string_view label) {
  ShinkiEmuConfigField f{};
  f.id = bytes(id);
  f.label = bytes(label);
  f.group = bytes("Scenario");
  f.default_value.kind = ShinkiEmuVALUE_STRING;
  f.max_bytes = 64;
  return f;
}
static TrialOptions options(const ShinkiEmuOpenRequest &req) {
  TrialOptions result;
  if (req.configuration_count > 2 ||
      (req.configuration_count && !req.configuration))
    throw std::invalid_argument("Invalid scenario configuration");
  for (size_t i = 0; i < req.configuration_count; ++i) {
    const auto &item = req.configuration[i];
    if (item.value.kind != ShinkiEmuVALUE_STRING || item.value.text.len > 64)
      throw std::invalid_argument("Invalid scenario setting");
    auto id = string_of(item.id), value = string_of(item.value.text);
    if (value.find_first_not_of("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrst"
                                "uvwxyz0123456789_-") != std::string::npos)
      throw std::invalid_argument("Expected an archive or scene stem");
    if (id == "entry_scene")
      result.scene = value;
    else if (id == "script_bank")
      result.script_bank = value;
    else
      throw std::invalid_argument("Unknown scenario setting");
  }
  return result;
}
struct Session {
  uint64_t epoch;
  std::mutex mutex;
  std::condition_variable wake;
  bool stop{}, paused{}, ready{};
  uint32_t status{ShinkiEmuSESSION_RUNNING};
  ShinkiEmuError failure{};
  platform::InputSnapshot pending;
  Lease pixels;
  uint64_t generation{}, timestamp{};
  std::jthread thread;

  explicit Session(uint64_t value) : epoch(value) {}
  ~Session() {
    {
      std::lock_guard lock(mutex);
      stop = true;
    }
    wake.notify_all();
    if (thread.joinable())
      thread.join();
  }
  void work(const fs::path &root, const TrialOptions &opts) noexcept {
    try {
      auto base = platform::native_desktop_banks(root);
      auto banks = [base, selected = opts.script_bank](std::string_view path) {
        // "ck-gal" is the logical script group requested by upstream Scene.
        if (!selected.empty() && path == "ck-gal.pak")
          return base(selected + ".pak");
        if (!selected.empty() && path == "IC/ck-gal.pak")
          return base("IC/" + selected + ".pak");
        return base(path);
      };
      auto save = root / ".shinki" / "leaf-trial";
      fs::create_directories(save);
      auto app = platform::native_application(
          banks, save, {}, {}, {},
          platform::native_desktop_legacy_settings(root),
          platform::native_desktop_legacy_saves(root));
      if (!opts.scene.empty())
        app->start({opts.scene}, 0);
      using Surface = std::unique_ptr<SDL_Surface, decltype(&SDL_FreeSurface)>;
      Surface surface(SDL_CreateRGBSurfaceWithFormat(0, width, height, 32,
                                                     SDL_PIXELFORMAT_RGBA32),
                      SDL_FreeSurface);
      if (!surface)
        throw std::runtime_error(SDL_GetError());
      auto *renderer = SDL_CreateSoftwareRenderer(surface.get());
      if (!renderer)
        throw std::runtime_error(SDL_GetError());
      auto canvas = platform::SdlCanvas::adopt(renderer);
      canvas->logical_size(width, height);
      auto start = Clock::now();
      auto time_us = [&] {
        return std::chrono::duration_cast<std::chrono::microseconds>(
                   Clock::now() - start)
            .count();
      };
      auto submit = [&] {
        if (SDL_RenderFlush(renderer) || SDL_LockSurface(surface.get()))
          throw std::runtime_error(SDL_GetError());
        struct Unlock {
          SDL_Surface *s;
          ~Unlock() { SDL_UnlockSurface(s); }
        } unlock{surface.get()};
        auto image = std::make_shared<Pixels>(width * height * 4);
        for (int y = 0; y < height; ++y)
          memcpy(image->data() + y * width * 4,
                 static_cast<uint8_t *>(surface->pixels) + y * surface->pitch,
                 width * 4);
        std::lock_guard lock(mutex);
        pixels = std::move(image);
        ++generation;
        timestamp = time_us() * 1000;
      };
      wa2::game::DriverSettings settings;
      settings.presentation_hz = 60;
      platform::SdlApplication runner(*app, *canvas, settings, {}, submit);
      bool observed_story = false, observed_dialogue = false;
      uint64_t observed_voices = 0;
      const bool trace = std::getenv("SHINKI_LEAF_TRIAL_TRACE") != nullptr;
      {
        std::lock_guard lock(mutex);
        ready = true;
      }
      wake.notify_all();
      for (;;) {
        platform::InputSnapshot events;
        {
          std::lock_guard lock(mutex);
          if (stop)
            break;
          events = pending;
          events.background |= paused;
          pending.pressed_keys.reset();
          pending.released_keys.reset();
          pending.pressed_buttons.reset();
          pending.released_buttons.reset();
          pending.wheel_x = pending.wheel_y = 0;
        }
        auto tick = runner.pump(time_us(), events, time_us);
        if (trace && app->page() == wa2::game::Page::story && app->scene()) {
          if (!observed_story) {
            std::fputs("Leaf: entered the upstream story VM.\n", stderr);
            observed_story = true;
          }
          const auto &stats = app->scene()->statistics();
          if (stats.voices != observed_voices) {
            if (!observed_voices || stats.voices / 10 != observed_voices / 10)
              std::fprintf(stderr, "Leaf: scene %s, %llu messages, %llu voices started.\n",
                           stats.scene.c_str(), static_cast<unsigned long long>(stats.messages),
                           static_cast<unsigned long long>(stats.voices));
            observed_voices = stats.voices;
          }
          if (!observed_dialogue &&
              !app->scene()->backlog().entries().empty()) {
            std::fputs(
                "Leaf: upstream story backlog received a dialogue entry.\n",
                stderr);
            observed_dialogue = true;
          }
        }
        if (tick.closed) {
          std::lock_guard lock(mutex);
          status = ShinkiEmuSESSION_FINISHED;
          break;
        }
        std::unique_lock lock(mutex);
        wake.wait_for(lock,
                      std::chrono::microseconds(
                          std::clamp<int64_t>(tick.sleep_us, 500, 16000)),
                      [&] { return stop; });
      }
    } catch (const std::exception &ex) {
      std::lock_guard lock(mutex);
      error(&failure, "LEAF_RUNTIME", ex.what(), ShinkiEmuCORE_ERROR);
      status = ShinkiEmuSESSION_FAILED;
    } catch (...) {
      std::lock_guard lock(mutex);
      error(&failure, "LEAF_RUNTIME", "Unknown engine error",
            ShinkiEmuCORE_ERROR);
      status = ShinkiEmuSESSION_FAILED;
    }
    wake.notify_all();
  }
};

static int initialize(const ShinkiEmuDiagnostics *diagnostics,
                      ShinkiEmuError *) noexcept {
  return diagnostics ? ShinkiEmuOK : ShinkiEmuINVALID_ARGUMENT;
}
static int describe(ShinkiEmuDescriptor *out, ShinkiEmuError *) noexcept {
  if (!out)
    return ShinkiEmuINVALID_ARGUMENT;
  static const ShinkiEmuBytes formats[] = {bytes("leaf.bnr")};
  static const ShinkiEmuConfigField fields[] = {
      field("entry_scene", "Direct scene (optional)"),
      field("script_bank", "Script archive stem (optional)")};
  *out = {{sizeof(*out), 3},
          bytes("leaf"),
          bytes("whitealbum2-Android"),
          bytes("0.1.0-trial.3"),
          ShinkiEmuCAP_CPU_SURFACE,
          formats,
          1,
          fields,
          2,
          bytes("Leaf native trial"),
          bytes("Leaf"),
          {}};
  return ShinkiEmuOK;
}
static int probe(ShinkiEmuBytes game, ShinkiEmuProbeResult *out,
                 ShinkiEmuError *err) noexcept {
  if (!out)
    return ShinkiEmuINVALID_ARGUMENT;
  *out = {};
  try {
    fs::path root = platform::native_utf8_path(string_of(game));
    if (!leaf_scripts(root))
      return ShinkiEmuNO_MATCH;
    static thread_local std::string id;
    const auto name = root.filename().u8string();
    id.assign(name.begin(), name.end());
    *out = {bytes(id), bytes("leaf.bnr"), 9500, 0};
    return ShinkiEmuOK;
  } catch (const std::exception &ex) {
    return error(err, "LEAF_PROBE", ex.what(), ShinkiEmuCORE_ERROR);
  }
}
static int open_game(const ShinkiEmuOpenRequest *req, ShinkiEmuOpenResult *out,
                     ShinkiEmuError *err) noexcept {
  if (!out)
    return ShinkiEmuINVALID_ARGUMENT;
  *out = {};
  if (!req || req->header.size < sizeof(*req) || !req->session_epoch ||
      !req->host.cancellation.is_cancelled)
    return ShinkiEmuINVALID_ARGUMENT;
  try {
    fs::path root = fs::absolute(platform::native_utf8_path(string_of(req->game_path)));
    if (!leaf_scripts(root))
      return ShinkiEmuNO_MATCH;
    auto opts = options(*req);
    auto s = std::make_unique<Session>(req->session_epoch);
    s->thread = std::jthread(
        [owner = s.get(), root, opts] { owner->work(root, opts); });
    auto until = Clock::now() + 30s;
    std::unique_lock lock(s->mutex);
    while (!s->ready && s->status != ShinkiEmuSESSION_FAILED) {
      s->wake.wait_for(lock, 25ms);
      if (req->host.cancellation.is_cancelled(req->host.cancellation.context,
                                              req->session_epoch))
        return ShinkiEmuCANCELLED;
      if (Clock::now() > until)
        return error(err, "LEAF_OPEN_TIMEOUT", "Upstream startup timed out",
                     ShinkiEmuCORE_ERROR);
    }
    if (s->status == ShinkiEmuSESSION_FAILED) {
      if (err)
        *err = s->failure;
      return ShinkiEmuCORE_ERROR;
    }
    *out = {s.release(), 1, 1, surface_info()};
    return ShinkiEmuOK;
  } catch (const std::exception &ex) {
    return error(err, "LEAF_OPEN", ex.what(), ShinkiEmuCORE_ERROR);
  }
}
static SDL_Scancode scancode(const std::string &key) {
  if (key == "Enter")
    return SDL_SCANCODE_RETURN;
  if (key == "ControlLeft")
    return SDL_SCANCODE_LCTRL;
  if (key == "ControlRight")
    return SDL_SCANCODE_RCTRL;
  if (key.starts_with("Arrow"))
    return SDL_GetScancodeFromName(key.c_str() + 5);
  if (key.starts_with("Key"))
    return SDL_GetScancodeFromName(key.c_str() + 3);
  return SDL_GetScancodeFromName(key.c_str());
}
static int input(void *handle, const ShinkiEmuInputEvent *events, size_t count,
                 ShinkiEmuError *err) noexcept {
  if (!handle || (!events && count))
    return ShinkiEmuINVALID_ARGUMENT;
  try {
    auto &s = *static_cast<Session *>(handle);
    std::lock_guard lock(s.mutex);
    auto &p = s.pending;
    for (size_t i = 0; i < count; ++i) {
      const auto &e = events[i];
      switch (e.kind) {
      case ShinkiEmuINPUT_KEY: {
        auto key = scancode(string_of(e.physical_key));
        if (key == SDL_SCANCODE_UNKNOWN)
          break;
        p.keys.set(key, e.pressed);
        if (e.pressed && !e.repeat)
          p.pressed_keys.set(key);
        if (!e.pressed)
          p.released_keys.set(key);
        break;
      }
      case ShinkiEmuINPUT_POINTER_BUTTON: {
        if (!e.button || e.button > 3)
          break;
        unsigned b = e.button == ShinkiEmuBUTTON_SECONDARY ? 2
                     : e.button == ShinkiEmuBUTTON_MIDDLE  ? 1
                                                           : 0;
        p.buttons.set(b, e.pressed);
        (e.pressed ? p.pressed_buttons : p.released_buttons).set(b);
        [[fallthrough]];
      }
      case ShinkiEmuINPUT_POINTER_MOVE:
        p.pointer = true;
        p.x = e.x;
        p.y = e.y;
        break;
      case ShinkiEmuINPUT_WINDOW:
        p.focused = e.window.focused;
        p.background = e.window.suspended;
        break;
      case ShinkiEmuINPUT_WHEEL:
        p.wheel_x += e.x;
        p.wheel_y += e.y;
        break;
      }
    }
    return ShinkiEmuOK;
  } catch (const std::exception &ex) {
    return error(err, "LEAF_INPUT", ex.what(), ShinkiEmuINVALID_ARGUMENT);
  }
}
static int control(void *h, uint32_t c, ShinkiEmuError *) noexcept {
  if (!h)
    return ShinkiEmuINVALID_ARGUMENT;
  if (c != ShinkiEmuCONTROL_PAUSE && c != ShinkiEmuCONTROL_RESUME)
    return ShinkiEmuUNSUPPORTED;
  auto &s = *static_cast<Session *>(h);
  std::lock_guard lock(s.mutex);
  s.paused = c == ShinkiEmuCONTROL_PAUSE;
  s.wake.notify_all();
  return ShinkiEmuOK;
}
static int state(void *h, ShinkiEmuSessionState *out,
                 ShinkiEmuError *) noexcept {
  if (!h || !out)
    return ShinkiEmuINVALID_ARGUMENT;
  auto &s = *static_cast<Session *>(h);
  std::lock_guard lock(s.mutex);
  *out = {s.status == ShinkiEmuSESSION_RUNNING && s.paused
              ? ShinkiEmuSESSION_PAUSED
              : s.status,
          0, 1, 1, s.failure};
  return ShinkiEmuOK;
}
static int frame(void *h, ShinkiEmuFrameIdentity last,
                 ShinkiEmuFrameSurface *out, ShinkiEmuError *err) noexcept {
  if (!h || !out)
    return ShinkiEmuINVALID_ARGUMENT;
  *out = {};
  auto &s = *static_cast<Session *>(h);
  try {
    std::lock_guard lock(s.mutex);
    out->identity = {s.epoch, 1, s.generation};
    out->timestamp_ns = s.timestamp;
    out->info = surface_info();
    if (!s.pixels)
      return ShinkiEmuOK;
    if (last.session_epoch == s.epoch && last.output_epoch == 1 &&
        last.generation == s.generation) {
      out->state = ShinkiEmuFRAME_UNCHANGED;
      return ShinkiEmuOK;
    }
    auto lease = std::make_unique<Lease>(s.pixels);
    out->state = ShinkiEmuFRAME_READY;
    out->cpu = {{s.pixels->data(), s.pixels->size()},
                lease.release(),
                [](void *p) { delete static_cast<Lease *>(p); }};
    return ShinkiEmuOK;
  } catch (const std::exception &ex) {
    return error(err, "LEAF_FRAME", ex.what(), ShinkiEmuCORE_ERROR);
  }
}
static void cancel(void *h) noexcept {
  if (h) {
    auto &s = *static_cast<Session *>(h);
    std::lock_guard lock(s.mutex);
    s.stop = true;
    s.wake.notify_all();
  }
}
static int close_game(void *h, ShinkiEmuError *) noexcept {
  delete static_cast<Session *>(h);
  return ShinkiEmuOK;
}
static int extension(ShinkiEmuBytes, uint32_t, const void **out) noexcept {
  if (!out)
    return ShinkiEmuINVALID_ARGUMENT;
  *out = nullptr;
  return ShinkiEmuUNSUPPORTED;
}
#if defined(_WIN32)
#define LEAF_EXPORT __declspec(dllexport)
#else
#define LEAF_EXPORT __attribute__((visibility("default")))
#endif
extern "C" LEAF_EXPORT int
shinki_emu_query(uint32_t version, const ShinkiEmuCoreApi **out) noexcept {
  if (!out)
    return ShinkiEmuINVALID_ARGUMENT;
  static const ShinkiEmuCoreApi api = {
      {sizeof(api), 3}, initialize, describe, probe,  open_game,  input,
      control,          state,      frame,    cancel, close_game, extension};
  *out = version == 3 ? &api : nullptr;
  return *out ? ShinkiEmuOK : ShinkiEmuUNSUPPORTED;
}
