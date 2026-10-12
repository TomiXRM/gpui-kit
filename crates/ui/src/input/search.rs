use aho_corasick::AhoCorasick;
use std::{ops::Range, rc::Rc};

use gpui::StatefulInteractiveElement as _;
use gpui::{
    App, AppContext as _, Context, Empty, Entity, FocusHandle, Focusable, Half,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Render, Styled,
    Subscription, Window, actions, div, prelude::FluentBuilder as _,
};
use ropey::Rope;

use crate::{
    ActiveTheme, Disableable, ElementExt, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{
        Enter, Escape, IndentInline, Input, InputEvent, InputState, OutdentInline, RopeExt as _,
        Search, movement::MoveDirection,
    },
    label::Label,
    v_flex,
};

const CONTEXT: &'static str = "SearchPanel";

actions!(input, [Tab]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("escape", Escape, Some(CONTEXT)),
        // The code editor's ancestor Input context otherwise consumes Return
        // as Find Next before the focused Button receives its keyboard click.
        gpui::KeyBinding::new("enter", gpui::NoAction, Some("SearchPanelButton")),
    ]);
}

#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<AhoCorasick>,

    pub(super) matched_ranges: Rc<Vec<Range<usize>>>,
    pub(super) current_match_ix: usize,
    /// Is in replacing mode, if true, the next update will update the current match index based on matched ranges.
    replacing: bool,
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            matched_ranges: Rc::new(Vec::new()),
            current_match_ix: 0,
            replacing: false,
        }
    }

    /// Update source text and re-match
    pub(crate) fn update(&mut self, text: &Rope) {
        if self.text.eq(text) {
            return;
        }

        self.text = text.clone();
        self.update_matches();
    }

    fn update_matches(&mut self) {
        let mut new_ranges = Vec::new();
        if let Some(query) = &self.query {
            let text = self.text.to_string();
            // FIXME: Use stream find
            let matches = query.stream_find_iter(text.as_bytes());

            for query_match in matches.into_iter() {
                let query_match = query_match.expect("query match for select all action");
                new_ranges.push(query_match.range());
            }
        }
        self.matched_ranges = Rc::new(new_ranges);
        if !self.replacing {
            self.current_match_ix = 0;
        } else if self.matched_ranges.is_empty() {
            self.current_match_ix = 0;
        } else {
            self.current_match_ix = self.current_match_ix.min(self.matched_ranges.len() - 1);
        }
        self.replacing = false;
    }

    /// Update the search query and reset the current match index.
    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        if query.len() > 0 {
            self.query = Some(
                AhoCorasick::builder()
                    .ascii_case_insensitive(case_insensitive)
                    .build(&[query.to_string()])
                    .expect("failed to build AhoCorasick query in SearchMatcher"),
            );
        } else {
            self.query = None;
        }
        self.update_matches();
    }

    /// Returns the number of matches found.
    #[allow(unused)]
    #[inline]
    fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    fn peek(&self) -> Option<Range<usize>> {
        let next_match_ix = self.next_ix()?;
        self.matched_ranges.get(next_match_ix).cloned()
    }

    fn next_ix(&self) -> Option<usize> {
        if self.matched_ranges.is_empty() {
            None
        } else if self.has_next_match_without_wrap() {
            Some(self.current_match_ix + 1)
        } else {
            Some(0)
        }
    }

    fn has_next_match_without_wrap(&self) -> bool {
        self.current_match_ix < self.matched_ranges.len().saturating_sub(1)
    }

    fn label(&self) -> String {
        if self.len() == 0 {
            return "0/0".to_string();
        }
        format!("{}/{}", self.current_match_ix + 1, self.len())
    }

    /// Update the current match index based on the given offset.
    fn update_cursor_by_offset(&mut self, offset: usize) {
        for (ix, range) in self.matched_ranges.iter().enumerate() {
            self.current_match_ix = ix;
            if range.contains(&offset) || range.end >= offset {
                return;
            }
        }
    }
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let next_match_ix = self.next_ix()?;
        self.current_match_ix = next_match_ix;
        self.matched_ranges.get(next_match_ix).cloned()
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.matched_ranges.is_empty() {
            return None;
        }

        if self.current_match_ix == 0 {
            self.current_match_ix = self.matched_ranges.len();
        }

        self.current_match_ix -= 1;
        let item = self.matched_ranges[self.current_match_ix].clone();

        Some(item)
    }
}

