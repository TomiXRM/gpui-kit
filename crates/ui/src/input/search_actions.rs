//! Find controls must not inherit editing from the containing code Input.
use super::state::*;
use crate::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use gpui::{Action, App, InteractiveElement, Window};

fn consume<A: Action>(_: &A, _: &mut Window, cx: &mut App) {
    cx.stop_propagation();
}

/// Fields consume their own actions first. Padding and Buttons consume the
/// remainder here, before it can reach the hidden owning editor's Input.
pub(super) fn contain_editor_actions<E: InteractiveElement>(element: E) -> E {
    element
        .on_action(consume::<Backspace>)
        .on_action(consume::<Delete>)
        .on_action(consume::<DeleteToBeginningOfLine>)
        .on_action(consume::<DeleteToEndOfLine>)
        .on_action(consume::<DeleteToPreviousWordStart>)
        .on_action(consume::<DeleteToNextWordEnd>)
        .on_action(consume::<Paste>)
        .on_action(consume::<Cut>)
        .on_action(consume::<Copy>)
        .on_action(consume::<Undo>)
        .on_action(consume::<Redo>)
        .on_action(consume::<Indent>)
        .on_action(consume::<Outdent>)
        .on_action(consume::<MoveLeft>)
        .on_action(consume::<MoveRight>)
        .on_action(consume::<MoveUp>)
        .on_action(consume::<MoveDown>)
        .on_action(consume::<MoveHome>)
        .on_action(consume::<MoveEnd>)
        .on_action(consume::<MovePageUp>)
        .on_action(consume::<MovePageDown>)
        .on_action(consume::<MoveToStartOfLine>)
        .on_action(consume::<MoveToEndOfLine>)
        .on_action(consume::<MoveToStart>)
        .on_action(consume::<MoveToEnd>)
        .on_action(consume::<MoveToPreviousWord>)
        .on_action(consume::<MoveToNextWord>)
        .on_action(consume::<SelectLeft>)
        .on_action(consume::<SelectRight>)
        .on_action(consume::<SelectUp>)
        .on_action(consume::<SelectDown>)
        .on_action(consume::<SelectAll>)
        .on_action(consume::<SelectToStartOfLine>)
        .on_action(consume::<SelectToEndOfLine>)
        .on_action(consume::<SelectToStart>)
        .on_action(consume::<SelectToEnd>)
        .on_action(consume::<SelectToPreviousWordStart>)
        .on_action(consume::<SelectToNextWordEnd>)
        .on_action(consume::<ShowCharacterPalette>)
        .on_action(consume::<ToggleCodeActions>)
        .on_action(consume::<GoToDefinition>)
}
