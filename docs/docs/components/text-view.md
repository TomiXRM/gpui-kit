---
title: TextView
description: Renders Markdown and HTML text with optional custom Markdown plugins.
---

# TextView

`TextView` renders formatted text in GPUI. It supports Markdown and simple HTML, text selection, code block actions, and custom Markdown plugins for project-specific syntax.

## Import

```rust
use gpui_component::text::{markdown, TextView};
```

## Usage

### Markdown

Use the `markdown` helper when you only need to render Markdown text:

```rust
use gpui_component::text::markdown;

markdown("# Hello\n\nThis is **Markdown**.")
    .selectable(true)
    .scrollable(true)
```

You can also construct a `TextView` directly when you need a stable id:

```rust
use gpui_component::text::TextView;

TextView::markdown("preview", markdown_source)
    .selectable(true)
```

### HTML

```rust
TextView::html("html-preview", "<strong>Hello</strong>")
```

## Managed selection across virtualized views

For an ordered conversation, retain an `Entity<TextViewState>` for each accepted
post and render it with `TextView::new(&state).selectable(true)`. The accepted
parse owns one canonical rendered-text stream; Copy does not depend on every
post or every part of a large post having been painted. Reusing the same state
and exact source avoids repeating an unchanged parse. Equal byte length is not
source equality.

At an accepted-content or activation seam, call
`Root::set_text_selection_group(Some((group_id, &members)), cx)`, where
`members` is an ordered slice of `(ElementId, Entity<TextViewState>)`. Supply
unique, opaque consumer IDs tied to the real posts, not row indices, author/time
metadata, or body text. IDs must remain associated with the same retained text
entities across refreshes. Duplicate post IDs or duplicate text entities are
rejected. This API installs one group per Root, not a global conversation cache.

Keep the returned `TextSelectionGroupRetirement` lease in the active consumer.
Root retains only weak text entities and weak lease ownership. An unchanged
binding returns another clone of the live lease. Explicit `retire()`, the last
consumer clone's drop, or `set_text_selection_group(None, cx)` synchronously
retires the logical range, native geometry, viewport and edge-scroll task;
retirement needs no paint or App/Window access. An old lease cannot retire a
replacement binding. Retire on owner/screen departure and modal displacement,
and do not install groups while rendering.

Wrap the one outer scroll viewport with
`TextSelectionGroupElement::text_selection_group(group_id, scroll_callback)`.
The callback scrolls that viewport and requests the next layout. The marker
and actual text layouts provide hit authority; membership alone never does.
Pointer endpoints use the nearest shaped caret on the current visual line,
respecting grapheme boundaries rather than slicing a multi-byte character or
multi-codepoint grapheme. Accepted-source caret metadata is reused, not rebuilt
per mouse move. Cached GPUI element replay retains only GPUI-owned presence
states; the controller's weak presence expires on actual unmount, so cached
coordinates cannot become permanent hit authority.

### Refresh and source boundaries

A changed binding always retires the prior native hitboxes, viewport, drag
pointer and edge task and returns a fresh lease. It preserves **only logical
endpoints** when the old range is currently valid, the group and active scope
are unchanged, and the selected interval still contains the same ordered
stable IDs, retained entities and accepted source revisions. Thus reordering,
prepending or editing posts outside the selected interval can preserve a
selection of an unchanged post. Changing/removing a selected post, inserting
inside the selected interval, changing its order, leaving the group, or losing
a required weak entity fails closed. Copy resumes only after the replacement
viewport is painted; old geometry and an old lease confer no authority.

Endpoints are UTF-8 byte offsets in the canonical **rendered** stream, not raw
Markdown/HTML offsets. Its existing block/table separators, link labels,
custom-node text and literal code are preserved; image destinations/alt text
and hidden HTML are not invented as selectable text. The same traversal backs
SelectAll and managed Copy. Width, zoom and style changes invalidate/rebuild
geometry without changing an unchanged source interval. A changed source or
Markdown plugin revision is a separate parse/selection event.

