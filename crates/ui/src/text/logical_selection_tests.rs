use super::*;
use crate::{
    WindowExt as _,
    input::{Copy, Input, InputState, SelectAll},
    text::TextView,
};
use gpui::{
    Action as _, AppContext as _, ClipboardItem, Focusable as _, InputEvent as _,
    InteractiveElement as _, ListAlignment, ListOffset, ListState, Modifiers, MouseButton,
    MouseMoveEvent, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
    canvas, div, point,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
};

const MIDDLE: &str = "Middle **é🙂** and [label](https://private.invalid)\n\n- [x] task\n\n| H | 値 |\n|---|---|\n| r | λ |\n\n```text\nliteral <!-- safe -->\n```\n\n<!-- hidden PRIVATE -->";
const EXPECTED: &str =
    "A尾\n\nMiddle é🙂 and label\ntask\nH 値\nr λ\n\nliteral <!-- safe -->\n\nZ🙂";

struct Conversation {
    posts: Vec<(ElementId, Entity<TextViewState>)>,
    list: ListState,
    painted: Rc<RefCell<HashSet<ElementId>>>,
    paint_calls: Rc<Cell<usize>>,
    input: Option<Entity<InputState>>,
    marker: bool,
    width: Pixels,
    fixed_rows: bool,
    consume_release: bool,
    retirement: Option<TextSelectionGroupRetirement>,
    group: ElementId,
}

impl Conversation {
    fn new(sources: &[&str], cx: &mut Context<Self>) -> Self {
        Self {
            posts: sources
                .iter()
                .enumerate()
                .map(|(ix, text)| {
                    (
                        ElementId::Integer(ix as u64),
                        cx.new(|cx| TextViewState::markdown(text, cx)),
                    )
                })
                .collect(),
            list: ListState::new(sources.len(), ListAlignment::Top, px(0.)),
            painted: Rc::default(),
            marker: true,
            width: px(360.),
            fixed_rows: true,
            paint_calls: Rc::default(),
            input: None,
            consume_release: false,
            retirement: None,
            group: "conversation".into(),
        }
    }
}

impl Render for Conversation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let posts = self.posts.clone();
        let painted = self.painted.clone();
        let paint_calls = self.paint_calls.clone();
        let fixed = self.fixed_rows;
        let consume_release = self.consume_release;
        let mut viewport = div()
            .w(self.width)
            .h(px(120.))
            .overflow_hidden()
            .child(
                gpui::list(self.list.clone(), move |ix, _, _| {
                    let (key, state) = &posts[ix];
                    let key = key.clone();
                    let painted = painted.clone();
                    let paint_calls = paint_calls.clone();
                    let row = div()
                        .pt(px(24.))
                        .min_h(px(180.))
                        .child(TextView::new(state).selectable(true))
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |_, _, _, _| {
                                    painted.borrow_mut().insert(key.clone());
                                    paint_calls.set(paint_calls.get() + 1);
                                },
                            )
                            .absolute()
                            .size_full(),
                        );
                    if fixed {
                        row.h(px(180.)).into_any_element()
                    } else {
                        row.into_any_element()
                    }
                })
                .size_full(),
            )
            .into_any_element();
        if self.marker {
            let list = self.list.clone();
            viewport = viewport
                .text_selection_group(self.group.clone(), move |delta, window, _| {
                    list.scroll_by(delta);
                    window.refresh();
                })
                .into_any_element();
        }
        div()
            .child(viewport)
            .children(
                self.input
                    .as_ref()
                    .map(|input| Input::new(input).disabled(true)),
            )
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                if consume_release {
                    cx.stop_propagation();
                }
            })
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn setup<'a>(
    sources: &[&str],
    cx: &'a mut TestAppContext,
) -> (Entity<Conversation>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let (root, cx) = cx.add_window_view(|window, cx| {
        let conversation = cx.new(|cx| Conversation::new(sources, cx));
        Root::new(conversation, window, cx).bordered(false)
    });
    let conversation = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<Conversation>().unwrap()
    });
    let posts = conversation.read_with(cx, |view, _| view.posts.clone());
    let retirement = root.update(cx, |root, cx| {
        root.set_text_selection_group(Some(("conversation".into(), &posts)), cx)
    });
    conversation.update(cx, |view, _| view.retirement = retirement);
    drop(posts);
    draw(cx);
    (conversation, cx)
}

