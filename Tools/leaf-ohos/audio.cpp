#include "sdl_audio.hpp"
#include <ohaudio/native_audiorenderer.h>
#include <ohaudio/native_audiostreambuilder.h>
#include <atomic>
#include <cstring>

namespace wa2::platform {
struct SdlAudio::Impl {
    audio::Mixer &mixer;
    OH_AudioRenderer *renderer{};
    std::atomic<uint64_t> calls{}, frames{}, errors{};
    std::atomic<bool> paused{};
    std::string error;
    explicit Impl(audio::Mixer &value) : mixer(value) {}
    static OH_AudioData_Callback_Result fill(OH_AudioRenderer *, void *opaque, void *buffer, int32_t bytes) {
        auto &self = *static_cast<Impl *>(opaque);
        if (bytes <= 0) return AUDIO_DATA_CALLBACK_RESULT_VALID;
        const auto count = size_t(bytes) / sizeof(float);
        if (self.paused || count % 2) std::memset(buffer, 0, bytes);
        else try {
            self.mixer.render(std::span(static_cast<float *>(buffer), count), 48000);
            self.frames += count / 2;
            ++self.calls;
        } catch (...) {++self.errors; std::memset(buffer, 0, bytes);}
        return AUDIO_DATA_CALLBACK_RESULT_VALID;
    }
};
SdlAudio::SdlAudio(audio::Mixer &mixer) : impl_(std::make_unique<Impl>(mixer)) {}
SdlAudio::~SdlAudio() {close();}
bool SdlAudio::open(const std::string &) {
    auto &state = *impl_;
    if (state.renderer) return true;
    OH_AudioStreamBuilder *builder{};
    auto result = OH_AudioStreamBuilder_Create(&builder, AUDIOSTREAM_TYPE_RENDERER);
    if (!result) result = OH_AudioStreamBuilder_SetSamplingRate(builder, 48000);
    if (!result) result = OH_AudioStreamBuilder_SetChannelCount(builder, 2);
    if (!result) result = OH_AudioStreamBuilder_SetSampleFormat(builder, AUDIOSTREAM_SAMPLE_F32LE);
    if (!result) result = OH_AudioStreamBuilder_SetRendererInfo(builder, AUDIOSTREAM_USAGE_GAME);
    if (!result) result = OH_AudioStreamBuilder_SetRendererWriteDataCallback(builder, Impl::fill, &state);
    if (!result) result = OH_AudioStreamBuilder_GenerateRenderer(builder, &state.renderer);
    if (builder) OH_AudioStreamBuilder_Destroy(builder);
    if (!result) result = OH_AudioRenderer_Start(state.renderer);
    if (result) { close(); state.error = "OHAudio " + std::to_string(result); return false; }
    state.error.clear();
    return true;
}
void SdlAudio::pause(bool paused) {impl_->paused = paused;}
void SdlAudio::close() {
    if (!impl_->renderer) return;
    OH_AudioRenderer_Stop(impl_->renderer);
    OH_AudioRenderer_Release(impl_->renderer);
    impl_->renderer = nullptr;
}
std::vector<std::string> SdlAudio::devices() {return {};}
AudioStatus SdlAudio::status() const {return {48000, 0, impl_->calls, impl_->frames, impl_->errors, "OHAudio", impl_->error};}
}