### Test-platform boundary

The SDK's `logical_selection` and legacy `window_selection`/`text_view` tests
exercise the managed and ungrouped paths without skips. The SDK Input fixture
is explicitly **disabled**: its exact draft/empty clipboard assertions prove
only that disabled boundary. GPUI's `TestWindow` does not implement the native
input-handler hooks used by an enabled Input; do not suppress that panic or
claim the disabled fixture proves enabled editing.

Kagi's `issue_conversation_enabled_input_copy` scenario supplies the separate
real AppKit counterpart: enabled production Reply SelectAll/Copy, physical
Backspace to empty, poison-preserving empty Copy, then physical Tab and Copy
of the original conversation selection without reselecting/reactivating it.
It also checks departure retirement and repository/draft invariants. This is
native event-simulation coverage with a private test clipboard, not hardware
input, IME, Tier B screenshots or a full-suite result.

At the 2026-10-10 Kagi integration checkpoint, the complete SDK lib suite ran
334 tests with no failures, ignored or filtered cases; all ten native Issue
conversation scenarios passed together, including enabled Input recovery.
Both obsolete parser text dead-code warnings were absent. Those runs preceded
formatting-only changes. Scoped formatting/check of the 13 changed Rust
targets used the existing edition/style edition 2024 configuration and passed;
the whole SDK workspace format check failed on unrelated baseline as well as
the then-unformatted changes. No full-repository format pass is implied.
The SDK lib disables doctests. Post-format/public-pin acceptance, other gates
and default-app Tier B remain separate; see Kagi ADR-0198 for the dated record.
PM subsequently reran the post-format complete SDK lib suite: again 334 tests
passed (test bodies 0.07 s). UI-lib Clippy exited 0 with one newly surfaced
argument-count diagnostic at the private native paint-registration boundary;
a localized documented allowance preserves its existing independent paint
inputs without an extra payload type or geometry change. PM's exact-annotation
UI-lib Clippy (`bg_619`) then exited 0 in 4.19 s (check/compile 3.99 s), with no
new argument-count/dead-code warnings; only existing dependency
future-compatibility notices remained. Exact-source SDK lib plus the same
13-target configured-2024 format check (`bg_620`) then passed: 334 tests,
no failures, ignored or filtered cases, bodies 0.07 s; scoped check exit 0, combined
process 7.33 s. No public SDK revision, post-format native rerun or
whole-workspace format pass is claimed.

## Markdown Plugins

Use `.plugin(...)` to support custom Markdown formats. A plugin owns both parsing and rendering, so callers only need to attach it to the `TextView`:

```rust
markdown(source)
    .plugin(TickerPlugin::new())
```

A Markdown plugin implements `MarkdownPlugin`:

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

Then attach it to a Markdown `TextView`:

```rust
markdown("$AAPL.US")
    .plugin(TickerPlugin::new())
```

## MarkdownNode

`MarkdownNode` is the neutral data passed between `parse` and `render`.

```rust
MarkdownNode::new("ticker", TickerNode { symbol })
    .text("$AAPL.US")
    .markdown("$AAPL.US")
```

- `name` is the stable node name used to match the renderer.
- `data` is typed parser output read with `node.data::<T>()`.
- `text` is the plain text representation used by selection and fallback rendering.
- `markdown` is the Markdown representation used when the document is serialized back to Markdown.

## Block Plugins

Custom Markdown rendering currently supports block plugins. Return `true` from `is_block()` for plugins that should be registered today:

```rust
fn is_block(&self) -> bool {
    true
}
```

Inline plugins are reserved for future `TextView` support.

## Code Block Actions

You can render controls for Markdown code blocks:

```rust
markdown(source)
    .code_block_actions(|code_block, _window, _cx| {
        gpui::div().child(format!("Run {}", code_block.lang().unwrap_or_default()))
    })
```
