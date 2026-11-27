// Persistent Piper worker. Requests are UTF-8 lines: speed, volume, then text.
// Responses are a little-endian length and a PCM WAV. Zero means synthesis failed.
#include "sherpa-onnx/c-api/c-api.h"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#ifdef _WIN32
  #define NOMINMAX
  #include <windows.h>
  #include <filesystem>
  #include <fcntl.h>
  #include <io.h>
#else
  #include <dlfcn.h>
  #include <sys/resource.h>
#endif

int main() {
    std::ios::sync_with_stdio(false);
#ifdef _WIN32
    if (_setmode(_fileno(stdout), _O_BINARY) == -1) return 1;
    const auto path = std::filesystem::absolute(L"sherpa-onnx-c-api.dll");
    const auto dll = LoadLibraryExW(path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    const auto symbol = [dll](const char* name) { return GetProcAddress(dll, name); };
#else
    setpriority(PRIO_PROCESS, 0, 5);
    // Load ONNX Runtime first by path so its SONAME resolves when loading sherpa-onnx.
    if (!dlopen("./libonnxruntime.so", RTLD_NOW | RTLD_LOCAL)) return 1;
    const auto dll = dlopen("./libsherpa-onnx-c-api.so", RTLD_NOW | RTLD_LOCAL);
    const auto symbol = [dll](const char* name) { return dlsym(dll, name); };
#endif
    if (!dll) return 1;
    const auto create = reinterpret_cast<decltype(&SherpaOnnxCreateOfflineTts)>(symbol("SherpaOnnxCreateOfflineTts"));
    const auto destroy = reinterpret_cast<decltype(&SherpaOnnxDestroyOfflineTts)>(symbol("SherpaOnnxDestroyOfflineTts"));
    const auto generate = reinterpret_cast<decltype(&SherpaOnnxOfflineTtsGenerate)>(symbol("SherpaOnnxOfflineTtsGenerate"));
    const auto free_audio = reinterpret_cast<decltype(&SherpaOnnxDestroyOfflineTtsGeneratedAudio)>(
        symbol("SherpaOnnxDestroyOfflineTtsGeneratedAudio"));
    if (!create || !destroy || !generate || !free_audio) return 1;
    SherpaOnnxOfflineTtsConfig config{};
    config.model.vits.model = "model/en_US-kristin-medium.onnx";
    config.model.vits.tokens = "model/tokens.txt";
    config.model.vits.data_dir = "model/espeak-ng-data";
    config.model.vits.length_scale = 1.0f;
    config.model.vits.noise_scale = 0.667f;
    config.model.vits.noise_scale_w = 0.8f;
    config.model.num_threads = 2;
    config.model.provider = "cpu";
    config.max_num_sentences = 1;
    using Engine = std::unique_ptr<const SherpaOnnxOfflineTts, decltype(&SherpaOnnxDestroyOfflineTts)>;
    Engine engine(create(&config), destroy);
    if (!engine) return 1;

    for (std::string line; std::getline(std::cin, line);) {
        std::istringstream fields(line);
        float speed = 1.0f, volume = 1.0f;
        std::string phrase;
        if (!(fields >> speed >> volume) || !std::getline(fields >> std::ws, phrase)) return 2;
        using Audio = std::unique_ptr<const SherpaOnnxGeneratedAudio, decltype(&SherpaOnnxDestroyOfflineTtsGeneratedAudio)>;
        Audio audio(generate(engine.get(), phrase.c_str(), 0, speed), free_audio);
        const auto write = [](auto value) {
            std::cout.write(reinterpret_cast<const char*>(&value), sizeof(value));
        };
        if (!audio || !audio->samples || audio->n <= 0 || audio->n > 2000000 || audio->sample_rate <= 0) {
            write(uint32_t{0});
        } else {
            const auto rate = static_cast<uint32_t>(audio->sample_rate);
            const auto size = static_cast<uint32_t>(audio->n) * 2;
            write(size + 44);
            std::cout.write("RIFF", 4);
            write(size + 36);
            std::cout.write("WAVEfmt ", 8);
            write(uint32_t{16});
            write(uint16_t{1}); // PCM
            write(uint16_t{1}); // mono
            write(rate);
            write(rate * 2);
            write(uint16_t{2});
            write(uint16_t{16});
            std::cout.write("data", 4);
            write(size);
            for (int i = 0; i < audio->n; ++i) {
                const auto sample = audio->samples[i] * volume;
                write(static_cast<int16_t>(std::isfinite(sample) ? std::clamp(sample, -1.0f, 1.0f) * 32767 : 0));
            }
        }
        std::cout.flush();
        if (!std::cout) return 1;
    }
}