pub(super) struct SearchPanel {
    pub(super) labels: super::SearchPanelLabels,
    #[cfg(feature = "test-support")]
    pub(super) probe: Rc<std::cell::RefCell<super::SearchPanelProbe>>,
    editor: Entity<InputState>,
    focus_handle: FocusHandle,
    search_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    case_insensitive: bool,
    replace_mode: bool,
    matcher: SearchMatcher,
    input_width: Pixels,

    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl InputState {
    /// Update the search matcher when text changes.
    pub(super) fn update_search(&mut self, cx: &mut App) {
        let Some(search_panel) = self.search_panel.as_ref() else {
            return;
        };

        let text = self.text.clone();
        search_panel.update(cx, |this, _| {
            this.matcher.update(&text);
        });
    }

    pub(super) fn on_action_search(
        &mut self,
        _: &Search,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.searchable {
            return;
        }

        let search_panel = match self.search_panel.as_ref() {
            Some(panel) => panel.clone(),
            None => SearchPanel::new(cx.entity(), self.search_labels.clone(), window, cx),
        };

        let text = self.text.clone();
        let editor = cx.entity();
        let selected_text = Rope::from(self.selected_text());
        search_panel.update(cx, |this, cx| {
            this.editor = editor;
            this.matcher.update(&text);
            this.show(&selected_text, window, cx);
        });
        self.search_panel = Some(search_panel);
        cx.notify();
    }
}

impl SearchPanel {
    #[cfg(feature = "test-support")]
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
    #[cfg(feature = "test-support")]
    pub(super) fn is_replacing(&self) -> bool {
        self.replace_mode
    }
    #[cfg(feature = "test-support")]
    pub(super) fn is_case_sensitive(&self) -> bool {
        !self.case_insensitive
    }
    #[cfg(feature = "test-support")]
    pub(super) fn match_position(&self) -> (usize, usize) {
        (self.matcher.current_match_ix, self.matcher.len())
    }

    fn next_scroll_direction(
        previous_match_ix: usize,
        current_match_ix: usize,
    ) -> Option<MoveDirection> {
        if current_match_ix <= previous_match_ix {
            None
        } else {
            Some(MoveDirection::Down)
        }
    }

    fn prev_scroll_direction(
        previous_match_ix: usize,
        current_match_ix: usize,
    ) -> Option<MoveDirection> {
        if current_match_ix >= previous_match_ix {
            None
        } else {
            Some(MoveDirection::Up)
        }
    }

    pub fn new(
        editor: Entity<InputState>,
        labels: super::SearchPanelLabels,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let replace_input = cx.new(|cx| InputState::new(window, cx));

        cx.new(|cx| {
            let _subscriptions =
                vec![
                    cx.subscribe(&search_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                        // Handle search input changes
                        match ev {
                            InputEvent::Change => {
                                this.update_search_query(cx);
                            }
                            _ => {}
                        }
                    }),
                ];

            Self {
                labels,
                #[cfg(feature = "test-support")]
                probe: Rc::default(),
                editor,
                focus_handle: cx.focus_handle().tab_stop(false),
                search_input,
                replace_input,
                case_insensitive: true,
                replace_mode: false,
                matcher: SearchMatcher::new(),
                open: true,
                input_width: Pixels::ZERO,
                _subscriptions,
            }
        })
    }

    pub(super) fn show(
        &mut self,
        selected_text: &Rope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = true;
        self.search_input
            .read(cx)
            .focus_handle
            .clone()
            .focus(window, cx);

        self.search_input.update(cx, |this, cx| {
            if selected_text.len() > 0 {
                // Set value will emit to update_search_query
                this.set_value(selected_text.to_string(), window, cx);
            }
            this.select_all(&super::SelectAll, window, cx);
        });
    }

    fn update_search_query(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value();
        let visible_range_offset = self
            .editor
            .read(cx)
            .last_layout
            .as_ref()
            .map(|l| l.visible_range_offset.clone());

        self.matcher
            .update_query(query.as_str(), self.case_insensitive);

        if let Some(visible_range_offset) = visible_range_offset {
            self.matcher
                .update_cursor_by_offset(visible_range_offset.start);
        }
        cx.notify();
    }

    fn replaceable(&self, cx: &App) -> bool {
        let editor = self.editor.read(cx);
        editor.replaceable
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.editor.read(cx).focus_handle.clone().focus(window, cx);
        cx.notify();
    }

