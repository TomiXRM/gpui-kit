use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use gpui::SharedString;
use unicode_segmentation::UnicodeSegmentation as _;

use super::inline::InlineState;

/// Identity is meaningful only in the accepted document revision. Layout fragments
/// carry this descriptor, never a source-Markdown offset or a temporary state ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderedLeaf {
    pub revision: usize,
    pub id: usize,
    pub start: usize,
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct RenderedRun {
    text: SharedString,
    start: usize,
    leaf: Option<RenderedLeaf>,
}

#[derive(Debug, PartialEq)]
struct LeafCaretMap {
    // ASCII graphemes need no storage. Keep only multi-byte/multi-codepoint
    // spans and hard-line starts, derived once from the accepted leaf source.
    graphemes: Vec<Range<usize>>,
    line_starts: Vec<usize>,
}

impl LeafCaretMap {
    fn new(text: &str) -> Self {
        Self {
            graphemes: text
                .grapheme_indices(true)
                .filter_map(|(start, grapheme)| {
                    (grapheme.len() > 1).then_some(start..start + grapheme.len())
                })
                .collect(),
            line_starts: text
                .match_indices('\n')
                .map(|(index, _)| index + 1)
                .collect(),
        }
    }
}

#[derive(Debug, PartialEq)]
struct LeafInfo {
    leaf: RenderedLeaf,
    carets: LeafCaretMap,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct RenderedDocument {
    runs: Vec<RenderedRun>,
    pub revision: usize,
    pub len: usize,
    leaves: Vec<LeafInfo>,
}

impl RenderedDocument {
    pub fn new(revision: usize) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }

    pub fn push(&mut self, text: SharedString, state: Option<&Arc<Mutex<InlineState>>>) {
        let leaf = state.map(|state| {
            let leaf = RenderedLeaf {
                revision: self.revision,
                id: self.leaves.len(),
                start: self.len,
                len: text.len(),
            };
            self.leaves.push(LeafInfo {
                leaf,
                carets: LeafCaretMap::new(&text),
            });
            if let Ok(mut state) = state.lock() {
                state.set_text(text.clone());
                state.leaf = Some(leaf);
            }
            leaf
        });
        if !text.is_empty() {
            self.runs.push(RenderedRun {
                start: self.len,
                text,
                leaf,
            });
            self.len += self.runs.last().unwrap().text.len();
        }
    }

    pub fn leaf(&self, id: usize) -> Option<RenderedLeaf> {
        self.leaves.get(id).map(|info| info.leaf)
    }

    pub fn caret_line_start(&self, leaf: RenderedLeaf, offset: usize) -> Option<usize> {
        let info = self.leaves.get(leaf.id)?;
        if info.leaf != leaf || offset > leaf.len {
            return None;
        }
        let ix = info
            .carets
            .line_starts
            .partition_point(|start| *start <= offset);
        Some(if ix == 0 {
            0
        } else {
            info.carets.line_starts[ix - 1]
        })
    }

    pub fn caret_bounds(&self, leaf: RenderedLeaf, offset: usize) -> Option<(usize, usize)> {
        let info = self.leaves.get(leaf.id)?;
        if info.leaf != leaf || offset > leaf.len {
            return None;
        }
        let ix = info
            .carets
            .graphemes
            .partition_point(|range| range.end <= offset);
        if let Some(range) = info.carets.graphemes.get(ix) {
            if range.start < offset {
                return Some((range.start, range.end));
            }
        }
        Some((offset, offset))
    }

    pub fn is_boundary(&self, offset: usize) -> bool {
        if offset == self.len {
            return true;
        }
        let ix = self
            .runs
            .partition_point(|run| run.start + run.text.len() <= offset);
        self.runs
            .get(ix)
            .is_some_and(|run| offset >= run.start && run.text.is_char_boundary(offset - run.start))
    }

    pub fn range_is_whitespace(&self, range: Range<usize>) -> Option<bool> {
        if range.start > range.end || !self.is_boundary(range.start) || !self.is_boundary(range.end)
        {
            return None;
        }
        let start = self
            .runs
            .partition_point(|run| run.start + run.text.len() <= range.start);
        for run in self.runs[start..]
            .iter()
            .take_while(|run| run.start < range.end)
        {
            let lo = range.start.max(run.start) - run.start;
            let hi = range.end.min(run.start + run.text.len()) - run.start;
            if !run.text[lo..hi].chars().all(char::is_whitespace) {
                return Some(false);
            }
        }
        Some(true)
    }

    pub fn write_range(&self, range: Range<usize>, out: &mut String) -> Option<()> {
        if range.start > range.end || !self.is_boundary(range.start) || !self.is_boundary(range.end)
        {
            return None;
        }
        let first = self
            .runs
            .partition_point(|run| run.start + run.text.len() <= range.start);
        for run in self.runs[first..]
            .iter()
            .take_while(|run| run.start < range.end)
        {
            let start = range.start.max(run.start).saturating_sub(run.start);
            let end = range
                .end
                .min(run.start + run.text.len())
                .saturating_sub(run.start);
            if start < end {
                out.push_str(run.text.get(start..end)?);
            }
        }
        Some(())
    }

    pub fn text(&self) -> String {
        let mut text = String::with_capacity(self.len);
        self.write_range(0..self.len, &mut text).unwrap();
        text
    }
}