fn bind(
    view: &Entity<Conversation>,
    posts: &[(ElementId, Entity<TextViewState>)],
    cx: &mut VisualTestContext,
) {
    let group = view.read_with(cx, |view, _| view.group.clone());
    let retirement = cx.update(|window, cx| {
        Root::update(window, cx, |root, _, cx| {
            root.set_text_selection_group(Some((group, posts)), cx)
        })
    });
    view.update(cx, |view, _| view.retirement = retirement);
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

fn jump(view: &Entity<Conversation>, ix: usize, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    draw(cx);
}

fn insertion(
    view: &Entity<Conversation>,
    post: usize,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let id = view.read_with(cx, |view, _| view.posts[post].1.entity_id());
    cx.update(|window, cx| {
        let root = Root::read(window, cx);
        let selection = root.logical_selection.state.borrow();
        let geometry = selection
            .geometry
            .iter()
            .find(|geometry| {
                geometry.view == id
                    && offset >= geometry.leaf.start + geometry.source_range.start
                    && offset <= geometry.leaf.start + geometry.source_range.end
            })
            .expect("requested logical insertion boundary must be visibly laid out");
        let local = offset - geometry.leaf.start - geometry.source_range.start;
        let position = geometry.layout.position_for_index(local).unwrap();
        point(
            position.x + px(0.01),
            position.y + geometry.layout.line_height() / 2.,
        )
    })
}

fn copy(cx: &mut VisualTestContext) -> String {
    cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
        window.dispatch_action(Copy.boxed_clone(), cx);
    });
    // Pinned Window::dispatch_action defers to the end of the update effect
    // cycle. Reading in the dispatch closure would only read the poison.
    cx.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

fn select_across(view: &Entity<Conversation>, reverse: bool, cx: &mut VisualTestContext) {
    let (first, first_offset, last, last_offset) = if reverse {
        (2, "Z🙂".len(), 0, "α ".len())
    } else {
        (0, "α ".len(), 2, "Z🙂".len())
    };
    jump(view, first, cx);
    let start = insertion(view, first, first_offset, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    jump(view, last, cx);
    let end = insertion(view, last, last_offset, cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), EXPECTED);
    assert!(
        !view.read_with(cx, |view, _| view
            .painted
            .borrow()
            .contains(&ElementId::Integer(1))),
        "the GFM middle post must never be painted to make Copy correct"
    );
}

#[gpui::test]
fn unpainted_middle_forward_reverse_and_unmounted_endpoints_copy_exactly(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish", "unselected fourth"], cx);
    select_across(&view, false, cx);
    jump(&view, 3, cx);
    assert_eq!(copy(cx), EXPECTED, "both endpoints may be unmounted");
    select_across(&view, true, cx);
}

