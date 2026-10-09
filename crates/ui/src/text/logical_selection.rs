use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    rc::{Rc, Weak},
    time::Duration,
};

use gpui::{
    App, Bounds, Context, Element, ElementId, Entity, EntityId, FocusHandle, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Pixels, Point, Task,
    TextLayout, WeakEntity, WeakFocusHandle, Window, px,
};

use super::{
    TextViewState,
    rendered::{RenderedDocument, RenderedLeaf},
    window_selection::SelectionScope,
};
use crate::{Root, global_state::GlobalState, scroll::AutoScroll};

type EdgeScroll = Rc<dyn Fn(Pixels, &mut Window, &mut App)>;

struct Member {
    key: ElementId,
    view: WeakEntity<TextViewState>,
    revision: usize,
    focus: WeakFocusHandle,
}

struct Group {
    key: ElementId,
    members: Vec<Member>,
    by_view: HashMap<EntityId, usize>,
    by_post: HashMap<ElementId, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Affinity {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    post: ElementId,
    revision: usize,
    leaf: Option<usize>,
    offset: usize,
    affinity: Affinity,
}

// GPUI owns these states at the element's GlobalElementId. Cached replay
// promotes accessed states; actual unmount drops them before end-frame defer.
// The controller holds only weak presence, never prolonging native geometry.
#[derive(Default)]
struct InlinePresence {
    group: RefCell<Weak<RefCell<GroupState>>>,
    index: Cell<usize>,
}

#[derive(Default)]
struct ViewportPresence;

struct Geometry {
    view: EntityId,
    post: ElementId,
    leaf: RenderedLeaf,
    source_range: Range<usize>,
    layout: TextLayout,
    lines: Vec<Bounds<Pixels>>,
    hitbox: Hitbox,
    scope: SelectionScope,
    presence: Weak<InlinePresence>,
}

struct Viewport {
    bounds: Bounds<Pixels>,
    scope: SelectionScope,
    hitbox: Hitbox,
    scroll: EdgeScroll,
    presence: Weak<ViewportPresence>,
}

impl Viewport {
    fn is_live(&self) -> bool {
        self.presence.strong_count() > 0
    }
}

/// The consumer's lifetime authority for one installed group binding.
///
/// Keep a returned lease while that conversation is active. Explicit `retire`
/// and the last consumer clone's Drop immediately clear the actual logical
/// range, current-frame geometry/viewport and edge task, without App/Window.
/// Root is not a consumer clone. An old lease cannot retire a replacement.
#[derive(Clone)]
pub struct TextSelectionGroupRetirement(Rc<RetirementOwner>);

impl TextSelectionGroupRetirement {
    pub fn retire(&self) {
        self.0.state.borrow_mut().retire();
    }
    pub fn is_active(&self) -> bool {
        !self.0.state.borrow().retired
    }
}

struct RetirementOwner {
    state: Rc<RefCell<GroupState>>,
}
impl Drop for RetirementOwner {
    fn drop(&mut self) {
        self.state.borrow_mut().retire();
    }
}

#[derive(Default)]
pub(crate) struct LogicalSelection {
    state: Rc<RefCell<GroupState>>,
    owner: Weak<RetirementOwner>,
}
impl LogicalSelection {
    // Keep membership even when retired: before a new paint, the old managed
    // states must not silently become eligible legacy geometric Copy sources.
    pub(crate) fn contains(&self, view: EntityId) -> bool {
        self.state.borrow().contains(view)
    }
    pub(crate) fn has_active_group(&self) -> bool {
        let selection = self.state.borrow();
        !selection.retired && selection.group.is_some()
    }
}
impl Drop for LogicalSelection {
    fn drop(&mut self) {
        self.state.borrow_mut().retire();
    }
}

#[derive(Default)]
struct GroupState {
    group: Option<Group>,
    anchor: Option<Endpoint>,
    cursor: Option<Endpoint>,
    scope: Option<SelectionScope>,
    dragging: bool,
    did_hit_text: bool,
    pointer: Option<Point<Pixels>>,
    viewport: Option<Viewport>,
    geometry: Vec<Geometry>,
    awaiting_layout: bool,
    edge_task: Option<Task<()>>,
    retired: bool,
    dispatch_focus: Option<(WeakFocusHandle, Option<usize>)>,
}

impl GroupState {
    pub(crate) fn contains(&self, view: EntityId) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| group.by_view.contains_key(&view))
    }

    fn clear(&mut self) {
        self.anchor = None;
        self.cursor = None;
        self.scope = None;
        self.dragging = false;
        self.did_hit_text = false;
        self.pointer = None;
        self.edge_task = None;
        self.dispatch_focus = None;
    }

    fn retire(&mut self) {
        self.retired = true;
        self.clear();
        self.viewport = None;
        self.geometry.clear();
        self.awaiting_layout = false;
    }

    fn retire_unmounted_geometry(&mut self) {
        let mut index = 0;
        self.geometry.retain(|geometry| {
            let Some(presence) = geometry.presence.upgrade() else {
                return false;
            };
            presence.index.set(index);
            index += 1;
            true
        });
        if self
            .viewport
            .as_ref()
            .is_some_and(|viewport| !viewport.is_live())
        {
            self.viewport = None;
        }
    }

    fn ordered(&self) -> Option<(usize, &Endpoint, usize, &Endpoint)> {
        if self.retired {
            return None;
        }
        if !self.did_hit_text {
            return None;
        }
        let group = self.group.as_ref()?;
        let anchor = self.anchor.as_ref()?;
        let cursor = self.cursor.as_ref()?;
        let a = *group.by_post.get(&anchor.post)?;
        let c = *group.by_post.get(&cursor.post)?;
        if (a, anchor.offset) <= (c, cursor.offset) {
            Some((a, anchor, c, cursor))
        } else {
            Some((c, cursor, a, anchor))
        }
    }

    fn endpoint_valid(endpoint: &Endpoint, member: &Member, state: &TextViewState) -> bool {
        let Some(rendered) = state.rendered() else {
            return false;
        };
        if endpoint.post != member.key
            || endpoint.revision != member.revision
            || endpoint.revision != rendered.revision
            || endpoint.offset > rendered.len
        {
            return false;
        }
        if let Some(id) = endpoint.leaf {
            let Some(leaf) = rendered.leaf(id) else {
                return false;
            };
            if endpoint.offset < leaf.start || endpoint.offset > leaf.start + leaf.len {
                return false;
            }
        }
        rendered.is_boundary(endpoint.offset)
    }

    fn valid(&self, active_scope: SelectionScope, cx: &App) -> bool {
        let Some(viewport) = &self.viewport else {
            return false;
        };
        if !viewport.is_live() || viewport.scope != active_scope || self.scope != Some(active_scope)
        {
            return false;
        }
        let Some((a, anchor, c, cursor)) = self.ordered() else {
            return false;
        };
        let Some(group) = self.group.as_ref() else {
            return false;
        };
        for (ix, member) in group.members.iter().enumerate().take(c + 1).skip(a) {
            let Some(view) = member.view.upgrade() else {
                return false;
            };
            let state = view.read(cx);
            if state.rendered().map(|document| document.revision) != Some(member.revision) {
                return false;
            }
            if ix == a && !Self::endpoint_valid(anchor, member, state) {
                return false;
            }
            if ix == c && !Self::endpoint_valid(cursor, member, state) {
                return false;
            }
        }
        true
    }

    fn hit(
        &self,
        position: Point<Pixels>,
        window: &Window,
        cx: &App,
    ) -> Option<(Endpoint, EntityId, bool)> {
        if self.retired {
            return None;
        }
        if self.awaiting_layout {
            return None;
        }
        let viewport = self.viewport.as_ref()?;
        if !viewport.is_live() {
            return None;
        }
        let scope = viewport.scope;
        let group = self.group.as_ref()?;
        let mut nearest: Option<(&Geometry, Bounds<Pixels>, Pixels)> = None;
        for geometry in &self.geometry {
            if geometry.scope != scope || geometry.presence.strong_count() == 0 {
                continue;
            }
            let Some(index) = group.by_view.get(&geometry.view) else {
                continue;
            };
            let member = &group.members[*index];
            if member.revision != geometry.leaf.revision {
                continue;
            }
            let Some(view) = member.view.upgrade() else {
                continue;
            };
            let state = view.read(cx);
            let Some(rendered) = state.rendered() else {
                continue;
            };
            if rendered.revision != member.revision {
                continue;
            }
            for bounds in &geometry.lines {
                if bounds.contains(&position) && geometry.hitbox.is_hovered(window) {
                    return Some((
                        geometry.nearest_endpoint(position, rendered)?,
                        geometry.view,
                        true,
                    ));
                }
                let dy = if position.y < bounds.top() {
                    bounds.top() - position.y
                } else if position.y > bounds.bottom() {
                    position.y - bounds.bottom()
                } else {
                    px(0.)
                };
                let dx = if position.x < bounds.left() {
                    bounds.left() - position.x
                } else if position.x > bounds.right() {
                    position.x - bounds.right()
                } else {
                    px(0.)
                };
                let distance = dy * 1000. + dx;
                if nearest.as_ref().is_none_or(|(_, _, old)| distance < *old) {
                    nearest = Some((geometry, *bounds, distance));
                }
            }
        }
        let (geometry, bounds, _) = nearest?;
        let point = gpui::point(
            position.x.clamp(bounds.left(), bounds.right()),
            position.y.clamp(bounds.top(), bounds.bottom() - px(0.01)),
        );
        let member = &group.members[*group.by_view.get(&geometry.view)?];
        let view = member.view.upgrade()?;
        let state = view.read(cx);
        Some((
            geometry.nearest_endpoint(point, state.rendered()?)?,
            geometry.view,
            false,
        ))
    }
}

