---
title: TextView
description: 渲染 Markdown 与 HTML 文本，并支持自定义 Markdown 插件。
---

# TextView

`TextView` 用于在 GPUI 中渲染格式化文本。它支持 Markdown、简单 HTML、文本选择、代码块操作，以及通过 Markdown 插件解析和渲染项目自定义语法。

## 导入

```rust
use gpui_component::text::{markdown, TextView};
```

## 用法

### Markdown

只需要渲染 Markdown 时，可以使用 `markdown` helper：

```rust
use gpui_component::text::markdown;

markdown("# Hello\n\nThis is **Markdown**.")
    .selectable(true)
    .scrollable(true)
```

如果需要稳定 id，也可以直接构造 `TextView`：

```rust
use gpui_component::text::TextView;

TextView::markdown("preview", markdown_source)
    .selectable(true)
```

### HTML

```rust
TextView::html("html-preview", "<strong>Hello</strong>")
```

## 跨虚拟视图的托管选择

有序会话为每个已接受的帖子保留 `Entity<TextViewState>`，使用
`TextView::new(&state).selectable(true)` 渲染。已接受的解析结果拥有统一的
渲染文本流；复制不要求每个帖子或巨大帖子的每一部分都已经绘制。
相同 state 和完全相同的 source 会复用解析；字节长度相同不代表 source 相同。

在内容接受或激活边界调用
`Root::set_text_selection_group(Some((group_id, &members)), cx)`。
`members` 是有序的 `(ElementId, Entity<TextViewState>)` 切片。
consumer ID 必须唯一且与真实帖子及其保留的 text entity 绑定，不可使用行号、
author/time metadata 或正文作为身份。重复 ID 或重复 entity 会被拒绝。
每个 Root 只有一个托管组，没有全局会话缓存。

活跃 consumer 必须保留返回的 `TextSelectionGroupRetirement` lease。
Root 只保留 text entity 和 lease owner 的弱引用。未改变的绑定返回相同活跃
lease 的 clone；`retire()`、最后一个 consumer clone 的 drop 或
`set_text_selection_group(None, cx)` 会同步撤销选择、原生几何、viewport 和
边缘滚动 task，不需要下一次绘制。lease 的撤销不需要 App/Window；
旧 lease 不能撤销替代绑定。owner/页面离开或 modal 接管时撤销，不在 render 中安装。

用 `TextSelectionGroupElement::text_selection_group(group_id, scroll_callback)`
包裹唯一的外层滚动 viewport。callback 滚动该 viewport 并请求下一次 layout。
marker 和真实 text layout 提供 hit 权限，membership 本身不提供权限。
指针端点使用当前视觉行上最近的 shaped caret，并遵守 grapheme 边界，不切开
多字节字符或多码点 grapheme。caret metadata 随接受的 source 缓存，不在每次
mousemove 时重建。GPUI cached replay 保留 GPUI 自己拥有的 presence state；
controller 只有弱引用，真正 unmount 后旧坐标失去 hit 权限。

### 刷新、source 与平台边界

绑定变化总是撤销旧 hitbox、viewport、drag pointer 和 edge task，并返回新 lease。
只有旧范围当前有效、group/scope 相同、选中区间的有序 stable ID、保留 entity 和
已接受 source revision 全部不变时，才保留逻辑端点。因此选中区间外的重排、
前插或正文修改可以保留未改变帖子的选择。修改/删除选中帖子、向选中区间插入、
改变区间顺序、离开组或失去必要的弱 entity 都拒绝旧选择。
替代 viewport 绘制后 Copy 才恢复；旧几何和旧 lease 不提供权限。

端点是统一渲染文本流的 UTF-8 字节偏移，不是原始 Markdown/HTML 偏移。
保留现有 block/table 分隔符、link label、自定义节点文本和 literal code；
不会把 image destination/alt text 或隐藏 HTML 编造成可选文本。
SelectAll 和托管 Copy 共用同一遍历。width、zoom 和 style 改变只重建几何，
不改变未修改的 source 区间；source 或 Markdown plugin revision 改变另行处理。

SDK 的 `logical_selection` 及旧 `window_selection`/`text_view` 测试不跳过用例。
Input fixture 明确使用 **disabled** Input，draft/empty clipboard 的精确断言只证明
该边界。GPUI `TestWindow` 没有 enabled Input 所需的 native input-handler hook；
不可吞掉 panic，或把 disabled 测试当作 enabled 编辑的证明。
Kagi 的 `issue_conversation_enabled_input_copy` 单独使用真实 AppKit 窗口，覆盖
enabled production Reply 的 SelectAll/Copy、实际 Backspace 清空、empty Copy
保留 poison、实际 Tab 后无需重新选择/激活即可复制原会话选择，以及离开时撤销和
repository/draft 不变性。这是 private test clipboard 上的 native event simulation，
不是硬件输入、IME、Tier B 截图或完整 suite 通过的声明。