#[gpui::test]
fn mousemove_then_copy_in_same_update_needs_no_paint(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    let start = insertion(&view, 0, "α ".len(), cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    jump(&view, 2, cx);
    let end = insertion(&view, 2, "Z🙂".len(), cx);
    let paints_before = view.read_with(cx, |view, _| view.paint_calls.get());
    let (text, paints_at_copy) = cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
        window.dispatch_event(
            MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        // Window::dispatch_action defers; test-support draws dirty windows when
        // that update finishes. Dispatch synchronously through the actual
        // current focus handle and observe Copy before that legitimate draw.
        window
            .focused(cx)
            .expect("the native drag must keep its managed focus")
            .dispatch_action(&Copy, window, cx);
        (
            cx.read_from_clipboard().unwrap().text().unwrap(),
            view.read(cx).paint_calls.get(),
        )
    });
    assert_eq!(text, EXPECTED);
    assert_eq!(
        paints_at_copy, paints_before,
        "the canonical Copy assertion must execute before any selection repaint"
    );
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn accepted_same_source_resize_and_theme_keep_range_but_edit_and_none_retire_it(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    select_across(&view, false, cx);
    let first = view.read_with(cx, |view, _| view.posts[0].1.clone());
    first.update(cx, |state, cx| state.set_text("α A尾", cx));
    view.update(cx, |view, cx| {
        view.width = px(240.);
        cx.notify();
    });
    draw(cx);
    assert_eq!(copy(cx), EXPECTED);
    cx.update(|window, cx| {
        crate::Theme::change(crate::ThemeMode::Dark, Some(window), cx);
    });
    draw(cx);
    assert_eq!(copy(cx), EXPECTED);
    first.update(cx, |state, cx| state.set_text("β B尾", cx));
    assert_eq!(
        copy(cx),
        "poison",
        "same-byte-length accepted edit cannot reuse old endpoints"
    );
    let posts = view.read_with(cx, |view, _| view.posts.clone());
    bind(&view, &posts, cx);
    draw(cx);
    // A fresh selection works after revision acceptance; leaving the group retires it.
    let start = insertion(&view, 2, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let end = insertion(&view, 2, "Z🙂".len(), cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), "Z🙂");
    drop(cx.update(|window, cx| {
        Root::update(window, cx, |root, _, cx| {
            root.set_text_selection_group(None, cx)
        })
    }));
    assert_eq!(copy(cx), "poison");
}

#[gpui::test]
fn current_frame_removes_stale_hits_and_focused_post_selectall_stays_local(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    let old = insertion(&view, 0, "α ".len(), cx);
    jump(&view, 2, cx);
    cx.simulate_mouse_down(old, MouseButton::Left, Modifiers::default());
    let end = insertion(&view, 2, "Z🙂".len(), cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    let selected = cx.update(|window, cx| window.selected_text(cx));
    assert!(!selected.contains("A尾") && !selected.contains("Middle"));
    let last = view.read_with(cx, |view, _| view.posts[2].1.clone());
    cx.update(|window, cx| {
        let focus = last.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        window.dispatch_action(SelectAll.boxed_clone(), cx);
    });
    assert_eq!(copy(cx), "Z🙂 finish");
    view.update(cx, |view, cx| {
        view.marker = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        copy(cx),
        "poison",
        "a live retained entity is not current viewport geometry"
    );
}

#[gpui::test]
fn clipped_single_post_copies_canonical_middle_without_layout_and_survives_reflow(
    cx: &mut TestAppContext,
) {
    let code = (0..200)
        .map(|ix| format!("line {ix} λ🙂\n"))
        .collect::<String>();
    let source = format!("α A尾\n\n```text\n{code}```\n\nZ🙂 finish");
    let (view, cx) = setup(&[&source], cx);
    view.update(cx, |view, cx| {
        view.fixed_rows = false;
        view.list.remeasure();
        cx.notify();
    });
    draw(cx);
    let start = insertion(&view, 0, "α ".len(), cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, cx| {
        view.list.scroll_to_end();
        cx.notify();
    });
    draw(cx);
    let expected = format!("A尾\n{code}Z🙂");
    // Fenced code's final source newline is removed by the existing mdast parser,
    // then canonical BlockNode serialization emits its one block separator.
    let tail_offset = "α A尾\n".len() + code.len() + "Z🙂".len();
    let end = insertion(&view, 0, tail_offset, cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), expected);
    view.update(cx, |view, cx| {
        view.width = px(220.);
        view.list.remeasure();
        cx.notify();
    });
    draw(cx);
    assert_eq!(copy(cx), expected);
}

#[gpui::test]
fn edge_delegate_rehits_after_layout_and_stops_on_mouseup_none_and_scope(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish", "composer end"], cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let edge = point(px(100.), px(119.));
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    let before = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(32));
    draw(cx);
    let after = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert!(after.item_ix > before.item_ix || after.offset_in_item > before.offset_in_item);
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    let stopped = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(64));
    draw(cx);
    let now = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert_eq!(
        (now.item_ix, now.offset_in_item),
        (stopped.item_ix, stopped.offset_in_item)
    );
    // Repeat with an active edge task, first deactivating the group synchronously.
    jump(&view, 0, cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    drop(cx.update(|window, cx| {
        Root::update(window, cx, |root, _, cx| {
            root.set_text_selection_group(None, cx)
        })
    }));
    let stopped = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(64));
    draw(cx);
    let now = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert_eq!(
        (now.item_ix, now.offset_in_item),
        (stopped.item_ix, stopped.offset_in_item)
    );
    assert_eq!(copy(cx), "poison");

    let posts = view.read_with(cx, |view, _| view.posts.clone());
    bind(&view, &posts, cx);
    draw(cx);
    jump(&view, 0, cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    cx.update(|window, cx| {
        Root::update(window, cx, |root, window, cx| {
            root.open_dialog(|dialog, _, _| dialog.child("modal private"), window, cx);
        })
    });
    let stopped = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(64));
    draw(cx);
    let now = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert_eq!(
        (now.item_ix, now.offset_in_item),
        (stopped.item_ix, stopped.offset_in_item)
    );
    assert_eq!(copy(cx), "poison");
}

