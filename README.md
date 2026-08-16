# Native 插件骨架

## 构建
`cargo build --release`，产物 target/release/lib*.so 复制到插件目录并改名与
plugin.json 的 entry 一致

## 说明
native 插件拥有宿主完整权限，用于实时 DSP、硬件与深度系统集成；
普通逻辑/UI 优先使用 wasm 插件（沙箱安全）

## 能力
按需声明 capabilities；process() 内禁止调用宿主 API（实时安全）