impl Geometry {
    fn nearest_endpoint(
        &self,
        position: Point<Pixels>,
        document: &RenderedDocument,
    ) -> Option<Endpoint> {
        if self.layout.len() != self.source_range.len() {
            return None;
        }
        let containing = self
            .layout
            .index_for_position(position)
            .unwrap_or_else(|index| index);
        let leaf_containing = self.source_range.start + containing;
        let hard_start = document
            .caret_line_start(self.leaf, leaf_containing)?
            .max(self.source_range.start)
            - self.source_range.start;
        let line = self.layout.line_layout_for_index(containing)?;
        let origin = self.layout.position_for_index(hard_start)?;
        let height = self.layout.line_height();
        if height <= px(0.) {
            return None;
        }
        let relative = position - origin;
        let shaped = line
            .closest_index_for_position(relative, height)
            .unwrap_or_else(|index| index);
        let row = ((relative.y / height).max(0.) as usize).min(line.wrap_boundaries.len());
        let wrap_start_x = if row == 0 {
            px(0.)
        } else {
            let boundary = &line.wrap_boundaries[row - 1];
            line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix]
                .position
                .x
        };
        let (a, b) = document.caret_bounds(self.leaf, leaf_containing)?;
        let (c, d) =
            document.caret_bounds(self.leaf, self.source_range.start + hard_start + shaped)?;
        let candidates = [a, b, c, d];
        let mut nearest = None;
        for (ix, offset) in candidates.iter().copied().enumerate() {
            if candidates[..ix].contains(&offset) {
                continue;
            }
            let local = offset
                .saturating_sub(self.source_range.start)
                .min(self.source_range.len());
            let line_index = local.saturating_sub(hard_start).min(line.len());
            // Use the current visual row's shaped caret x, including its wrap
            // affinity. Comparing both containing and closest boundaries also
            // accounts for GPUI's final-glyph closest-index end shortcut.
            let x = origin.x + line.unwrapped_layout.x_for_index(line_index) - wrap_start_x;
            let distance = (position.x - x).abs();
            if nearest.as_ref().is_none_or(|(_, old)| distance < *old) {
                nearest = Some((offset, distance));
            }
        }
        let offset = nearest?.0;
        Some(Endpoint {
            post: self.post.clone(),
            revision: self.leaf.revision,
            leaf: Some(self.leaf.id),
            offset: self.leaf.start + offset,
            affinity: if offset == self.leaf.len {
                Affinity::After
            } else {
                Affinity::Before
            },
        })
    }
}