// TestWindow has no native input-handler implementation. This covers only the
// disabled Input boundary; enabled nonempty/empty Copy and selection recovery
// are exercised on the real AppKit window by Kagi's native consumer scenario.
#[gpui::test]
fn disabled_input_copy_does_not_fall_back_to_conversation(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    select_across(&view, false, cx);
    let input =
        cx.update(|window, cx| cx.new(|cx| InputState::new(window, cx).default_value("draft🙂")));
    view.update(cx, |view, cx| {
        view.input = Some(input.clone());
        cx.notify();
    });
    draw(cx);
    cx.update(|window, cx| {
        input.focus_handle(cx).focus(window, cx);
        window.dispatch_action(SelectAll.boxed_clone(), cx);
    });
    assert_eq!(copy(cx), "draft🙂");
    cx.update(|window, cx| input.update(cx, |state, cx| state.set_value("", window, cx)));
    assert_eq!(copy(cx), "poison");
}

#[gpui::test]
fn native_image_wrapping_maps_fragment_bytes_back_to_the_persistent_run(cx: &mut TestAppContext) {
    // This URI is a local embedded one-pixel PNG, never a remote image address.
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=";
    let source = format!(
        "start ![private alt]({image}) αβ🙂 one two three four five six seven eight nine tail"
    );
    let expected = "start  αβ🙂 one two three four five six seven eight nine";
    let (view, cx) = setup(&[&source], cx);
    view.update(cx, |view, cx| {
        view.width = px(150.);
        view.fixed_rows = false;
        view.list.remeasure();
        cx.notify();
    });
    draw(cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, cx| {
        view.list.scroll_to_end();
        cx.notify();
    });
    draw(cx);
    let wrapped = cx.update(|window, cx| {
        Root::read(window, cx)
            .logical_selection
            .state
            .borrow()
            .geometry
            .iter()
            .any(|geometry| geometry.source_range.start > 0)
    });
    assert!(
        wrapped,
        "consumer must actually lay out a wrapped persistent text run"
    );
    let end = insertion(&view, 0, expected.len(), cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), expected);
}

#[gpui::test]
fn rendered_unicode_ranges_include_synthetic_separators_but_not_image_or_html_secrets(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let source = "# Head\n\né🙂 [label](/hidden) ![alt](asset://secret)\n\n- [ ] task\n\n| A | B |\n|---|---|\n| λ | 値 |\n\n<!-- HIDDEN -->\n\n```text\n<!-- literal -->\n```";
    let state = cx.new(|cx| TextViewState::markdown(source, cx));
    state.read_with(cx, |state, _| {
        let rendered = state.rendered().unwrap();
        assert_eq!(
            rendered.text(),
            "Head\né🙂 label \ntask\nA B\nλ 値\n\n<!-- literal -->\n"
        );
        let mut text = String::new();
        assert!(
            rendered
                .write_range(
                    "Head\né".len().."Head\né🙂 label \ntask\nA B\nλ".len(),
                    &mut text
                )
                .is_some()
        );
        assert_eq!(text, "🙂 label \ntask\nA B\nλ");
        assert!(
            rendered.write_range(6..7, &mut String::new()).is_none(),
            "UTF-8 interior endpoints cannot be repaired by guessing character offsets"
        );
    });
}

