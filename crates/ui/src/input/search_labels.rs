use gpui::SharedString;

/// Names and help text for the built-in Find/Replace controls.
/// Hosts can supply their locale; the standalone component defaults to English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPanelLabels {
    pub previous: SharedString,
    pub next: SharedString,
    pub replace: SharedString,
    pub replace_all: SharedString,
    pub close: SharedString,
    pub case_sensitive: SharedString,
    pub replace_mode: SharedString,
    pub search_field: SharedString,
    pub replace_field: SharedString,
}

impl Default for SearchPanelLabels {
    fn default() -> Self {
        Self {
            previous: "Previous match".into(),
            next: "Next match".into(),
            replace: "Replace".into(),
            replace_all: "Replace all".into(),
            close: "Close".into(),
            case_sensitive: "Match case".into(),
            replace_mode: "Show replacement field".into(),
            search_field: "Find".into(),
            replace_field: "Replacement".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_name_every_find_control_without_a_host_locale() {
        let labels = SearchPanelLabels::default();
        assert_eq!(labels.previous.as_ref(), "Previous match");
        assert_eq!(labels.next.as_ref(), "Next match");
        assert_eq!(labels.replace.as_ref(), "Replace");
        assert_eq!(labels.replace_all.as_ref(), "Replace all");
        assert_eq!(labels.close.as_ref(), "Close");
        assert_eq!(labels.case_sensitive.as_ref(), "Match case");
        assert_eq!(labels.replace_mode.as_ref(), "Show replacement field");
        assert_eq!(labels.search_field.as_ref(), "Find");
        assert_eq!(labels.replace_field.as_ref(), "Replacement");
    }
}