2026-10-10 Kagi 集成检查点的完整 SDK lib suite 跑了334件，failed/ignored/filtered
均为0；native Issue 会话10个场景一起通过，包含 enabled Input recovery。
两个 obsolete parser text dead-code warning 都已消失。上述运行在 formatting-only
变化之前；随后使用现有 edition/style edition2024配置对13个变更 Rust 目标作 scoped
format/check 并通过。整个 SDK workspace 的 format check 因未变更 baseline 和
当时未格式化的目标而失败，不代表全 repository format 通过。SDK lib 禁用 doctest；
post-format/公开 pin 的接受、其他 gate 和 default-app Tier B 仍需单独证据。
详细日期记录见 Kagi ADR-0198。
随后 PM 重跑 post-format 完整 SDK lib suite，仍334件通过（body0.07 s）。
UI-lib Clippy exit0，但 private native paint registration 新出现一条参数数量诊断；
局部带说明的 allowance 保留原有独立 paint 参数，不引入额外 payload type 或几何变化。
随后 exact-annotation UI-lib Clippy（PM `bg_619`）exit0，process4.19 s、
check/compile3.99 s；无新增参数数量/dead-code warning，只剩既有 dependency
future-compatibility notice。之后 exact-source SDK lib 与相同13目标的configured-2024
format check（PM `bg_620`）通过：334件、0 failed/ignored/filtered、body0.07 s，
scoped check exit0，合计process7.33 s。不宣称公开 SDK revision、
post-format native 重跑或整个 workspace format 通过。

## Markdown 插件

使用 `.plugin(...)` 支持自定义 Markdown 格式。插件同时拥有解析和渲染逻辑，调用方只需要把它挂到 `TextView` 上：

```rust
markdown(source)
    .plugin(TickerPlugin::new())
```

Markdown 插件实现 `MarkdownPlugin`：

```rust
use gpui::{App, IntoElement, ParentElement as _, Window};
use gpui_component::text::{
    markdown_ast, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
};

struct TickerNode {
    symbol: String,
}

struct TickerPlugin;

impl TickerPlugin {
    fn new() -> Self {
        Self
    }
}

impl MarkdownPlugin for TickerPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "ticker"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Paragraph(paragraph) = node else {
            return None;
        };
        let [markdown_ast::Node::Text(text)] = paragraph.children.as_slice() else {
            return None;
        };
        let symbol = text.value.strip_prefix('$')?;

        Some(
            MarkdownNode::new(
                "ticker",
                TickerNode {
                    symbol: symbol.to_string(),
                },
            )
            .text(format!("${symbol}"))
            .markdown(cx.node_source(node).unwrap_or(text.value.as_str())),
        )
    }

    fn render(
        &self,
        node: &MarkdownNode,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        let ticker = node.data::<TickerNode>().expect("ticker node data");

        gpui::div().child(format!("${}", ticker.symbol))
    }
}
```

然后挂到 Markdown `TextView`：

```rust
markdown("$AAPL.US")
    .plugin(TickerPlugin::new())
```

## MarkdownNode

`MarkdownNode` 是 `parse` 和 `render` 之间传递的中性数据结构。

```rust
MarkdownNode::new("ticker", TickerNode { symbol })
    .text("$AAPL.US")
    .markdown("$AAPL.US")
```

- `name` 是稳定的节点名称，用于匹配 renderer。
- `data` 是 parser 产生的类型化数据，通过 `node.data::<T>()` 读取。
- `text` 是纯文本表示，用于选择和未注册 renderer 时的回退渲染。
- `markdown` 是 Markdown 表示，用于将文档重新序列化为 Markdown。

## Block 插件

当前自定义 Markdown 渲染支持 block 插件。现在可注册的插件需要在 `is_block()` 中返回 `true`：

```rust
fn is_block(&self) -> bool {
    true
}
```

Inline 插件保留给未来的 `TextView` 支持。

## 代码块操作

可以为 Markdown 代码块渲染操作控件：

```rust
markdown(source)
    .code_block_actions(|code_block, _window, _cx| {
        gpui::div().child(format!("Run {}", code_block.lang().unwrap_or_default()))
    })
```