#[gpui::test]
fn missing_weak_middle_never_returns_a_partial_surviving_range(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    select_across(&view, false, cx);
    view.update(cx, |view, cx| {
        view.posts.remove(1);
        view.list.reset(view.posts.len());
        cx.notify();
    });
    // Retire the prior list closure too: it owned the previous frame's rows.
    draw(cx);
    draw(cx);
    assert_eq!(
        copy(cx),
        "poison",
        "failed weak upgrade must not copy just A and Z"
    );
}

#[gpui::test]
fn deferred_modal_group_survives_inactive_base_marker_and_repeated_actual_frames(
    cx: &mut TestAppContext,
) {
    let (_, cx) = setup(&["base private", MIDDLE, "base tail"], cx);
    let modal = cx.update(|_, cx| cx.new(|cx| TextViewState::markdown("Modal é🙂 only", cx)));
    let modal_for_dialog = modal.clone();
    let list = ListState::new(1, ListAlignment::Top, px(0.));
    let _modal_retirement = cx.update(|window, cx| {
        Root::update(window, cx, |root, window, cx| {
            root.open_dialog(
                move |dialog, _, _| {
                    let list = list.clone();
                    dialog.child(
                        div()
                            .w(px(360.))
                            .h(px(180.))
                            .child(TextView::new(&modal_for_dialog).selectable(true))
                            .text_selection_group("modal-conversation", move |delta, window, _| {
                                list.scroll_by(delta);
                                window.refresh();
                            }),
                    )
                },
                window,
                cx,
            );
            let posts = [(ElementId::Integer(42), modal.clone())];
            root.set_text_selection_group(Some(("modal-conversation".into(), &posts)), cx)
        })
    });
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    draw(cx);
    cx.update(|window, cx| {
        let focus = modal.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        window.dispatch_action(SelectAll.boxed_clone(), cx);
    });
    assert_eq!(copy(cx), "Modal é🙂 only");
    draw(cx);
    draw(cx);
    assert_eq!(
        copy(cx),
        "Modal é🙂 only",
        "inactive base marker cannot retire a group painted in the later deferred modal scope"
    );
}

#[gpui::test]
fn consumed_mouseup_still_cancels_the_outer_edge_task(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    view.update(cx, |view, cx| {
        view.consume_release = true;
        cx.notify();
    });
    draw(cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let edge = point(px(100.), px(119.));
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(32));
    draw(cx);
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    let stopped = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(64));
    draw(cx);
    let now = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert_eq!(
        (now.item_ix, now.offset_in_item),
        (stopped.item_ix, stopped.offset_in_item)
    );
}

