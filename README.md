# WhatdidIsay - Real-time Speech Transcription for MicYou
# WhatdidIsay - MicYou 实时语音转录插件

WhatdidIsay is a native DSP plugin for MicYou that silently listens to your microphone and transcribes your speech into organized text records. It runs entirely offline using state-of-the-art local AI models, ensuring your privacy while keeping a perfect log of everything you say during meetings, streams, or daily work.

WhatdidIsay 是一款为 MicYou 打造的原生 DSP 插件，它会在后台静默监听麦克风，并将你的语音实时转录为结构化的文本记录。它完全依赖先进的本地 AI 模型离线运行，在完美记录你会议、直播或日常工作中的每一句话的同时，绝对保障你的隐私安全。

---

## ✨ Features | 特性

- **Real-Time Safe DSP Integration**: Employs a lock-free ring buffer and a zero-polling background thread. It never blocks the main audio pipeline, ensuring zero audio dropouts or latency.
- **实时安全的 DSP 集成**：采用无锁环形缓冲区与零轮询（Zero-polling）后台线程设计。绝不阻塞主音频链路，确保零爆音、零延迟。

- **Next-Gen Local AI**: Powered by `sherpa-onnx`, utilizing the lightweight Silero VAD for precise speech detection and the Qwen3-ASR (0.6B int8) model for blazing-fast, highly accurate transcription.
- **新一代本地 AI 引擎**：基于 `sherpa-onnx` 构建，使用轻量级的 Silero VAD 进行精准的语音活动检测（VAD），并搭载 Qwen3-ASR (0.6B int8) 模型，提供极速且高精度的转录体验。

- **Automatic Language Detection**: No need to manually select languages. The Qwen3 model automatically identifies and transcribes multiple languages seamlessly.
- **自动语言检测**：无需手动选择语言。Qwen3 模型能够自动识别并无缝转录多种语言。

- **Structured Archiving**: Automatically organizes transcripts into daily text files, grouped by minute with precise second-level timestamps.
- **结构化归档**：自动将转录内容按天保存为文本文件，按分钟分组，并附带精确到秒的时间戳。

---

## 📦 Installation & Model Setup | 安装与模型配置

To use this plugin, you need to place the compiled dynamic library and the required AI models into the MicYou plugin directory. 

要使用此插件，你需要将编译好的动态链接库以及所需的 AI 模型放入 MicYou 的插件目录中。

### Step 1: Download Models | 第一步：下载模型

Download the required models and place them in your plugin folder (e.g., `~/.config/micyou/plugins/opss.whatdidisay/` on Linux/macOS or `%APPDATA%\micyou\plugins\opss.whatdidisay\` on Windows).

下载所需模型并将其放置在你的插件文件夹中（例如 Linux/macOS 下的 `~/.config/micyou/plugins/opss.whatdidisay/` 或 Windows 下的 `%APPDATA%\micyou\plugins\opss.whatdidisay\`）。

1. **Silero VAD**: Download `silero_vad.onnx` and place it in the root of the plugin directory.
   **Silero VAD**：下载 `silero_vad.onnx` 并将其放在插件目录的根目录下。
2. **Qwen3 ASR**: Download the `sherpa-onnx-qwen3-asr-0.6B-int8` repository from HuggingFace and place the entire folder inside the plugin directory.
   **Qwen3 ASR**：从 HuggingFace 下载 `sherpa-onnx-qwen3-asr-0.6B-int8` 仓库，并将整个文件夹放入插件目录中。

Your plugin directory structure should look exactly like this:

你的插件目录结构必须严格如下所示：

```text
opss.whatdidisay/
├── plugin.json
├── WhatdidIsay.dll          # (or .so / .dylib depending on your OS)
├── silero_vad.onnx
└── sherpa-onnx-qwen3-asr-0.6B-int8/
    ├── conv_frontend.onnx
    ├── encoder.int8.onnx
    ├── decoder.int8.onnx
    └── tokenizer/
```

---

## 📝 Recording Format | 记录格式

Transcripts are saved in the `records/` subdirectory within the plugin folder. A new file is created every day (`YYYYMMDD.txt`), and entries are grouped by the minute.

转录文本保存在插件文件夹内的 `records/` 子目录中。每天会创建一个新文件（`YYYYMMDD.txt`），条目按分钟进行分组。

Example of `20260822.txt`:

`20260822.txt` 示例：

```text
14:30
	15 Hello everyone, let's start the meeting.
	45 Today we will discuss the new plugin architecture.
14:31
	10 The DSP ring buffer is fully lock-free.
```

---

## ⚙️ Configuration | 配置项

You can configure the plugin behavior via the MicYou plugin settings UI:

你可以通过 MicYou 的插件设置界面配置插件行为：

- **Model Load Timing (模型加载时机)**:
  - `MicYou Start (MicYou 启动时)`: Loads the AI models into memory as soon as MicYou launches.
  - `Device Connect (设备连接时)`: (Default) Saves memory by only loading the models when a microphone device is connected.
  - `MicYou 启动时`：在 MicYou 启动时立即将 AI 模型加载到内存中。
  - `设备连接时`：（默认）仅在麦克风设备连接时才加载模型，以节省内存。

---

## 🛠️ Building from Source | 从源码构建

If you want to compile the plugin yourself, ensure you have Rust and Cargo installed. Add `sherpa-onnx` and `chrono` to your `Cargo.toml` dependencies.

如果你想自己编译插件，请确保已安装 Rust 和 Cargo。并在 `Cargo.toml` 依赖中添加 `sherpa-onnx` 和 `chrono`。

```bash
# Clone the repository and navigate to the folder
# 克隆仓库并进入文件夹
cargo build --release

# The compiled library will be at:
# 编译后的动态库位于：
# Windows: target/release/whatdidisay.dll
# Linux:   target/release/libwhatdidisay.so
# macOS:   target/release/libwhatdidisay.dylib
```

Copy the compiled binary to your plugin directory and rename it to match the `entry` field in `plugin.json` (e.g., `WhatdidIsay.dll`).

将编译好的二进制文件复制到你的插件目录，并重命名以匹配 `plugin.json` 中的 `entry` 字段（例如 `WhatdidIsay.dll`）。

---

## 📄 License | 许可证

This plugin is released under the Unlicense. Do whatever you want with it.

本插件基于 Unlicense 发布。你可以随心所欲地使用它。