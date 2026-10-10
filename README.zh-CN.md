# GPUI Component

[English](./README.md) | [简体中文](./README.zh-CN.md)

[![Build Status](https://github.com/longbridge/gpui-component/actions/workflows/ci.yml/badge.svg)](https://github.com/longbridge/gpui-component/actions/workflows/ci.yml) [![Docs](https://docs.rs/gpui-component/badge.svg)](https://docs.rs/gpui-component/) [![Crates.io](https://img.shields.io/crates/v/gpui-component.svg)](https://crates.io/crates/gpui-component)

基于 [GPUI](https://gpui.rs) 构建出色桌面应用程序的 UI 组件库。

## 特性

- **丰富性**：60+ 个跨平台桌面 UI 组件。
- **原生体验**：遵循 macOS 和 Windows 的组件交互设计，结合 shadcn/ui 设计，带来现代化体验。
- **易于使用**：无状态 `RenderOnce` 组件，简单易用。
- **可定制**：内置 `Theme` 和 `ThemeColor`，支持多主题和基于变量的配置。
- **多尺寸支持**：支持 `xs`、`sm`、`md` 和 `lg` 等尺寸。
- **灵活布局**：支持面板排列、调整大小和自由布局（Tiles）的 Dock 布局系统。
- **高性能**：虚拟化的 Table 和 List 组件，支撑海量数据的流畅渲染。
- **内容渲染**：完全 Native 的高性能 Markdown 和 HTML 渲染。
- **图表**：丰富的图表组件，用于可视化数据。
- **编辑器**：高性能代码编辑器（支持最多 200K 行稳定性能），集成 LSP（诊断、补全、悬停提示等）。
- **语法高亮**：基于 Tree Sitter 的 Editor 和 Markdown 组件的语法高亮。

### 输入重排

当字体、换行或视口几何发生变化时，原生 `Input` 现在会保留已聚焦、
字节位置未变的空选区光标所在文本行，前提是此前排版的真实端点和光标位于
输入框自身的文本视口内。即使行高不变，也能检测字体变化。此前，选区不变
可能导致旧的像素滚动偏移继续生效，使缩放或收窄后的光标缓冲行落在排版
范围之外。

修正会先确定原生可见范围，再排版；使用新排版坐标进行最小的整行／光标
可见性调整，并与滚动持久化共用原生范围钳制。没有新增公开 API 或输入
持有状态，也不会重置值、选区、焦点或撤销历史。手动滚轮阅读和待处理的
显式滚动意图优先；非空／反向选区不会被视为新的光标跟随请求。右对齐
光标指示器被钳制到视口内，并不意味着屏外的逻辑端点应将头部阅读位置
拉回尾部。

**消费端验证：** Kagi 的十三个 macOS 原生消费端场景全部通过，包括三个
已有 PR 检验和十个定向重排场景。覆盖英文／日文长 PR 草稿的实际缩放
（1111 至 1670 千分比），重排期间不使用 End／重新聚焦／重置修复；验证
新排版的 END 字形／光标查询边界，以及固定高度多行输入、带行号代码
输入、三行自动增高、行高不变的字体变化、仅宽度变化引起的换行、
手动／延迟滚动意图、非空选区的两个方向、撤销／重做，以及右对齐单行
输入在宽度从 420 变为 300 时保持头部阅读。真实 Editor 场景验证新字形
位于窗口内；真实 Issues 场景验证值、焦点、草稿订阅和撤销／重做。

**限制：** 这些场景不证明私有绘制光标的软换行亲和性（公开几何使用
false affinity）、Issues 私有光标／字形几何、Editor 自身视口裁剪、
原生折叠／幽灵文本／IME／只读行为、性能，或默认应用的 Tier B 目视
检查。此前的父级裁剪未被保存；保留判定使用此前输入框自身的文本视口。
整行可见性要求视口高度至少能容纳一行文本。

## Showcase

https://longbridge.github.io/gpui-component/gallery/

我们基于 GPUI Component 构建的商业应用：[Longbridge Pro](https://longbridge.com/desktop)。

<img width="1763" alt="Image" src="https://github.com/user-attachments/assets/e1ecb9c3-2dd3-431e-bd97-5a819c30e551" />

## Usage

```toml
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit"] }
gpui-component = { git = "https://github.com/longbridge/gpui-component" }
```

### Examples

```rs
use gpui::*;
use gpui_component::{button::*, *};

pub struct HelloWorld;
impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .items_center()
            .justify_center()
            .child("Hello, World!")
            .child(
                Button::new("ok")
                    .primary()
                    .label("Let's Go!")
                    .on_click(|_, _, _| println!("Clicked!")),
            )
    }
}

fn main() {
    gpui_platform::application().run(move |cx| {
        // 使用任何 GPUI Component 功能之前必须先调用此函数。
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| HelloWorld);
                // 窗口的第一层应该是一个 Root。
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
```

### Assets

GPUI Component 提供了 `Icon` 元素，但默认不包含 SVG 文件。

示例使用 [Lucide](https://lucide.dev) 图标，但你可以使用任意喜欢的图标。只需按照 [IconName](https://github.com/longbridge/gpui-component/blob/main/crates/ui/src/icon.rs#L86) 中的定义命名 SVG 文件，然后将所需图标添加到项目中即可。

## Development

### Story

`story` crate 是一个展示所有可用组件的画廊应用程序，通过以下命令运行：

```bash
cargo run
```

### Examples

一些重要的示例内置在 `story` crate 中，可以直接运行：

```bash
# 支持 LSP 和语法高亮的代码编辑器
cargo run --example editor

# Dock 布局系统（面板、分割视图、标签页）
cargo run --example dock

# Markdown 渲染
cargo run --example markdown

# HTML 渲染
cargo run --example html
```

`examples` 目录还包含独立示例，每个示例专注于单一功能。每个示例是一个独立的 crate，使用 `cargo run -p <name>` 运行：

```bash
# 基础 Hello World
cargo run -p hello_world

# 系统监控器（实时 CPU/内存数据图表）
cargo run -p system_monitor

# 窗口标题自定义
cargo run -p window_title
```

### Web with WASM

你也可以通过 WASM 在浏览器中运行 Gallery：

```bash
cd crates/story-web

# 安装依赖（仅首次）
make install

# 构建并运行开发服务器
make dev
```

画廊将在 http://localhost:3000 上可用。

更多详情请参阅 [crates/story-web/README.md](crates/story-web/README.md)。

查看 [CONTRIBUTING.md](CONTRIBUTING.md) 了解更多详情。

## 与其他框架对比

| 特性                | GPUI Component       | [Iced]             | [egui]                | [Qt 6]                                            |
| ------------------- | -------------------- | ------------------ | --------------------- | ------------------------------------------------- |
| 语言                | Rust                 | Rust               | Rust                  | C++/QML                                           |
| 核心                | GPUI                 | wgpu               | wgpu                  | QT                                                |
| 许可证              | Apache 2.0           | MIT                | MIT/Apache 2.0        | [Commercial/LGPL](https://www.qt.io/qt-licensing) |
| 最小二进制大小 [^1] | 12MB                 | 11MB               | 5M                    | 20MB [^2]                                         |
| 跨平台              | 是                   | 是                 | 是                    | 是                                                |
| 文档                | 一般                 | 一般               | 一般                  | 良好                                              |
| Web 支持            | 是（WASM）           | 是                 | 是                    | 是                                                |
| UI 风格             | 现代                 | 基础               | 基础                  | 基础                                              |
| CJK 支持            | 是                   | 是                 | 差                    | 是                                                |
| Chart               | 是                   | 否                 | 否                    | 是                                                |
| Table（大数据集）   | 是<br>（虚拟行、列） | 否                 | 是<br>（虚拟行）      | 是<br>（虚拟行、列）                              |
| Table 列宽调整      | 是                   | 否                 | 是                    | 是                                                |
| 文本基础            | Rope                 | [COSMIC Text] [^3] | trait TextBuffer [^4] | [QTextDocument]                                   |
| Code Editor         | 简单                 | 简单               | 简单                  | 基础 API                                          |
| Dock 布局           | 是                   | 是                 | 是                    | 是                                                |
| 语法高亮            | [Tree Sitter]        | [Syntect]          | [Syntect]             | [QSyntaxHighlighter]                              |
| Markdown 渲染       | 是                   | 是                 | 基础                  | 否                                                |
| Markdown 混合 HTML  | 是                   | 否                 | 否                    | 否                                                |
| HTML 渲染           | 基础                 | 否                 | 否                    | 基础                                              |
| 文本选择            | TextView             | 否                 | 任意 Label            | 是                                                |
| 自定义主题          | 是                   | 是                 | 是                    | 是                                                |
| 内置主题            | 是                   | 否                 | 否                    | 否                                                |
| 国际化              | 是                   | 是                 | 是                    | 是                                                |

> 如发现任何错误或过时信息，请提交 issue 或 PR。

[Iced]: https://github.com/iced-rs/iced
[egui]: https://github.com/emilk/egui
[QT 6]: https://www.qt.io/product/qt6
[Tree Sitter]: https://tree-sitter.github.io/tree-sitter/
[Syntect]: https://github.com/trishume/syntect
[QSyntaxHighlighter]: https://doc.qt.io/qt-6/qsyntaxhighlighter.html
[QTextDocument]: https://doc.qt.io/qt-6/qtextdocument.html
[COSMIC Text]: https://github.com/pop-os/cosmic-text

[^1]: 使用简单 Hello World 示例的 Release 构建。

[^2]: [减小 Qt 应用程序的二进制大小](https://www.qt.io/blog/reducing-binary-size-of-qt-applications-part-3-more-platforms)

[^3]: Iced Editor: <https://github.com/iced-rs/iced/blob/db5a1f6353b9f8520c4f9633d1cdc90242c2afe1/graphics/src/text/editor.rs#L65-L68>

[^4]: egui TextBuffer: <https://github.com/emilk/egui/blob/0a81372cfd3a4deda640acdecbbaf24bf78bb6a2/crates/egui/src/widgets/text_edit/text_buffer.rs#L20>

## 许可证

Apache-2.0

- UI 设计基于 [shadcn/ui](https://ui.shadcn.com)，部分来自 [Reui](https://reui.io)。
- 图标来自 [Lucide](https://lucide.dev)。