impl Root {
    /// Install one ordered managed-text group and keep the returned consumer
    /// lease while it is active. Root retains only weak text entities and a weak
    /// lease owner. Call at accepted-content/activation seams, not during Render.
    /// Unchanged bindings return another clone of the same live lease. Changed
    /// bindings retire the old lease and native geometry; only a valid range in
    /// the same group/scope with an unchanged selected stable-key/source sequence
    /// survives until the replacement's viewport is painted.
    #[must_use = "Keep the returned lease while this conversation is active"]
    pub fn set_text_selection_group(
        &mut self,
        group: Option<(ElementId, &[(ElementId, Entity<TextViewState>)])>,
        cx: &mut Context<Self>,
    ) -> Option<TextSelectionGroupRetirement> {
        let unchanged =
            {
                let selection = self.logical_selection.state.borrow();
                !selection.retired
                    && match (&selection.group, &group) {
                        (None, None) => true,
                        (Some(old), Some((key, posts))) => {
                            old.key == *key
                                && old.members.len() == posts.len()
                                && old.members.iter().zip(posts.iter()).all(
                                    |(member, (post, view))| {
                                        member.key == *post
                                            && member.view.entity_id() == view.entity_id()
                                            && view.read(cx).rendered().is_some_and(|document| {
                                                document.revision == member.revision
                                            })
                                    },
                                )
                        }
                        _ => false,
                    }
            };
        if unchanged {
            return self
                .logical_selection
                .owner
                .upgrade()
                .map(TextSelectionGroupRetirement);
        }
        let replacement = group.and_then(|(key, posts)| {
            let mut keys = HashMap::with_capacity(posts.len());
            let mut by_view = HashMap::with_capacity(posts.len());
            let mut members = Vec::with_capacity(posts.len());
            for (index, (post, view)) in posts.iter().enumerate() {
                if keys.insert(post.clone(), index).is_some()
                    || by_view.insert(view.entity_id(), index).is_some()
                {
                    return None;
                }
                let state = view.read(cx);
                let revision = state.rendered()?.revision;
                let focus = state.focus_handle.downgrade();
                members.push(Member {
                    key: post.clone(),
                    view: view.downgrade(),
                    revision,
                    focus,
                });
            }
            Some(Group {
                key,
                members,
                by_view,
                by_post: keys,
            })
        });
        let mut next = GroupState {
            group: replacement,
            ..GroupState::default()
        };
        let preserve = {
            let selection = self.logical_selection.state.borrow();
            self.logical_selection.owner.strong_count() > 0
                && selection.valid(self.active_selection_scope(), cx)
                && match (&selection.group, &next.group, selection.ordered()) {
                    (Some(old), Some(new), Some((a, start, c, end))) if old.key == new.key => new
                        .by_post
                        .get(&start.post)
                        .zip(new.by_post.get(&end.post))
                        .is_some_and(|(&new_a, &new_c)| {
                            new_a <= new_c
                                && c - a == new_c - new_a
                                && old.members[a..=c]
                                    .iter()
                                    .zip(&new.members[new_a..=new_c])
                                    .all(|(old, new)| {
                                        old.key == new.key
                                            && old.view.entity_id() == new.view.entity_id()
                                            && old.revision == new.revision
                                    })
                        }),
                    _ => false,
                }
        };
        if preserve {
            self.notify_logical_geometry(cx);
            let mut selection = self.logical_selection.state.borrow_mut();
            // Ordinals outside the selected interval may change. Move only
            // logical endpoints, never old hitboxes, viewport or drag/task.
            next.anchor = selection.anchor.take();
            next.cursor = selection.cursor.take();
            next.scope = selection.scope;
            next.did_hit_text = selection.did_hit_text;
        }
        self.clear_text_selection(cx);
        self.retire_text_selection_group();
        let old = self.logical_selection.state.borrow_mut().group.take();
        if let Some(old) = old {
            for member in old.members {
                if let Some(view) = member.view.upgrade() {
                    view.update(cx, |state, _| state.selection_root = None);
                }
            }
        }
        self.logical_selection.state = Rc::new(RefCell::new(next));
        self.logical_selection.owner = Weak::new();
        let weak_root = cx.entity().downgrade();
        let has_group = {
            let selection = self.logical_selection.state.borrow();
            if let Some(group) = &selection.group {
                for member in &group.members {
                    self.selectable_text_views.remove(&member.view.entity_id());
                    self.selectable_text_inlines
                        .remove(&member.view.entity_id());
                    if let Some(view) = member.view.upgrade() {
                        view.update(cx, |state, cx| {
                            state.selection_root = Some(weak_root.clone());
                            state.clear_local_selection(cx);
                        });
                    }
                }
                true
            } else {
                false
            }
        };
        cx.notify();
        if !has_group {
            return None;
        }
        let owner = Rc::new(RetirementOwner {
            state: self.logical_selection.state.clone(),
        });
        self.logical_selection.owner = Rc::downgrade(&owner);
        Some(TextSelectionGroupRetirement(owner))
    }

