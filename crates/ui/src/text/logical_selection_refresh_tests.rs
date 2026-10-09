use super::*;
use crate::{input::Copy, text::TextView};
use gpui::{
    Action as _, AppContext as _, ClipboardItem, Modifiers, MouseButton, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, div, point,
};

struct RefreshConversation {
    posts: Vec<(ElementId, Entity<TextViewState>)>,
}

impl Render for RefreshConversation {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(360.))
            .h(px(300.))
            .children(
                self.posts
                    .iter()
                    .map(|(_, text)| div().h(px(60.)).child(TextView::new(text).selectable(true))),
            )
            .text_selection_group("refresh-conversation", |_, _, _| {})
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

fn copy(cx: &mut VisualTestContext) -> String {
    cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
        window.dispatch_action(Copy.boxed_clone(), cx);
    });
    cx.update(|_, cx| {
        cx.read_from_clipboard()
            .expect("test clipboard item")
            .text()
            .expect("test clipboard text")
    })
}

fn bind(
    root: &Entity<Root>,
    posts: &[(ElementId, Entity<TextViewState>)],
    cx: &mut VisualTestContext,
) -> TextSelectionGroupRetirement {
    root.update(cx, |root, cx| {
        root.set_text_selection_group(Some(("refresh-conversation".into(), posts)), cx)
    })
    .expect("valid group lease")
}

#[gpui::test]
fn stable_selected_post_moves_without_transferring_native_authority_or_old_lease(
    cx: &mut TestAppContext,
) {
    cx.update(crate::init);
    let (root, cx) = cx.add_window_view(|window, cx| {
        let conversation = cx.new(|cx| RefreshConversation {
            posts: ["ROOT", "OLD_A", "OLD_B"]
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    (
                        ElementId::Integer(index as u64),
                        cx.new(|cx| TextViewState::markdown(*text, cx)),
                    )
                })
                .collect(),
        });
        Root::new(conversation, window, cx).bordered(false)
    });
    let conversation = root.read_with(cx, |root, _| {
        root.view()
            .clone()
            .downcast::<RefreshConversation>()
            .expect("refresh conversation")
    });
    let mut posts = conversation.read_with(cx, |view, _| view.posts.clone());
    let old = bind(&root, &posts, cx);
    draw(cx);
    let selected_view = posts[2].1.entity_id();
    let (start, end) = cx.update(|_, cx| {
        let selection = root.read(cx).logical_selection.state.borrow();
        let geometry = selection
            .geometry
            .iter()
            .find(|geometry| geometry.view == selected_view)
            .expect("selected B has current painted geometry");
        let start = geometry
            .layout
            .position_for_index(0)
            .expect("B start caret");
        let end = geometry
            .layout
            .position_for_index("OLD_B".len())
            .expect("B end caret");
        let middle = geometry.layout.line_height() / 2.;
        (
            point(start.x + px(0.01), start.y + middle),
            point(end.x + px(0.01), end.y + middle),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(copy(cx), "OLD_B");

    posts.swap(1, 2);
    let replacement = cx.update(|window, cx| {
        conversation.update(cx, |view, cx| {
            view.posts = posts.clone();
            cx.notify();
        });
        let replacement = root
            .update(cx, |root, cx| {
                root.set_text_selection_group(Some(("refresh-conversation".into(), &posts)), cx)
            })
            .expect("replacement lease");
        let root = root.read(cx);
        let selection = root.logical_selection.state.borrow();
        assert_eq!(
            selection.anchor.as_ref().expect("preserved anchor").post,
            ElementId::Integer(2)
        );
        assert_eq!(
            selection.cursor.as_ref().expect("preserved cursor").post,
            ElementId::Integer(2)
        );
        assert!(selection.viewport.is_none());
        assert!(selection.geometry.is_empty());
        assert!(!selection.dragging);
        assert!(selection.pointer.is_none());
        assert!(selection.edge_task.is_none());
        assert!(
            selection.hit(start, window, cx).is_none(),
            "old geometry has no hit authority"
        );
        assert!(
            !root.has_logical_selection(cx),
            "Copy needs replacement viewport paint"
        );
        replacement
    });
    assert!(!old.is_active());
    assert!(!Rc::ptr_eq(&old.0, &replacement.0));
    old.retire();
    drop(old);
    draw(cx);
    assert_eq!(
        copy(cx),
        "OLD_B",
        "repaint resolves true B in its new position"
    );

    let prepend = cx.new(|cx| TextViewState::markdown("NEW_P", cx));
    posts.insert(1, (ElementId::Integer(3), prepend));
    conversation.update(cx, |view, cx| {
        view.posts = posts.clone();
        cx.notify();
    });
    let prepended = bind(&root, &posts, cx);
    replacement.retire();
    drop(replacement);
    draw(cx);
    assert_eq!(
        copy(cx),
        "OLD_B",
        "unselected insertion must not retarget the range"
    );

    posts[3]
        .1
        .update(cx, |state, cx| state.set_text("NEW_A", cx));
    let unselected_edit = bind(&root, &posts, cx);
    prepended.retire();
    drop(prepended);
    draw(cx);
    assert_eq!(
        copy(cx),
        "OLD_B",
        "unselected source changes preserve true B"
    );

    posts[2]
        .1
        .update(cx, |state, cx| state.set_text("NEW_B", cx));
    let selected_edit = bind(&root, &posts, cx);
    drop(unselected_edit);
    draw(cx);
    assert_eq!(
        copy(cx),
        "poison",
        "selected same-length replacement fails closed"
    );
    drop(selected_edit);
}