    fn on_action_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if action.shift {
            self.prev(window, cx);
        } else {
            self.next(window, cx);
        }
    }

    fn on_action_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    fn on_action_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn on_action_shift_tab(
        &mut self,
        _: &OutdentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
    }

    fn prev(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let previous_match_ix = self.matcher.current_match_ix;
        if let Some(range) = self.matcher.next_back() {
            let direction =
                Self::prev_scroll_direction(previous_match_ix, self.matcher.current_match_ix);
            self.editor.update(cx, |state, cx| {
                state.scroll_to(range.start, direction, cx);
            });
        }
    }

    fn next(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let previous_match_ix = self.matcher.current_match_ix;
        if let Some(range) = self.matcher.next() {
            let direction =
                Self::next_scroll_direction(previous_match_ix, self.matcher.current_match_ix);
            self.editor.update(cx, |state, cx| {
                state.scroll_to(range.end, direction, cx);
            });
        }
    }

    pub(super) fn matcher(&self) -> Option<&SearchMatcher> {
        if !self.open {
            return None;
        }

        Some(&self.matcher)
    }

    fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.replace_mode = false;
            cx.notify();
            return;
        }

        let new_text = self.replace_input.read(cx).value();
        self.matcher.replacing = true;
        let previous_match_ix = self.matcher.current_match_ix;
        if let Some(range) = self
            .matcher
            .matched_ranges
            .get(self.matcher.current_match_ix)
            .cloned()
        {
            if self.matcher.len() == 1 {
                self.replace_input.focus_handle(cx).focus(window, cx);
            }
            let text_state = self.editor.clone();
            let next_match_ix = self.matcher.next_ix().unwrap_or(previous_match_ix);
            let next_range = self.matcher.peek().unwrap_or(range.clone());
            self.matcher.current_match_ix = next_match_ix;
            let direction = Self::next_scroll_direction(previous_match_ix, next_match_ix);
            cx.spawn_in(window, async move |_, cx| {
                cx.update(|window, cx| {
                    text_state.update(cx, |state, cx| {
                        let range_utf16 = state.range_to_utf16(&range);
                        state.scroll_to(next_range.end, direction, cx);
                        state.replace_text_in_range_silent(
                            Some(range_utf16),
                            new_text.as_str(),
                            window,
                            cx,
                        );
                    });
                })
            })
            .detach();
        }
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.replace_mode = false;
            cx.notify();
            return;
        }

        let new_text = self.replace_input.read(cx).value();
        self.matcher.replacing = true;
        let ranges = self.matcher.matched_ranges.clone();
        if ranges.is_empty() {
            return;
        }
        // The action may become disabled on the next frame. Keep keyboard
        // ownership in the bar rather than on an untracked disabled Button.
        self.replace_input.focus_handle(cx).focus(window, cx);

        let editor = self.editor.clone();
        cx.spawn_in(window, async move |_, cx| {
            cx.update(|window, cx| {
                editor.update(cx, |state, cx| {
                    // Replace from the end to avoid messing up the ranges.
                    let mut rope = state.text.clone();
                    for range in ranges.iter().rev() {
                        rope.replace(range.clone(), new_text.as_str());
                    }
                    state.replace_text_in_range_silent(
                        Some(0..state.text.len()),
                        &rope.to_string(),
                        window,
                        cx,
                    );
                    state.scroll_to(0, Some(MoveDirection::Down), cx);
                });
            })
        })
        .detach();
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return Empty.into_any_element();
        }

        let has_matches = self.matcher.len() > 0;
        let allow_replace = self.replaceable(cx);
        if !allow_replace {
            self.replace_mode = false;
        }
        #[cfg(feature = "test-support")]
        {
            let mut probe = self.probe.borrow_mut();
            probe.controls.clear();
            probe.bounds.clear();
        }

        v_flex()
            .id("search-panel")
            .occlude()
            .track_focus(&self.focus_handle(cx))
            .key_context(CONTEXT)
            .map(super::search_actions::contain_editor_actions)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // A field or Button prevents default to retain its own
                    // focus/selection. Unclaimed padding belongs to Find, not
                    // the non-Tab-stop container (regardless of listener order).
                    if this.focus_handle.is_focused(window) || !window.default_prevented() {
                        this.search_input.focus_handle(cx).focus(window, cx);
                        window.prevent_default();
                    }
                    // The bar overlays an Input: never select code underneath it.
                    cx.stop_propagation();
                }),
            )
            .map(|this| {
                #[cfg(feature = "test-support")]
                let this = this.on_prepaint({
                    let probe = self.probe.clone();
                    move |bounds, _, _| {
                        probe.borrow_mut().bounds.insert("panel", bounds);
                    }
                });
                this
            })
            .on_action(cx.listener(Self::on_action_enter))
            .on_action(cx.listener(Self::on_action_escape))
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_shift_tab))
            .font_family(cx.theme().font_family.clone())
            .items_center()
            .py_2()
            .px_3()
            .w_full()
            .gap_1()
            .bg(cx.theme().tokens.popover)
            .border_b_1()
            .rounded(cx.theme().radius.half())
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .id("search-field-group")
                            .flex()
                            .flex_1()
                            .gap_1()
                            .role(gpui::Role::Group)
                            .aria_label(self.labels.search_field.clone())
                            .child(
                                Input::new(&self.search_input)
                                    .focus_bordered(false)
                                    .suffix(
                                        Button::new("case-insensitive")
                                            .map(|button| {
                                                self.control(
                                                    button,
                                                    "case",
                                                    self.labels.case_sensitive.clone(),
                                                    Some(!self.case_insensitive),
                                                )
                                            })
                                            .selected(!self.case_insensitive)
                                            .xsmall()
                                            .compact()
                                            .ghost()
                                            .icon(IconName::CaseSensitive)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.case_insensitive = !this.case_insensitive;
                                                this.update_search_query(cx);
                                                cx.notify();
                                            })),
                                    )
                                    .small()
                                    .w_full()
                                    .shadow_none(),
                            )
                            .on_prepaint({
                                let view = cx.entity();
                                move |bounds, _, cx| {
                                    view.update(cx, |r, _| r.input_width = bounds.size.width)
                                }
                            }),
                    )
                    .when(allow_replace, |this| {
                        this.child(
                            Button::new("replace-mode")
                                .map(|button| {
                                    self.control(
                                        button,
                                        "mode",
                                        self.labels.replace_mode.clone(),
                                        Some(self.replace_mode),
                                    )
                                })
                                .xsmall()
                                .ghost()
                                .icon(IconName::Replace)
                                .selected(self.replace_mode)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_mode = !this.replace_mode;
                                    if this.replace_mode {
                                        this.replace_input
                                            .read(cx)
                                            .focus_handle
                                            .clone()
                                            .focus(window, cx);
                                    } else {
                                        this.search_input
                                            .read(cx)
                                            .focus_handle
                                            .clone()
                                            .focus(window, cx);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("prev")
                            .map(|button| {
                                self.control(button, "prev", self.labels.previous.clone(), None)
                            })
                            .xsmall()
                            .ghost()
                            .icon(IconName::ChevronLeft)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prev(window, cx);
                            })),
                    )
                    .child(
                        Button::new("next")
                            .map(|button| {
                                self.control(button, "next", self.labels.next.clone(), None)
                            })
                            .xsmall()
                            .ghost()
                            .icon(IconName::ChevronRight)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.next(window, cx);
                            })),
                    )
                    .child(
                        Label::new(self.matcher.label())
                            .when(!has_matches, |this| {
                                this.text_color(cx.theme().muted_foreground)
                            })
                            .text_left()
                            .min_w_16(),
                    )
                    .child(div().w_7())
                    .child(
                        Button::new("close")
                            .map(|button| {
                                self.control(button, "close", self.labels.close.clone(), None)
                            })
                            .xsmall()
                            .ghost()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_action_escape(&Escape, window, cx);
                            })),
                    ),
            )
            .when(self.replace_mode && allow_replace, |this| {
                this.child(
                    h_flex()
                        .id("replacement-field-group")
                        .w_full()
                        .gap_2()
                        .role(gpui::Role::Group)
                        .aria_label(self.labels.replace_field.clone())
                        .child(
                            Input::new(&self.replace_input)
                                .focus_bordered(false)
                                .small()
                                .w(self.input_width)
                                .shadow_none(),
                        )
                        .child(
                            Button::new("replace-one")
                                .map(|button| {
                                    self.control(
                                        button,
                                        "replace",
                                        self.labels.replace.clone(),
                                        None,
                                    )
                                })
                                .small()
                                .label(self.labels.replace.clone())
                                .disabled(!has_matches)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_next(window, cx);
                                })),
                        )
                        .child(
                            Button::new("replace-all")
                                .map(|button| {
                                    self.control(
                                        button,
                                        "all",
                                        self.labels.replace_all.clone(),
                                        None,
                                    )
                                })
                                .small()
                                .label(self.labels.replace_all.clone())
                                .disabled(!has_matches)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_all(window, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