    pub(crate) fn retire_text_selection_group(&mut self) {
        self.logical_selection.state.borrow_mut().retire();
    }

    pub(crate) fn clear_logical_selection(&mut self, cx: &mut Context<Self>) {
        let had_range = {
            let mut selection = self.logical_selection.state.borrow_mut();
            let had_range = selection.anchor.is_some();
            selection.clear();
            had_range
        };
        if had_range {
            self.notify_logical_geometry(cx);
        }
    }

    pub(crate) fn invalidate_logical_post(&mut self, view: EntityId, cx: &mut Context<Self>) {
        let affected = {
            let mut selection = self.logical_selection.state.borrow_mut();
            if !selection.contains(view) {
                return;
            }
            selection.geometry.retain(|geometry| geometry.view != view);
            selection.dragging
                || selection.ordered().is_some_and(|(a, _, c, _)| {
                    selection
                        .group
                        .as_ref()
                        .and_then(|group| group.by_view.get(&view))
                        .is_some_and(|index| *index >= a && *index <= c)
                })
        };
        if affected {
            self.clear_logical_selection(cx);
        }
    }

    fn notify_logical_geometry(&self, cx: &mut Context<Self>) {
        let selection = self.logical_selection.state.borrow();
        let mut previous = None;
        for geometry in &selection.geometry {
            if previous == Some(geometry.view) {
                continue;
            }
            App::notify(cx, geometry.view);
            previous = Some(geometry.view);
        }
    }

