# WhatdidIsay - Real-time Speech Transcription for MicYou
# WhatdidIsay - MicYou 实时语音转录插件

WhatdidIsay is a native DSP plugin for MicYou that silently listens to your microphone and transcribes your speech into organized text records. It runs entirely offline using state-of-the-art local AI models, ensuring your privacy while keeping a perfect log of everything you say during meetings, gaming, or vibe coding.

WhatdidIsay 是一款为 MicYou 打造的原生 DSP 插件，它会在后台静默监听麦克风，并将你的语音实时转录为结构化的文本记录。它完全依赖本地 AI 模型离线运行，在完美记录你开会、开黑、vibe coding 的每一句话的同时，绝对保障你的隐私安全。

## ✨ Features | 特性

- **Next-Gen Local AI**: Powered by `sherpa-onnx`, utilizing the lightweight Silero VAD for precise speech detection and the Qwen3-ASR (0.6B int8) model for blazing-fast, highly accurate transcription.
  **新一代本地 AI 引擎**：基于 `sherpa-onnx` 构建，使用轻量级的 Silero VAD 进行精准的语音活动检测（VAD），并搭载 Qwen3-ASR (0.6B int8) 模型，提供极速且高精度的转录体验。
- **Automatic Language Detection**: No need to manually select languages. The Qwen3 ASR model automatically identifies and transcribes multiple languages seamlessly.
  **自动语言检测**：无需手动选择语言。Qwen3 ASR 模型能够自动识别并无缝转录多种语言。
- **Structured Archiving**: Automatically organizes transcripts into daily text files, grouped by minute with precise second-level timestamps.
  **结构化归档**：自动将转录内容按天保存为文本文件，按分钟分组，并附带精确到秒的时间戳。
- **Cross-Plugin Broadcast Service**: Acts as a system-wide speech-to-text provider. Other plugins can effortlessly subscribe to real-time transcription broadcasts via a lightweight binary protocol to build voice-controlled features.
  **跨插件广播服务**：作为全局语音识别服务提供者。其他插件可通过轻量的二进制协议，轻松订阅实时转录广播，用于开发语音输入、语音指令等功能。

## 📦 Installation | 安装

### Option 1: One-Click Install via MicYou (Recommended) | 方式一：通过 MicYou 一键安装（推荐）

The official release package comes with **all AI models pre-bundled**. You do not need to download models or configure files manually.
官方发布的 Release 压缩包已**内置全部 AI 模型**。你无需手动下载模型或配置任何文件。

1. Go to the [Releases](../../releases) page and download the latest `whatdidisay-vX.X.X.zip`.
   前往 [Releases](../../releases) 页面下载最新的 `whatdidisay-vX.X.X.zip`。
2. Open MicYou, navigate to **Settings -> Plugins**.
   打开 MicYou，进入 **设置 -> 插件**。
3. Click the **Import Plugin** button, select the downloaded `.zip` file, and enable the plugin. Done!
   点击 **导入插件** 按钮，选择下载的 `.zip` 文件，然后启用插件即可。开箱即用！

### Option 2: Build from Source (For Developers) | 方式二：从源码构建（面向开发者）

> ⚠️ **Note:** Manual model installation is required **only if you compile the plugin yourself**. Release users can skip this entirely.
> ⚠️ **注意：** 只有当你**自行编译插件**时才需要手动安装模型。使用 Release 版本的用户可完全跳过此步骤。

1. **Build the library | 编译动态库**:
   Ensure you have Rust and Cargo installed. Add `sherpa-onnx` and `chrono` to your `Cargo.toml`.
   确保已安装 Rust 和 Cargo，并在 `Cargo.toml` 中添加 `sherpa-onnx` 和 `chrono` 依赖。
   ```bash
   cargo build --release
   ```
   *The compiled binary will be at `target/release/whatdidisay.dll` (Windows), `.so` (Linux), or `.dylib` (macOS).*