#[gpui::test]
fn no_app_retire_before_paint_blocks_copy_stale_hits_and_selected_all_fallback(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    let first = view.read_with(cx, |view, _| view.posts[0].1.clone());
    cx.update(|window, cx| {
        let focus = first.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        window.dispatch_action(SelectAll.boxed_clone(), cx);
    });
    assert_eq!(copy(cx), "α A尾");
    let stale = insertion(&view, 0, 0, cx);
    let retirement = view.read_with(cx, |view, _| view.retirement.as_ref().unwrap().clone());
    let paint_calls = view.read_with(cx, |view, _| view.paint_calls.clone());
    let before = paint_calls.get();
    retirement.retire(); // No App, Window, deferred dispatch, or new paint.
    assert!(!retirement.is_active());
    assert_eq!(copy(cx), "poison");
    assert_eq!(paint_calls.get(), before);
    cx.simulate_mouse_down(stale, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(stale.x + px(30.), stale.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_up(stale, MouseButton::Left, Modifiers::default());
    assert_eq!(
        copy(cx),
        "poison",
        "retired painted members cannot fall back to local/legacy selection"
    );
}

#[gpui::test]
fn unchanged_lease_reconciliation_preserves_range_and_last_consumer_drop_retires(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    select_across(&view, false, cx);
    let previous = view.update(cx, |view, _| view.retirement.take().unwrap());
    let posts = view.read_with(cx, |view, _| view.posts.clone());
    bind(&view, &posts, cx);
    drop(previous); // The new returned consumer clone still owns the binding.
    assert_eq!(copy(cx), EXPECTED);
    let last = view.update(cx, |view, _| view.retirement.take().unwrap());
    drop(last); // Root's weak lease owner must not keep the selection alive.
    assert_eq!(copy(cx), "poison");
}

#[gpui::test]
fn old_lease_cannot_retire_replacement_range_or_its_real_edge_task(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["α A尾", MIDDLE, "Z🙂 finish"], cx);
    let old = view.read_with(cx, |view, _| view.retirement.as_ref().unwrap().clone());
    view.update(cx, |view, cx| {
        view.group = "replacement".into();
        cx.notify();
    });
    let posts = view.read_with(cx, |view, _| view.posts.clone());
    bind(&view, &posts, cx);
    draw(cx);
    let start = insertion(&view, 0, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let edge = point(px(100.), px(119.));
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    let before = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    old.retire();
    drop(old);
    assert_eq!(copy(cx), "α A尾");
    cx.executor().advance_clock(Duration::from_millis(32));
    draw(cx);
    let after = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert!(after.item_ix > before.item_ix || after.offset_in_item > before.offset_in_item);
    let last = view.update(cx, |view, _| view.retirement.take().unwrap());
    drop(last); // A pending timer owns no strong consumer lease across await.
    assert!(cx.update(|window, cx| {
        Root::read(window, cx)
            .logical_selection
            .state
            .borrow()
            .edge_task
            .is_none()
    }));
    let stopped = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    cx.executor().advance_clock(Duration::from_millis(64));
    draw(cx);
    let now = view.read_with(cx, |view, _| view.list.logical_scroll_top());
    assert_eq!(
        (now.item_ix, now.offset_in_item),
        (stopped.item_ix, stopped.offset_in_item)
    );
    assert_eq!(copy(cx), "poison");
}

#[gpui::test]
fn visible_code_style_cache_and_same_source_acceptance_keep_logical_copy(cx: &mut TestAppContext) {
    const SOURCE: &str = "```rust\nlet café = \"🙂\";\n```";
    let (view, cx) = setup(&[SOURCE], cx);
    let state = view.read_with(cx, |view, _| view.posts[0].1.clone());
    cx.update(|window, cx| {
        let focus = state.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        window.dispatch_action(SelectAll.boxed_clone(), cx);
    });
    assert_eq!(copy(cx), "let café = \"🙂\";");
    state.update(cx, |state, cx| state.set_text(SOURCE, cx));
    let posts = view.read_with(cx, |view, _| view.posts.clone());
    bind(&view, &posts, cx);
    view.update(cx, |view, cx| {
        view.width = px(180.);
        cx.notify();
    });
    cx.update(|window, cx| crate::Theme::change(crate::ThemeMode::Dark, Some(window), cx));
    draw(cx);
    assert_eq!(copy(cx), "let café = \"🙂\";");
}

#[gpui::test]
fn partial_first_paragraph_remains_exact_when_later_paragraph_is_painted(cx: &mut TestAppContext) {
    let (view, cx) = setup(&["first\n\nsecond"], cx);
    let start = insertion(&view, 0, 0, cx);
    let end = insertion(&view, 0, 3, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(copy(cx), "fir");
}

#[gpui::test]
fn partial_before_native_image_remains_exact_when_after_image_run_is_painted(
    cx: &mut TestAppContext,
) {
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=";
    let source = format!("first![private alt]({image})second");
    let (view, cx) = setup(&[&source], cx);
    let start = insertion(&view, 0, 0, cx);
    let end = insertion(&view, 0, 3, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(copy(cx), "fir");
}

struct CachedReplayCounter(usize);

impl Render for CachedReplayCounter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(format!("unrelated {}", self.0))
    }
}

struct CachedConversationHost {
    conversation: Entity<Conversation>,
    unrelated: Entity<CachedReplayCounter>,
    mounted: bool,
}

impl Render for CachedConversationHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // GPUI cached views require definite bounds, not content measurement.
        let mut layout = div().w(px(360.)).h(px(120.));
        let style = layout.style().clone();
        div()
            .children(
                self.mounted
                    .then(|| self.conversation.clone().cached(style)),
            )
            .child(self.unrelated.clone())
    }
}

