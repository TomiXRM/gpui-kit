use super::search::SearchPanel;
use crate::button::Button;
#[cfg(feature = "test-support")]
use crate::tooltip::Tooltip;
use gpui::InteractiveElement as _;
use gpui::{Role, SharedString, StatefulInteractiveElement as _};

impl SearchPanel {
    pub(super) fn control(
        &self,
        button: Button,
        id: &'static str,
        name: SharedString,
        toggled: Option<bool>,
    ) -> Button {
        let button = super::search_actions::contain_editor_actions(button);
        let button = button
            .role(Role::Button)
            .key_context("SearchPanelButton")
            .aria_label(name.clone());
        let button = match toggled {
            Some(toggled) => button.aria_toggled(toggled.into()),
            None => button,
        };
        #[cfg(feature = "test-support")]
        let button = {
            self.probe.borrow_mut().controls.insert(
                id,
                super::SearchControlProbe {
                    role: Role::Button,
                    name: Some(name.clone()),
                    toggled,
                },
            );
            let help_probe = self.probe.clone();
            let bounds_probe = self.probe.clone();
            let focus_probe = self.probe.clone();
            button
                .bounds_probe(move |bounds| {
                    bounds_probe.borrow_mut().bounds.insert(id, bounds);
                })
                .on_key_down(move |event, _, cx| {
                    if event.keystroke.key == "f19" {
                        focus_probe.borrow_mut().focused_control = Some(id);
                        cx.stop_propagation();
                    }
                })
                .tooltip_builder(move |window, cx| {
                    help_probe.borrow_mut().help.insert(id, name.clone());
                    Tooltip::new(name.clone()).build(window, cx)
                })
        };
        #[cfg(not(feature = "test-support"))]
        let button = {
            let _ = id;
            button.tooltip(name)
        };
        button
    }
}