    pub(crate) fn clear_group_local_selections(&self, cx: &mut Context<Self>) {
        let selection = self.logical_selection.state.borrow();
        let Some(group) = &selection.group else {
            return;
        };
        for member in &group.members {
            if let Some(view) = member.view.upgrade() {
                if view.read(cx).has_view_selection() {
                    view.update(cx, |state, cx| state.clear_local_selection(cx));
                }
            }
        }
    }

    fn group_dispatch_focus(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        let active_scope = self.active_selection_scope();
        let focus = window.focused(cx)?;
        let mut selection = self.logical_selection.state.borrow_mut();
        if selection.retired
            || selection.scope != Some(active_scope)
            || self.logical_selection.owner.strong_count() == 0
        {
            return None;
        }
        // Focus IDs are private in GPUI. Compare its public weak handles only
        // when focus changes, not once per post on every conversation frame.
        let index = match selection.dispatch_focus.as_ref() {
            Some((cached, index)) if cached == &focus => *index,
            _ => {
                let index = selection
                    .group
                    .as_ref()?
                    .members
                    .iter()
                    .position(|member| member.focus == focus);
                selection.dispatch_focus = Some((focus.downgrade(), index));
                index
            }
        }?;
        let member = selection.group.as_ref()?.members.get(index)?;
        let view = member.view.upgrade()?;
        if view.read(cx).rendered()?.revision != member.revision {
            return None;
        }
        Some(focus)
    }

