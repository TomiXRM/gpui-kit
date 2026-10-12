//! Opt-in observations from the real SearchPanel renderer and tooltip builders.
use gpui::{App, Role, SharedString};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct SearchControlProbe {
    pub role: Role,
    pub name: Option<SharedString>,
    pub toggled: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchPanelProbe {
    pub controls: BTreeMap<&'static str, SearchControlProbe>,
    pub help: BTreeMap<&'static str, SharedString>,
    pub bounds: BTreeMap<&'static str, gpui::Bounds<gpui::Pixels>>,
    pub focused_control: Option<&'static str>,
    pub open: bool,
    pub replace_mode: bool,
    pub case_sensitive: bool,
    pub match_index: usize,
    pub match_count: usize,
}

impl super::InputState {
    pub fn search_panel_probe(&self, cx: &App) -> Option<SearchPanelProbe> {
        let panel = self.search_panel.as_ref()?.read(cx);
        let mut probe = panel.probe.borrow().clone();
        probe.open = panel.is_open();
        probe.replace_mode = panel.is_replacing();
        probe.case_sensitive = panel.is_case_sensitive();
        (probe.match_index, probe.match_count) = panel.match_position();
        Some(probe)
    }

    pub fn clear_search_help_probe(&self, cx: &App) {
        if let Some(panel) = &self.search_panel {
            panel.read(cx).probe.borrow_mut().help.clear();
        }
    }
}
