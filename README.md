NO RELEASES PROVIDED UNTIL MICYOU LICENSE ALLOWED PLUGIN LICENSES BESIDES GPL
DEVELOPING REPO FOR NOW
# WhatdidIsay - MicYou Voice Transcription Plugin
# WhatdidIsay - MicYou 语音转录插件

This is a MicYou plugin that transcribes your speech into text records every time you use MicYou. All transcription records are safely stored in the `records` directory within the plugin's installation folder, neatly organized by date and time.

这是一个 MicYou 插件，能在你每次使用 MicYou 时自动将你的语音转录为文本记录。所有的转录记录都会安全地保存在插件目录下的 `records` 文件夹中，并按日期和时间进行 neatly 归档。

***

The current version is **not** recommended for general daily use. It is strictly intended for the following three types of users:

当前版本**不建议**普通用户作为日常工具使用。它仅适合以下三类人群：

## **1. I NEED IT RIGHT NOW.**
## **1. 我急着用。**

2\. I am developing a MicYou plugin and want to see how others implement it.

2\. 我在开发 MicYou 插件，想看看别人是怎么做的。

3\. I don't mind having a 3GB AI model running in the background at all times.

3\. 我觉得在后台一直挂着一个 3GB 的 AI 模型完全没有问题。

***

**How to Use:** Install and enable this plugin via the plugin system in the MicYou Desktop Client (version 2.0.0 or above). Upon its first run, it will automatically pull the `faster-whisper-large-v2` model from ModelScope. Once the model is fully loaded, it will begin transcribing your audio and outputting the results to the `records` folder.

**如何使用：** 在 MicYou 桌面端（2.0.0 及以上版本）的插件系统中安装并启用本插件。首次运行时，程序会自动从 ModelScope 拉取 `faster-whisper-large-v2` 模型。当模型加载完成后，即可开始识别语音并将结果输出到 `records` 目录。

***

**Hardware Acceleration:** If you wish to enable hardware acceleration (e.g., CUDA), please follow the official guidelines at [SYSTRAN/faster-whisper](https://github.com/SYSTRAN/faster-whisper) to configure your local environment. Afterward, you can manually adjust the model loading parameters in the `main.py` file: 
`model = WhisperModel(model_path, device=args.device, compute_type=args.compute_type)`

**如何启用硬件加速：** 如果你希望开启硬件加速，请先根据 [SYSTRAN/faster-whisper](https://github.com/SYSTRAN/faster-whisper) 的官方指引配置好本地的硬件加速环境。随后，你可以在 `main.py` 文件中手动调整模型加载参数：
`model = WhisperModel(model_path, device=args.device, compute_type=args.compute_type)`

***

**Future Plans:** The current underlying model used by this plugin is quite large and heavy. In future updates, the model may be swapped for better performance, and recommendations for alternative model solutions are highly welcome.

**未来计划：** 本插件当前使用的底层模型较大且重。在后续的更新中，本插件计划更换底层模型以获得更好的性能，同时也非常欢迎推荐更优的模型方案。