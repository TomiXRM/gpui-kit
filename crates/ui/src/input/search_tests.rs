use super::*;

#[test]
fn test_search() {
    let mut matcher = SearchMatcher::new();
    matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
    matcher.update_query("Is", true);

    assert_eq!(matcher.len(), 3);
    let mut matches = matcher.clone();
    assert_eq!(matches.current_match_ix, 0);
    assert_eq!(matches.next(), Some(18..20));
    assert_eq!(matches.next(), Some(23..25));
    assert_eq!(matches.current_match_ix, 2);
    assert_eq!(matches.next(), Some(15..17));
    assert_eq!(matches.current_match_ix, 0);
    assert_eq!(matches.next_back(), Some(23..25));
    assert_eq!(matches.current_match_ix, 2);
    assert_eq!(matches.next_back(), Some(18..20));
    assert_eq!(matches.current_match_ix, 1);
    assert_eq!(matches.next_back(), Some(15..17));
    assert_eq!(matches.current_match_ix, 0);
    assert_eq!(matches.next_back(), Some(23..25));

    matcher.update_query("IS", false);
    assert_eq!(matcher.len(), 0);
    assert_eq!(matcher.next(), None);
    assert_eq!(matcher.next_back(), None);
}

#[test]
fn test_search_label() {
    let mut matcher = SearchMatcher::new();
    matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
    matcher.update_query("Is", true);
    assert_eq!(matcher.label(), "1/3");
    matcher.next();
    assert_eq!(matcher.label(), "2/3");
    matcher.next();
    assert_eq!(matcher.label(), "3/3");
    matcher.next();
    assert_eq!(matcher.label(), "1/3");

    matcher.update_query("IS", false);
    assert_eq!(matcher.label(), "0/0");
}

#[test]
fn test_select_range_start() {
    let mut matcher = SearchMatcher::new();
    matcher.matched_ranges = Rc::new(vec![5..10, 15..20, 25..30]);
    matcher.update_cursor_by_offset(0);
    assert_eq!(matcher.current_match_ix, 0);

    matcher.update_cursor_by_offset(5);
    assert_eq!(matcher.current_match_ix, 0);

    matcher.update_cursor_by_offset(12);
    assert_eq!(matcher.current_match_ix, 1);

    matcher.update_cursor_by_offset(16);
    assert_eq!(matcher.current_match_ix, 1);

    matcher.update_cursor_by_offset(30);
    assert_eq!(matcher.current_match_ix, 2);

    matcher.update_cursor_by_offset(31);
    assert_eq!(matcher.current_match_ix, 2);
}

#[test]
fn test_next_scroll_direction_returns_down_without_wrap() {
    assert!(matches!(
        SearchPanel::next_scroll_direction(0, 1),
        Some(MoveDirection::Down)
    ));
}

#[test]
fn test_next_scroll_direction_returns_none_on_wrap() {
    assert!(SearchPanel::next_scroll_direction(2, 0).is_none());
}

#[test]
fn test_next_scroll_direction_returns_none_for_single_match() {
    assert!(SearchPanel::next_scroll_direction(0, 0).is_none());
}

#[test]
fn test_next_ix_wraps_to_start() {
    let mut matcher = SearchMatcher::new();
    matcher.matched_ranges = Rc::new(vec![5..10, 15..20, 25..30]);
    matcher.current_match_ix = 2;

    assert_eq!(matcher.next_ix(), Some(0));
}

#[test]
fn test_prev_scroll_direction_returns_up_without_wrap() {
    assert!(matches!(
        SearchPanel::prev_scroll_direction(2, 1),
        Some(MoveDirection::Up)
    ));
}

#[test]
fn test_prev_scroll_direction_returns_none_on_wrap() {
    assert!(SearchPanel::prev_scroll_direction(0, 2).is_none());
}

#[test]
fn test_prev_scroll_direction_returns_none_for_single_match() {
    assert!(SearchPanel::prev_scroll_direction(0, 0).is_none());
}

#[test]
fn test_update_matches_clamps_current_match_index_while_replacing() {
    let mut matcher = SearchMatcher::new();
    matcher.update(&Rope::from("foo foo foo"));
    matcher.update_query("foo", true);
    matcher.current_match_ix = 2;
    matcher.replacing = true;

    matcher.update(&Rope::from("foo xoo foo"));

    assert_eq!(matcher.len(), 2);
    assert_eq!(matcher.current_match_ix, 1);
    assert_eq!(matcher.label(), "2/2");
    assert!(!matcher.replacing);
}