#[gpui::test]
fn cached_conversation_replay_keeps_copy_hits_and_lease_but_unmount_retires_geometry(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let (root, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|cx| CachedConversationHost {
            conversation: cx.new(|cx| Conversation::new(&["α A尾", MIDDLE, "Z🙂 finish"], cx)),
            unrelated: cx.new(|_| CachedReplayCounter(0)),
            mounted: true,
        });
        Root::new(host, window, cx).bordered(false)
    });
    let host = root.read_with(cx, |root, _| {
        root.view()
            .clone()
            .downcast::<CachedConversationHost>()
            .unwrap()
    });
    let (conversation, unrelated) = host.read_with(cx, |host, _| {
        (host.conversation.clone(), host.unrelated.clone())
    });
    let posts = conversation.read_with(cx, |view, _| view.posts.clone());
    bind(&conversation, &posts, cx);
    drop(posts);
    conversation.update(cx, |_, cx| cx.notify());
    // Test-support already draws at the notification update boundary. Another
    // explicit draw here would be cached replay, before gesture setup.

    let start = insertion(&conversation, 0, "α ".len(), cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    conversation.update(cx, |view, cx| {
        view.list.scroll_to(ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    let end = insertion(&conversation, 2, "Z🙂".len(), cx);
    let last_start = insertion(&conversation, 2, 0, cx);
    let last_end = insertion(&conversation, 2, "Z🙂 finish".len(), cx);
    let initial_copy = cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
        window.dispatch_event(
            MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        window
            .focused(cx)
            .unwrap()
            .dispatch_action(&Copy, window, cx);
        cx.read_from_clipboard().unwrap().text().unwrap()
    });
    assert_eq!(
        initial_copy, EXPECTED,
        "the accepted source must work before cached replay"
    );
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert!(
        !conversation.read_with(cx, |view, _| view
            .painted
            .borrow()
            .contains(&ElementId::Integer(1))),
        "the canonical middle post must remain unpainted"
    );

    let paints_before = conversation.read_with(cx, |view, _| view.paint_calls.get());
    unrelated.update(cx, |counter, cx| {
        counter.0 += 1;
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        conversation.read_with(cx, |view, _| view.paint_calls.get()),
        paints_before,
        "this must exercise GPUI cached paint replay, not repaint the conversation"
    );
    assert!(conversation.read_with(cx, |view, _| view.retirement.as_ref().unwrap().is_active()));
    assert_eq!(
        copy(cx),
        EXPECTED,
        "unrelated cached replay must retain current scoped canonical Copy"
    );

    // These saved positions belong to the unchanged cached layout. A real
    // second drag proves replayed current-frame hit geometry, not just Copy.
    cx.simulate_mouse_down(last_start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(last_end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(last_end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), "Z🙂 finish");
    assert!(conversation.read_with(cx, |view, _| view.retirement.as_ref().unwrap().is_active()));

    host.update(cx, |host, cx| {
        host.mounted = false;
        cx.notify();
    });
    draw(cx);
    cx.simulate_mouse_down(last_start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(last_end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(last_end, MouseButton::Left, Modifiers::default());
    assert_eq!(
        copy(cx),
        "poison",
        "a held source/lease must not resurrect an unmounted cached viewport or its stale hits"
    );
}

#[gpui::test]
fn trailing_glyph_right_half_selects_complete_ascii_and_unicode_posts(cx: &mut TestAppContext) {
    for (source, expected, trailing_start) in [
        ("**OLD_B**", "OLD_B", 4),
        ("**café🙂**", "café🙂", "café".len()),
    ] {
        let (view, visual) = setup(&[source], cx);
        let start = insertion(&view, 0, 0, visual);
        let glyph_start = insertion(&view, 0, trailing_start, visual);
        let final_caret = insertion(&view, 0, expected.len(), visual);
        assert!(
            final_caret.x > glyph_start.x,
            "the final rendered glyph must have a real visible advance"
        );
        let endpoint = point(
            glyph_start.x + (final_caret.x - glyph_start.x) * 0.75,
            final_caret.y,
        );
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(endpoint, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(endpoint, MouseButton::Left, Modifiers::default());
        assert_eq!(
            copy(visual),
            expected,
            "dragging into the right half of the final rendered glyph must include it, not treat its containing-glyph index as an exclusive caret"
        );
    }
}