    pub(crate) fn finish_text_selection_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active_scope = self.active_selection_scope();
        // Frame::finish has promoted cached accessed states and dropped every
        // unmounted state before this whole-draw deferred callback executes.
        self.logical_selection
            .state
            .borrow_mut()
            .retire_unmounted_geometry();
        let (retired, eligible, scope_changed) = {
            let selection = self.logical_selection.state.borrow();
            (
                selection.retired,
                selection
                    .viewport
                    .as_ref()
                    .is_some_and(|viewport| viewport.is_live() && viewport.scope == active_scope),
                selection.scope.is_some_and(|scope| scope != active_scope),
            )
        };
        if retired || scope_changed {
            self.retire_text_selection_group();
            self.clear_group_local_selections(cx);
            return;
        }
        if !eligible {
            // A missing native viewport cancels the drag, not the accepted
            // document/range. Copy/hit stay ineligible until it is present.
            let mut selection = self.logical_selection.state.borrow_mut();
            selection.dragging = false;
            selection.pointer = None;
            selection.edge_task = None;
            selection.geometry.clear();
            return;
        }
        let pointer = {
            let mut selection = self.logical_selection.state.borrow_mut();
            selection.awaiting_layout = false;
            selection.dragging.then_some(selection.pointer).flatten()
        };
        if let Some(position) = pointer {
            self.update_logical_selection(position, window, cx);
        }
    }

    // Keep GPUI paint identity, accepted-source range and native layout/hitbox
    // separate here: Geometry's owner/scope/presence is resolved only after
    // registration guards pass. A second paint-payload type would only bundle args.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn register_logical_inline(
        global_id: &GlobalElementId,
        view: &Entity<TextViewState>,
        leaf: RenderedLeaf,
        source_range: Range<usize>,
        layout: TextLayout,
        lines: Vec<Bounds<Pixels>>,
        hitbox: Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(root) = window.root::<Root>().flatten() else {
            return;
        };
        let scope = GlobalState::global(cx).current_selection_scope();
        let presence =
            window.with_element_state(global_id, |state: Option<Rc<InlinePresence>>, _| {
                let state = state.unwrap_or_default();
                (Rc::downgrade(&state), state)
            });
        root.update(cx, |root, _| {
            let mut selection = root.logical_selection.state.borrow_mut();
            if selection.retired {
                return;
            }
            let Some(group) = &selection.group else {
                return;
            };
            let Some(index) = group.by_view.get(&view.entity_id()) else {
                return;
            };
            let Some(viewport) = &selection.viewport else {
                return;
            };
            if !viewport.is_live()
                || viewport.scope != scope
                || group.members[*index].revision != leaf.revision
            {
                return;
            }
            let post = group.members[*index].key.clone();
            let mut lines = lines;
            lines.retain_mut(|bounds| {
                *bounds = bounds.intersect(&viewport.bounds);
                bounds.size.width > px(0.) && bounds.size.height > px(0.)
            });
            let Some(slot) = presence.upgrade() else {
                return;
            };
            let geometry = Geometry {
                view: view.entity_id(),
                post,
                leaf,
                source_range,
                layout,
                lines,
                hitbox,
                scope,
                presence,
            };
            let index = slot.index.get();
            let same_group = std::ptr::eq(
                slot.group.borrow().as_ptr(),
                Rc::as_ptr(&root.logical_selection.state),
            );
            if same_group
                && selection
                    .geometry
                    .get(index)
                    .is_some_and(|old| Weak::ptr_eq(&old.presence, &geometry.presence))
            {
                selection.geometry[index] = geometry;
            } else {
                slot.group
                    .replace(Rc::downgrade(&root.logical_selection.state));
                slot.index.set(selection.geometry.len());
                selection.geometry.push(geometry);
            }
        });
    }

    pub(super) fn logical_inline_selection(
        &self,
        view: EntityId,
        leaf: RenderedLeaf,
        source_range: Range<usize>,
        painter_scope: SelectionScope,
    ) -> Option<Range<usize>> {
        let selection = self.logical_selection.state.borrow();
        if painter_scope != self.active_selection_scope()
            || !selection.contains(view)
            || selection.scope != Some(self.active_selection_scope())
            || !selection.viewport.as_ref().is_some_and(|viewport| {
                viewport.is_live() && viewport.scope == self.active_selection_scope()
            })
        {
            return None;
        }
        let (a, start, c, end) = selection.ordered()?;
        let group = selection.group.as_ref()?;
        let index = *group.by_view.get(&view)?;
        if index < a || index > c || group.members[index].revision != leaf.revision {
            return None;
        }
        let start = if index == a { start.offset } else { 0 };
        let end = if index == c { end.offset } else { usize::MAX };
        let fragment_start = leaf.start + source_range.start;
        let fragment_end = leaf.start + source_range.end;
        let start = start.max(fragment_start);
        let end = end.min(fragment_end);
        if start < end {
            Some(start - fragment_start..end - fragment_start)
        } else {
            None
        }
    }

    pub(crate) fn has_logical_selection(&self, cx: &App) -> bool {
        let selection = self.logical_selection.state.borrow();
        selection.valid(self.active_selection_scope(), cx)
            && selection
                .ordered()
                .is_some_and(|(a, start, c, end)| a != c || start.offset != end.offset)
    }

    /// Retired/invalid grouped ranges cannot fall back to stale painted members.
    pub(crate) fn logical_selected_text(&self, cx: &App) -> Option<String> {
        let selection = self.logical_selection.state.borrow();
        selection.anchor.as_ref()?;
        if !selection.valid(self.active_selection_scope(), cx) {
            return Some(String::new());
        }
        let Some((a, start, c, end)) = selection.ordered() else {
            return Some(String::new());
        };
        let Some(group) = selection.group.as_ref() else {
            return Some(String::new());
        };
        let mut text = String::new();
        for index in a..=c {
            let member = &group.members[index];
            let Some(view) = member.view.upgrade() else {
                return Some(String::new());
            };
            let Some(rendered) = view.read(cx).rendered() else {
                return Some(String::new());
            };
            let range = (if index == a { start.offset } else { 0 })..(if index == c {
                end.offset
            } else {
                rendered.len
            });
            match rendered.range_is_whitespace(range.clone()) {
                Some(true) => continue,
                None => return Some(String::new()),
                Some(false) => {}
            }
            if !text.is_empty() {
                text.push('\n');
            }
            if rendered.write_range(range, &mut text).is_none() {
                return Some(String::new());
            }
        }
        Some(text)
    }

    pub(crate) fn start_logical_selection(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let active_scope = self.active_selection_scope();
        let (view_id, inside) = {
            let mut selection = self.logical_selection.state.borrow_mut();
            let Some(viewport) = &selection.viewport else {
                return false;
            };
            if !viewport.is_live()
                || viewport.scope != active_scope
                || !viewport.bounds.contains(&position)
                || !viewport.hitbox.is_hovered(window)
            {
                return false;
            }
            let Some((endpoint, view_id, inside)) = selection.hit(position, window, cx) else {
                return false;
            };
            let scope = viewport.scope;
            selection.scope = Some(scope);
            selection.anchor = Some(endpoint.clone());
            selection.cursor = Some(endpoint);
            selection.dragging = true;
            selection.did_hit_text = inside;
            selection.pointer = Some(position);
            (view_id, inside)
        };
        if inside {
            let view = self
                .logical_selection
                .state
                .borrow()
                .group
                .as_ref()
                .and_then(|group| {
                    group
                        .by_view
                        .get(&view_id)
                        .map(|index| &group.members[*index])
                })
                .and_then(|member| member.view.upgrade());
            if let Some(view) = view {
                let focus = view.read(cx).focus_handle.clone();
                focus.focus(window, cx);
            }
        }
        self.update_logical_edge_task(window, cx);
        true
    }

    pub(crate) fn update_logical_selection(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let active_scope = self.active_selection_scope();
        let changed = {
            let mut selection = self.logical_selection.state.borrow_mut();
            if !selection.dragging {
                return false;
            }
            if cx.has_active_drag() {
                return true;
            }
            if selection.retired || selection.scope != Some(active_scope) {
                drop(selection);
                self.retire_text_selection_group();
                return true;
            }
            selection.pointer = Some(position);
            if let Some((endpoint, _, inside)) = selection.hit(position, window, cx) {
                selection.did_hit_text |= inside;
                if selection.cursor.as_ref() != Some(&endpoint) {
                    selection.cursor = Some(endpoint);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if changed {
            self.notify_logical_geometry(cx);
        }
        self.update_logical_edge_task(window, cx);
        true
    }

    pub(crate) fn end_logical_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let did_hit_text = {
            let mut selection = self.logical_selection.state.borrow_mut();
            if !selection.dragging {
                return false;
            }
            selection.dragging = false;
            selection.edge_task = None;
            selection.pointer = None;
            selection.did_hit_text
        };
        if did_hit_text {
            self.notify_logical_geometry(cx);
        } else {
            self.clear_logical_selection(cx);
        }
        true
    }

    pub(crate) fn logical_scroll_pending(&mut self) -> bool {
        let mut selection = self.logical_selection.state.borrow_mut();
        if selection.retired || !selection.dragging {
            return false;
        }
        selection.awaiting_layout = true;
        true
    }

    fn update_logical_edge_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut selection = self.logical_selection.state.borrow_mut();
        let delta = selection
            .viewport
            .as_ref()
            .filter(|viewport| viewport.is_live())
            .zip(selection.pointer)
            .and_then(|(viewport, position)| {
                AutoScroll::compute_delta(position.y, viewport.bounds)
            });
        if selection.retired || !selection.dragging || !selection.did_hit_text || delta.is_none() {
            selection.edge_task = None;
            return;
        }
        if selection.edge_task.is_some() {
            return;
        }
        // The future owns no lease owner, text entity, or strong Root across
        // await. Last-consumer Drop can always retire and drop this actual Task.
        let weak_root = cx.entity().downgrade();
        selection.edge_task = Some(window.spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let keep = cx
                    .update(|window, cx| {
                        let Some(root) = weak_root.upgrade() else {
                            return false;
                        };
                        let tick = root.update(cx, |root, cx| {
                            let mut selection = root.logical_selection.state.borrow_mut();
                            if !selection.dragging
                                || !selection.valid(root.active_selection_scope(), cx)
                            {
                                let had_range = selection.anchor.is_some();
                                selection.clear();
                                drop(selection);
                                if had_range {
                                    root.notify_logical_geometry(cx);
                                }
                                return None;
                            }
                            let viewport = selection.viewport.as_ref()?;
                            let delta =
                                AutoScroll::compute_delta(selection.pointer?.y, viewport.bounds)?;
                            let scroll = viewport.scroll.clone();
                            selection.awaiting_layout = true;
                            Some((scroll, delta))
                        });
                        let Some((scroll, delta)) = tick else {
                            return false;
                        };
                        scroll(delta, window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    pub(crate) fn select_logical_post(
        &mut self,
        view: EntityId,
        rendered: Option<(usize, usize)>,
        cx: &mut Context<Self>,
    ) -> bool {
        let scope = self.active_selection_scope();
        {
            let mut selection = self.logical_selection.state.borrow_mut();
            let Some(group) = &selection.group else {
                return false;
            };
            let Some(index) = group.by_view.get(&view) else {
                return false;
            };
            let post = group.members[*index].key.clone();
            let expected_revision = group.members[*index].revision;
            let Some((revision, len)) = rendered else {
                selection.clear();
                return true;
            };
            if selection.retired || revision != expected_revision {
                selection.clear();
                return true;
            }
            selection.clear();
            selection.scope = Some(scope);
            selection.did_hit_text = true;
            selection.anchor = Some(Endpoint {
                post: post.clone(),
                revision,
                leaf: None,
                offset: 0,
                affinity: Affinity::Before,
            });
            selection.cursor = Some(Endpoint {
                post,
                revision,
                leaf: None,
                offset: len,
                affinity: Affinity::After,
            });
        }
        self.notify_logical_geometry(cx);
        true
    }

    pub(super) fn select_logical_leaf_range(
        &mut self,
        view: EntityId,
        leaf: RenderedLeaf,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let scope = self.active_selection_scope();
        {
            let mut selection = self.logical_selection.state.borrow_mut();
            let Some(group) = &selection.group else {
                return false;
            };
            let Some(index) = group.by_view.get(&view) else {
                return false;
            };
            let post = group.members[*index].key.clone();
            if selection.retired
                || group.members[*index].revision != leaf.revision
                || range.start > range.end
                || range.end > leaf.len
            {
                selection.clear();
                return true;
            }
            selection.clear();
            selection.scope = Some(scope);
            selection.did_hit_text = true;
            selection.anchor = Some(Endpoint {
                post: post.clone(),
                revision: leaf.revision,
                leaf: Some(leaf.id),
                offset: leaf.start + range.start,
                affinity: Affinity::Before,
            });
            selection.cursor = Some(Endpoint {
                post,
                revision: leaf.revision,
                leaf: Some(leaf.id),
                offset: leaf.start + range.end,
                affinity: Affinity::After,
            });
        }
        self.notify_logical_geometry(cx);
        true
    }
}

/// Attach once to the outer conversation viewport. This marker is layout
/// transparent. The callback scrolls the consumer's existing outer ListState;
/// capture weak owners and check the frozen activation/owner stamp there.
pub trait TextSelectionGroupElement: IntoElement + Sized {
    fn text_selection_group(
        self,
        group: impl Into<ElementId>,
        on_edge_scroll: impl Fn(Pixels, &mut Window, &mut App) + 'static,
    ) -> TextSelectionGroupMarker<Self::Element> {
        TextSelectionGroupMarker {
            element: self.into_element(),
            group: group.into(),
            scroll: Rc::new(on_edge_scroll),
        }
    }
}
impl<E: IntoElement> TextSelectionGroupElement for E {}

pub struct TextSelectionGroupMarker<E> {
    element: E,
    group: ElementId,
    scroll: EdgeScroll,
}

impl<E: Element> IntoElement for TextSelectionGroupMarker<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for TextSelectionGroupMarker<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = (E::PrepaintState, Hitbox);
    fn id(&self) -> Option<ElementId> {
        self.element.id().or_else(|| Some(self.group.clone()))
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let paint = self
            .element
            .prepaint(id, inspector, bounds, layout, window, cx);
        (paint, window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scope = GlobalState::global(cx).current_selection_scope();
        let Some(global_id) = id else {
            return;
        };
        let presence =
            window.with_element_state(global_id, |state: Option<Rc<ViewportPresence>>, _| {
                let state = state.unwrap_or_default();
                (Rc::downgrade(&state), state)
            });
        if let Some(root) = window.root::<Root>().flatten() {
            root.update(cx, |root, _| {
                let mut selection = root.logical_selection.state.borrow_mut();
                if !selection.retired
                    && scope == root.active_selection_scope()
                    && selection
                        .group
                        .as_ref()
                        .is_some_and(|group| group.key == self.group)
                {
                    selection.viewport = Some(Viewport {
                        bounds: bounds.intersect(&window.content_mask().bounds),
                        scope,
                        scroll: self.scroll.clone(),
                        hitbox: paint.1.clone(),
                        presence,
                    });
                }
            });
        }
        self.element
            .paint(id, inspector, bounds, layout, &mut paint.0, window, cx);
    }
}

/// Root's element bracket executes in every actual prepaint/paint, not just
/// Render. Pinned GPUI's on_next_frame callbacks run on the platform frame
/// request before draw and therefore are not a frame-retirement hook.
pub(crate) struct TextSelectionFrame<E>(pub E);
impl<E: Element> IntoElement for TextSelectionFrame<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<E: Element> Element for TextSelectionFrame<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = (E::PrepaintState, bool);
    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.0.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.0.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let mut managed_focus = false;
        if let Some(root) = window.root::<Root>().flatten() {
            if let Some(focus) = root.read(cx).group_dispatch_focus(window, cx) {
                // This is dispatch registration, not a focus change. A visible
                // descendant registers later and wins GPUI's focus-ID mapping.
                window.set_focus_handle(&focus, cx);
                managed_focus = true;
            }
        }
        (
            self.0.prepaint(id, inspector, bounds, layout, window, cx),
            managed_focus,
        )
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        Root::register_group_copy_action(paint.1, window, cx);
        self.0
            .paint(id, inspector, bounds, layout, &mut paint.0, window, cx);
        // Modal children may be deferred until after Root's own paint. Finish
        // retirement after the entire actual draw, not at Render/on_next_frame.
        window.defer(cx, |window, cx| {
            if let Some(root) = window.root::<Root>().flatten() {
                root.update(cx, |root, cx| root.finish_text_selection_frame(window, cx));
            }
        });
    }
}

#[cfg(test)]
#[path = "logical_selection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "logical_selection_refresh_tests.rs"]
mod refresh_tests;