2. **Download Models Manually | 手动下载模型**:
   Create a folder named `opss.whatdidisay` in your MicYou plugins directory (e.g., `%APPDATA%\micyou\plugins\` on Windows). Place your compiled binary, `plugin.json`, and the following models inside:
   在 MicYou 插件目录下（如 Windows 的 `%APPDATA%\micyou\plugins\`）创建 `opss.whatdidisay` 文件夹。将编译产物、`plugin.json` 以及以下模型放入其中：
   - **Silero VAD**: Download `silero_vad.onnx` to the root of the folder.
   - **Qwen3 ASR**: Download the `sherpa-onnx-qwen3-asr-0.6B-int8` repository from HuggingFace into the folder.

   **Required Directory Structure | 必须的目录结构**:
   ```text
   opss.whatdidisay/
   ├── plugin.json
   ├── WhatdidIsay.dll          # (or .so / .dylib)
   ├── silero_vad.onnx
   └── sherpa-onnx-qwen3-asr-0.6B-int8/
       ├── conv_frontend.onnx
       ├── encoder.int8.onnx
       ├── decoder.int8.onnx
       ├── LICENSE
       ├── README.md
       └── tokenizer/
   ```

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

## ⚙️ Configuration | 配置项

You can configure the plugin behavior via the MicYou plugin settings UI:
你可以通过 MicYou 的插件设置界面配置插件行为：

- **Model Load Timing (模型加载时机)**:
  - `MicYou Start (MicYou 启动时)`: Loads the AI models into memory as soon as MicYou launches.
  - `Device Connect (设备连接时)`: *(Default)* Saves memory by only loading the models when a microphone device is connected.
- **Model Status Notification (模型状态通知)**:
  - Toggle to receive system desktop notifications when the AI models finish loading or start unloading.
  - 开启后，将在 AI 模型加载完成或开始卸载时收到系统桌面通知。

## 🔌 Cross-Plugin Messaging | 跨插件消息通信

WhatdidIsay acts as a system-wide speech-to-text service for other MicYou plugins. Instead of loading heavy AI models in every plugin, developers can simply subscribe to the transcription broadcasts via the host's internal Message Bus.
WhatdidIsay 可作为全局语音识别服务，为其他 MicYou 插件提供能力。开发者无需在自己的插件中加载庞大的 AI 模型，只需通过宿主内部的消息总线订阅转录广播即可。

### Binary Protocol | 二进制协议
When a sentence is transcribed, WhatdidIsay broadcasts a binary message. Consumers only need to check the Magic Number to parse it, avoiding any JSON overhead.
当一句话被转录后，WhatdidIsay 会广播一条二进制消息。消费者只需校验 Magic Number 即可进行切片解析，避免了任何 JSON 解析开销。

- **Magic Number (4 Bytes)**: `b"WDIS"`
- **Start Timestamp (8 Bytes)**: `i64` (Little-Endian), Unix milliseconds when the speech started.
- **End Timestamp (8 Bytes)**: `i64` (Little-Endian), Unix milliseconds when the speech ended.
- **Text (Variable)**: UTF-8 encoded transcribed text.

### Usage Example (Rust) | 使用示例 (Rust)
Other plugins can listen to this by implementing `micyou_plugin_handle_message`:
其他插件可以通过实现 `micyou_plugin_handle_message` 来监听：

```rust
#[no_mangle]
pub extern "C" fn micyou_plugin_handle_message(
    source: *const c_char, topic: *const c_char, payload: *const u8, payload_len: u32
) -> mpl_result_t {
    unsafe {
        let len = payload_len as usize;
        let data = std::slice::from_raw_parts(payload, len);
        
        // Check Magic Number | 校验 Magic Number
        if len >= 20 && &data[0..4] == b"WDIS" {
            let start_ms = i64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
            let end_ms = i64::from_le_bytes(data[12..20].try_into().unwrap_or_default());
            let text = std::str::from_utf8(&data[20..]).unwrap_or("");
            
            println!("Transcribed: [{} -> {}] {}", start_ms, end_ms, text);
            // Handle the text, e.g., trigger voice commands | 处理文本，例如触发语音指令
        }
    }
    mpl_result_t::MPL_OK
}
```

## 🚀 Roadmap | 未来计划

- [ ] **Streaming ASR Support | 流式识别支持**: Add a streaming recognition option once `sherpa-onnx` fully supports streaming inference for the Qwen3 ASR model.
  在 `sherpa-onnx` 落实 Qwen3 ASR 模型的流式推理后，增加流式识别选项。
- [ ] **Android Companion Plugin | 安卓端配套插件**: Create a companion plugin for the upcoming MicYou Android client to display transcription results in real-time on mobile devices.
  在 MicYou 安卓端插件系统可用后，制作安卓端的配合插件，将识别结果在安卓端实时展示。

## 📄 License | 许可证

This plugin is released under the Unlicense. Do whatever you want with it.
本插件基于 Unlicense 发布。你可以随心所欲地使用它。