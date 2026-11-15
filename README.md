# Glint

<img src="src-tauri/icons/128x128@2x.png" width="128" alt="Glint icon" />

一个跟随 Codex 启动和退出的 macOS 额度浮层。

没有菜单栏图标、Dock 图标、设置页或账户配置。只显示额度、重置时间和可用重置次数。

## 安装

1. 从 [Releases](https://github.com/Evil4os/glint/releases/latest) 下载 `glint-*-macos-arm64-adhoc.zip`（Apple Silicon，macOS 15.2 或更新版本）。
2. 将 `Glint.app` 放入 `/Applications` 或 `~/Applications`，打开一次。
3. 已登录的 Codex 打开后，额度会自动出现在边栏；点击查看详情，移开鼠标收起。

首次打开会为当前用户注册一个本地监听程序。此后 Codex 启动时自动启动额度程序，Codex 退出后额度主程序也退出，通常在约 2 秒内响应。切换到其他应用只隐藏浮层，不退出主程序。监听程序不加载 WebKit、不读取登录信息、不请求网络，也没有界面。若主程序意外退出，会在 Codex 仍运行时重新启动；连续启动至少间隔 15 秒。

应用文件仍有名称和极简图标，因此 Finder、Spotlight 或启动器可能搜索到它。后台运行不等于隐藏安装记录。

**当前本地包仅做 ad-hoc 签名，尚未 Developer ID 签名和 Apple 公证。** 下载到另一台 Mac 后可能被 Gatekeeper 拦截，不能承诺无提示首次运行。本项目选择不做公证。确认下载来源可信后，可按 [Apple 的说明](https://support.apple.com/zh-cn/102445)，在首次尝试打开后前往“系统设置 → 隐私与安全性 → 仍要打开”。不同系统和管理策略可能限制此操作，不需要关闭系统整体安全保护。

## 使用条件

- 本版仅支持 macOS，当前分享包为 Apple Silicon（M 系列）。
- 需要同一台 Mac 上的 Codex 登录状态；未登录时不会虚构额度。
- 普通额度读取不需要另配 API key。重置功能需要本机可用且兼容的 Codex CLI；必须点击次数并二次确认才会使用重置卡。
- 启动时不弹出辅助功能授权请求。基础贴附无需该权限；已授权时会额外检测重叠位置的 Codex 更新控件并临时让位。未授权时无法保证自动避让更新按钮。
- 位置根据 Codex 当前侧栏布局计算，未来布局变化可能需要适配。

## 停止与卸载

先在终端运行（如果装在个人 Applications 文件夹，请相应替换路径）：

```sh
"/Applications/Glint.app/Contents/MacOS/glint" --uninstall-watcher
```

随后退出 Codex，并在“活动监视器”中结束仍在运行的 `glint` 主进程，再把应用移到废纸篓。卸载监听只移除 `~/Library/LaunchAgents/app.quotafloat.desktop.watch.plist`；不会删除 Codex 登录、额度数据或聊天记录。

## 数据与隐私

额度直接读取本机 Codex 登录状态并向额度服务请求，不估算 token 消耗。无分析统计、广告或开发者收集服务器。重置操作使用官方 Codex App Server 协议；不自动消耗重置卡。详细说明见 [PRIVACY.md](PRIVACY.md)。

## 开发与构建

需要 macOS、Node.js 22+、Rust stable 和 Xcode Command Line Tools。

```sh
npm ci
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npm run tauri -- build --target aarch64-apple-darwin --bundles app
python3 scripts/package-macos.py
```

`npm run dev` 使用模拟额度。开发／构建目录里的应用不会自动注册后台监听。发布包只包含应用、许可、安装说明和校验值，不包含本机配置、登录信息、QA 截图或备份。

图标源文件为 `src-tauri/icons/source.svg`，使用 `npm run tauri -- icon` 可重新生成；打包只引用 macOS 图标。`scripts/export-source.py` 会生成独立、干净的源码分享包，不包含工作区的本机产物。

## 来源与许可

本版基于 [change-42-yhmm/quota-float](https://github.com/change-42-yhmm/quota-float) 修改，保留 MIT 许可与原有版权声明，详见 [LICENSE](LICENSE) 和 [NOTICE](NOTICE)。这是独立社区项目，并非 OpenAI 官方产品。

更名兼容：内部应用标识及监听任务标识沿用 `app.quotafloat.desktop`，以保留既有状态；显示名称、图标和可执行文件均为 Glint。图标采用暖白底、柔蓝渐变与带星芒的小光点，不使用 OpenAI 官方标志。
