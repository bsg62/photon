//! The photos picked in the grid: which, the one the keyboard moves on from, and the one
//! a range is measured from.
//!
//! What the Svelte store holds about its selection (`library.svelte.ts`), without what it
//! needs only because a webview pages through the grid a thousand rows at a time. This
//! window holds the whole index it draws from, so a range's ids, the photo a key lands on
//! and the place of a photo after a rebuild are each a read of memory in the frame that
//! asks, and nothing here waits for an answer or guards against being overtaken by one.
//! Every call is given the index on screen.
//!
//! **Ids, not offsets.** An offset means a photo against one version of the index only: a
//! scan that indexes a photo into an earlier folder shifts every later one, and a
//! selection kept by offset would ring, and star, the neighbours of what was picked. The
//! lead and the anchor carry both, the offset for the keys and the id to find the photo
//! again by (`follow`); an offset of theirs read after a new grid without `follow` names
//! another photo.

use crate::grid::layout::has_header;
use photon_core::grid::{GridEntry, GridIndex};
use std::collections::HashSet;

/// A photo and where it is in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub offset: usize,
    pub id: i64,
}

/// The lead's photo left with a new grid, and the lead is on another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Landed {
    /// The photo that left.
    pub left: i64,
    /// Where the lead is now, in the new grid.
    pub at: usize,
}

#[derive(Debug, Default)]
pub struct Selection {
    ids: HashSet<i64>,
    /// The photo an arrow key moves on from. Usually selected; not after a Ctrl+click
    /// that took it out, nor where it landed beside photos that stayed selected.
    lead: Option<At>,
    /// The photo a Shift+click ranges from: where the user last clicked plainly or with
    /// Ctrl, or where a range or a select-all began.
    anchor: Option<At>,
    /// The two sets a rubber band holds while it is drawn.
    band: Option<Band>,
}

/// What a rubber band started from.
#[derive(Debug)]
struct Band {
    /// What it adds to: what was selected as it began, for a band that adds, and
    /// nothing for one that replaces. Every frame of the band is this plus what the
    /// rectangle covers, so a rectangle dragged smaller takes photos back off instead
    /// of piling each frame's on the last.
    base: HashSet<i64>,
    /// What was selected as it began, whichever kind it is: what an abandoned band puts
    /// back.
    ///
    /// There is no lead beside it, on purpose. A band does not move the lead while it
    /// is drawn - only its release does - so there is none to put back, and a kept one
    /// would be an offset from before whatever grid arrived under the drag.
    previous: HashSet<i64>,
}

fn ids_of(index: &GridIndex, ranges: &[(usize, usize)]) -> impl Iterator<Item = i64> {
    ranges.iter().flat_map(|&(start, end)| {
        let count = (end + 1).saturating_sub(start);
        index.rows(start, count).iter().map(|entry| entry.id)
    })
}

fn at(index: &GridIndex, offset: usize) -> Option<At> {
    let id = index.rows(offset, 1).first()?.id;
    Some(At { offset, id })
}

impl Selection {
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn is_selected(&self, id: i64) -> bool {
        self.ids.contains(&id)
    }

    /// The selected photos, in the grid's order: what an action is handed.
    pub fn ids(&self, index: &GridIndex) -> Vec<i64> {
        let mut placed: Vec<(usize, i64)> = (self.ids.iter())
            .filter_map(|id| Some((index.position_of(*id)?, *id)))
            .collect();
        placed.sort_unstable();
        placed.into_iter().map(|(_, id)| id).collect()
    }

    pub fn lead(&self) -> Option<At> {
        self.lead
    }

    /// The selected photo whose star says which way the star key goes: starred, the
    /// selection is unstarred; not, it is starred.
    ///
    /// The lead, when it is selected. It is not always: a Ctrl+click that takes a photo
    /// out leaves the lead on it, and a lead can land beside photos that stayed
    /// selected. Read regardless, the key took its direction from a photo with no ring
    /// on it, and a lone starred photo beside such a lead could not be unstarred at all.
    /// Then the selected photo that comes first in the grid stands in. (The Svelte rule
    /// takes the first one on screen, for want of the rest.)
    pub fn deciding(&self, index: &GridIndex) -> Option<i64> {
        match self.lead {
            Some(lead) if self.ids.contains(&lead.id) => Some(lead.id),
            _ => (self.ids.iter())
                .filter_map(|id| Some((index.position_of(*id)?, *id)))
                .min()
                .map(|(_, id)| id),
        }
    }

    pub fn anchor(&self) -> Option<At> {
        self.anchor
    }

    /// A plain click, or a key that moves: that photo alone.
    pub fn select(&mut self, index: &GridIndex, offset: usize) {
        let Some(at) = at(index, offset) else {
            return;
        };
        self.ids.clear();
        self.ids.insert(at.id);
        self.lead = Some(at);
        self.anchor = Some(at);
    }

    /// Ctrl/Cmd+click: puts one photo in or takes it out. The anchor goes to it either
    /// way, so the next Shift+click ranges from where the user last clicked, and so does
    /// the lead - unless that took the last photo out, which leaves nothing to lead.
    pub fn toggle(&mut self, index: &GridIndex, offset: usize) {
        let Some(at) = at(index, offset) else {
            return;
        };
        if !self.ids.remove(&at.id) {
            self.ids.insert(at.id);
        }
        self.lead = (!self.ids.is_empty()).then_some(at);
        self.anchor = Some(at);
    }

    /// Shift+click, and Shift with a key that moves: the selection becomes everything
    /// between the anchor and `offset`, and the lead goes to `offset`.
    ///
    /// The anchor stays, so dragging the far end back and forth ranges from the same
    /// start and does not walk away from it. Where there is none the range starts at the
    /// lead, and with neither at the first photo - and that start is the anchor from then
    /// on, for the same reason.
    pub fn extend(&mut self, index: &GridIndex, offset: usize) {
        let offset = offset.min(index.len().saturating_sub(1));
        let from = (self.anchor.or(self.lead)).map_or(0, |from| from.offset);
        let (start, end) = (from.min(offset), from.max(offset));
        self.ids = (index.rows(start, end - start + 1).iter())
            .map(|entry| entry.id)
            .collect();
        self.lead = at(index, offset);
        if self.anchor.is_none() {
            self.anchor = at(index, from);
        }
    }

    /// Ctrl/Cmd+A: what stands under the lead's header, or outside the library every
    /// photo of the view.
    ///
    /// Every view but All photos is a set the user asked for - an album, a search, the
    /// starred photos - so "all" is that set, and the sections inside it are an
    /// arrangement of it and not a bound. All photos is the whole library, where the
    /// unit being looked at is one section: on fifty thousand photos, selecting every one
    /// is never what the key was pressed for, and starring the result would be a long
    /// operation nobody asked for. That holds where the library has no headers too: it
    /// is then the folder of the lead's photo, whose photos lie anywhere in the grid.
    ///
    /// The lead stays where it is, so that Enter still opens the photo the user was on.
    /// The anchor goes to the start of what was selected, which is where a Shift+click
    /// afterwards ranges from: the selection began there. A folder's scattered photos
    /// have no such start in the grid, and there the anchor is the lead's own photo.
    pub fn select_all(&mut self, index: &GridIndex, library: bool) {
        let here = self.lead.map_or(0, |lead| lead.offset);
        let sections = index.sections();
        // "No header" is asked of the section, not "no folder": a month has no folder
        // and is a section all the same.
        let (ids, start): (HashSet<i64>, usize) =
            if library && sections.first().is_some_and(|first| !has_header(first)) {
                let Some(ids) = index.folder_ids_at(here) else {
                    return;
                };
                (ids.into_iter().collect(), here)
            } else {
                let holding = sections
                    .partition_point(|section| section.offset <= here)
                    .saturating_sub(1);
                let (start, count) = match sections.get(holding) {
                    Some(section) if library => (section.offset, section.count),
                    _ => (0, index.len()),
                };
                let range = index.rows(start, count);
                (range.iter().map(|entry| entry.id).collect(), start)
            };
        self.ids = ids;
        self.anchor = at(index, start);
        if self.lead.is_none() {
            self.lead = self.anchor;
        }
    }

    /// Clears the selection, and with it a band being drawn: what it started from is of
    /// the grid that was cleared.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.lead = None;
        self.anchor = None;
        self.band = None;
    }

    /// A rubber band begins. One that is `adding` - begun with Ctrl, Cmd or Shift held -
    /// builds on what is selected; any other replaces it.
    pub fn band_begin(&mut self, adding: bool) {
        self.band = Some(Band {
            base: if adding {
                self.ids.clone()
            } else {
                HashSet::new()
            },
            previous: self.ids.clone(),
        });
    }

    pub fn banding(&self) -> bool {
        self.band.is_some()
    }

    /// The band covers `ranges` of the grid now: what is selected is what it started
    /// with and those. The lead and the anchor stay where they are until it is let go.
    pub fn band_to(&mut self, index: &GridIndex, ranges: &[(usize, usize)]) {
        let Some(band) = &self.band else {
            return;
        };
        self.ids = band.base.clone();
        self.ids.extend(ids_of(index, ranges));
    }

    /// The band is let go of over `ranges`. The lead and the anchor go to its first
    /// photo, so that Enter opens something inside it and a Shift+click afterwards
    /// ranges from where it began.
    ///
    /// A band over nothing is a band, not a failure: dragging over empty room is how a
    /// selection is cleared with the mouse, and how an adding band that ends up covering
    /// nothing leaves what it started with - with its lead, which the band never moved.
    pub fn band_end(&mut self, index: &GridIndex, ranges: &[(usize, usize)]) {
        self.band_to(index, ranges);
        if self.band.take().is_none() {
            return;
        }
        match ranges.first().and_then(|first| at(index, first.0)) {
            Some(first) => {
                self.lead = Some(first);
                self.anchor = Some(first);
            }
            None if self.ids.is_empty() => {
                self.lead = None;
                self.anchor = None;
            }
            None => {}
        }
    }

    /// The band is abandoned - Escape, or the drag lost: the selection is what it was
    /// when the band began, less what has left the grid since.
    ///
    /// All of it may have left, and the lead with it: `follow` then landed the lead
    /// beside where it was, without selecting it, the band's own photos being selected
    /// at the time. With those gone again the lead would be on a photo with nothing
    /// selected, which nothing else here leaves behind: it is selected, alone, as
    /// `follow` selects a landing when nothing else is.
    pub fn band_cancel(&mut self) {
        let Some(band) = self.band.take() else {
            return;
        };
        self.ids = band.previous;
        if let Some(lead) = self.lead.filter(|_| self.ids.is_empty()) {
            self.ids.insert(lead.id);
            self.anchor = Some(lead);
        }
    }

    /// A new grid is put on screen in place of `old`, showing the same view: the
    /// selection goes with its photos.
    ///
    /// - A photo `new` does not hold is no longer selected. "N selected" is then exact,
    ///   and no action is handed a photo that no tile shows.
    /// - The lead and the anchor are found again by id.
    /// - A lead whose photo has left lands on the nearest photo that stays - the first
    ///   after it in the old grid that the new one holds, else the last before it - as a
    ///   file manager does after a delete, and that is answered. If nothing is left
    ///   selected the photo landed on is selected alone and is the anchor: so a key that
    ///   takes photos out of the view (a hide, a star taken off in Starred) can be
    ///   pressed again and again from where the user was.
    /// - An anchor whose photo has left is no anchor, and the next range starts at the
    ///   lead.
    /// - A band being drawn goes on: its rectangle covers what it covers in the new grid.
    ///
    /// It does not ask what rebuilt the grid, so it has no account to keep of a click made
    /// while a write was out: the photos clicked are still there, and stay selected.
    pub fn follow(&mut self, old: &GridIndex, new: &GridIndex) -> Option<Landed> {
        let found = |id: i64| new.position_of(id).map(|offset| At { offset, id });
        self.ids.retain(|id| new.position_of(*id).is_some());
        // And of what a band being drawn started from: neither its next frame nor its
        // abandoning brings back a photo the grid no longer holds.
        if let Some(band) = &mut self.band {
            band.base.retain(|id| new.position_of(*id).is_some());
            band.previous.retain(|id| new.position_of(*id).is_some());
        }
        self.anchor = self.anchor.and_then(|anchor| found(anchor.id));
        let lead = self.lead?;
        if let Some(lead) = found(lead.id) {
            self.lead = Some(lead);
            return None;
        }
        let stays = |entry: &GridEntry| found(entry.id);
        let was = lead.offset;
        let after = old.rows(was + 1, usize::MAX).iter().find_map(stays);
        let landing = after.or_else(|| old.rows(0, was).iter().rev().find_map(stays));
        self.lead = landing;
        let landing = landing?;
        if self.ids.is_empty() {
            self.ids.insert(landing.id);
            self.anchor = Some(landing);
        }
        Some(Landed {
            left: lead.id,
            at: landing.offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::{
        grid::{Layout, PeriodUnit},
        media::MediaKind,
    };

    fn entry(id: i64, folder_id: i64, taken_at: i64) -> GridEntry {
        GridEntry {
            id,
            folder_id,
            taken_at,
            aspect: 1.5,
            kind: MediaKind::Image,
            duration_ms: None,
            starred: false,
            has_copies: false,
            thumb_key: id as u64,
            size: 0,
            mtime_ms: 0,
        }
    }

    /// A grid laid out by folder: each slice is a folder's photos, by id.
    fn folders(folders: &[&[i64]]) -> GridIndex {
        let entries = (folders.iter().enumerate())
            .flat_map(|(folder, ids)| ids.iter().map(move |id| entry(*id, folder as i64 + 1, 0)))
            .collect();
        GridIndex::build(entries, Layout::Folders)
    }

    /// A grid with no headers: each photo is `(id, folder)`, in the order given.
    fn flat(photos: &[(i64, i64)]) -> GridIndex {
        let entries = (photos.iter())
            .map(|(id, folder)| entry(*id, *folder, 0))
            .collect();
        GridIndex::build(entries, Layout::Flat)
    }

    /// Ten photos, ids 10 to 19, in one folder.
    fn ten() -> GridIndex {
        folders(&[&[10, 11, 12, 13, 14, 15, 16, 17, 18, 19]])
    }

    fn selected(selection: &Selection, index: &GridIndex) -> Vec<i64> {
        selection.ids(index)
    }

    fn lead(selection: &Selection) -> Option<(usize, i64)> {
        selection.lead().map(|at| (at.offset, at.id))
    }

    fn anchor(selection: &Selection) -> Option<(usize, i64)> {
        selection.anchor().map(|at| (at.offset, at.id))
    }

    // ---- picking ----

    #[test]
    fn a_plain_selection_replaces_the_whole_set() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 2);
        s.toggle(&index, 5);
        assert_eq!(selected(&s, &index), [12, 15]);
        s.select(&index, 7);
        assert_eq!(selected(&s, &index), [17]);
        assert_eq!(lead(&s), Some((7, 17)));
        assert_eq!(anchor(&s), Some((7, 17)));
        assert_eq!(s.len(), 1);
        assert!(s.is_selected(17) && !s.is_selected(12));
    }

    #[test]
    fn a_selection_past_the_end_of_the_grid_selects_nothing() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 3);
        s.select(&index, 10);
        s.toggle(&index, 99);
        assert_eq!(selected(&s, &index), [13]);
        assert_eq!(lead(&s), Some((3, 13)));
    }

    #[test]
    fn ctrl_click_toggles_a_photo_in_and_out_moving_the_lead_each_time() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 1);
        s.toggle(&index, 4);
        assert_eq!(selected(&s, &index), [11, 14]);
        assert_eq!(lead(&s), Some((4, 14)));
        assert_eq!(anchor(&s), Some((4, 14)));
        // Out again: the lead and the anchor go to the photo clicked all the same.
        s.toggle(&index, 1);
        assert_eq!(selected(&s, &index), [14]);
        assert_eq!(lead(&s), Some((1, 11)));
        assert_eq!(anchor(&s), Some((1, 11)));
    }

    #[test]
    fn taking_the_last_photo_out_leaves_no_lead() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 4);
        s.toggle(&index, 4);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
        // The next Shift+click still ranges from where the user last clicked.
        assert_eq!(anchor(&s), Some((4, 14)));
    }

    // ---- ranges ----

    #[test]
    fn shift_click_selects_from_the_anchor_and_replaces_what_was_selected() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 8);
        s.toggle(&index, 2);
        s.extend(&index, 5);
        assert_eq!(selected(&s, &index), [12, 13, 14, 15]);
        assert_eq!(lead(&s), Some((5, 15)));
        assert_eq!(anchor(&s), Some((2, 12)));
    }

    #[test]
    fn shift_click_twice_ranges_again_from_the_same_anchor() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 4);
        s.extend(&index, 7);
        s.extend(&index, 5);
        assert_eq!(selected(&s, &index), [14, 15]);
        // And backwards, across the anchor.
        s.extend(&index, 2);
        assert_eq!(selected(&s, &index), [12, 13, 14]);
        assert_eq!(lead(&s), Some((2, 12)));
        assert_eq!(anchor(&s), Some((4, 14)));
    }

    #[test]
    fn a_range_with_no_anchor_starts_at_the_lead_and_keeps_that_start() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 3);
        s.toggle(&old, 6);
        // The anchor's photo (16) leaves; the lead's is 16 too, and lands on 17.
        let new = folders(&[&[10, 11, 12, 13, 14, 15, 17, 18, 19]]);
        s.follow(&old, &new);
        assert_eq!(anchor(&s), None);
        assert_eq!(lead(&s), Some((6, 17)));
        s.extend(&new, 8);
        assert_eq!(selected(&s, &new), [17, 18, 19]);
        // Dragging the far end back ranges from the same start, not from the lead.
        s.extend(&new, 7);
        assert_eq!(selected(&s, &new), [17, 18]);
        assert_eq!(anchor(&s), Some((6, 17)));
    }

    #[test]
    fn a_range_with_nothing_selected_starts_at_the_first_photo() {
        let index = ten();
        let mut s = Selection::default();
        s.extend(&index, 2);
        assert_eq!(selected(&s, &index), [10, 11, 12]);
        assert_eq!(lead(&s), Some((2, 12)));
        assert_eq!(anchor(&s), Some((0, 10)));
    }

    #[test]
    fn a_range_to_past_the_end_is_held_to_the_last_photo() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 7);
        s.extend(&index, 40);
        assert_eq!(selected(&s, &index), [17, 18, 19]);
        assert_eq!(lead(&s), Some((9, 19)));
        // And nothing at all in a grid with nothing in it.
        let empty = folders(&[]);
        let mut s = Selection::default();
        s.extend(&empty, 3);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
    }

    // ---- select all ----

    /// Three folders of three, four and two photos.
    fn library() -> GridIndex {
        folders(&[&[1, 2, 3], &[4, 5, 6, 7], &[8, 9]])
    }

    #[test]
    fn select_all_takes_the_folder_the_lead_is_in_not_the_whole_library() {
        let index = library();
        let mut s = Selection::default();
        s.select(&index, 5);
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [4, 5, 6, 7]);
        // The lead stays on the photo the user was on.
        assert_eq!(lead(&s), Some((5, 6)));
        // The last folder ends with the grid.
        s.select(&index, 8);
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [8, 9]);
        // A folder's first photo is in that folder, not in the one before it.
        s.select(&index, 3);
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [4, 5, 6, 7]);
    }

    #[test]
    fn select_all_leaves_the_anchor_at_the_start_of_what_it_selected() {
        let index = library();
        let mut s = Selection::default();
        s.select(&index, 5);
        s.select_all(&index, true);
        assert_eq!(anchor(&s), Some((3, 4)));
        // So a Shift+click afterwards ranges from the folder's first photo.
        s.extend(&index, 8);
        assert_eq!(selected(&s, &index), [4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn select_all_takes_the_first_folder_when_nothing_is_selected_yet() {
        let index = library();
        let mut s = Selection::default();
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [1, 2, 3]);
        assert_eq!(lead(&s), Some((0, 1)));
        assert_eq!(anchor(&s), Some((0, 1)));
    }

    #[test]
    fn select_all_takes_the_whole_view_outside_the_library() {
        let index = library();
        let mut s = Selection::default();
        s.select(&index, 5);
        s.select_all(&index, false);
        assert_eq!(selected(&s, &index), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(lead(&s), Some((5, 6)));
        assert_eq!(anchor(&s), Some((0, 1)));
    }

    // Under a flat sort the folder is no longer a run of the grid: its photos lie
    // wherever the order put them, and are a set of ids and not a range.
    #[test]
    fn select_all_takes_the_lead_photos_folder_under_a_flat_sort_wherever_its_photos_are() {
        let index = flat(&[(1, 7), (2, 8), (3, 7), (4, 9), (5, 7), (6, 8)]);
        let mut s = Selection::default();
        s.select(&index, 2);
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [1, 3, 5]);
        assert_eq!(lead(&s), Some((2, 3)));
        assert_eq!(anchor(&s), Some((2, 3)));
        // Outside the library a flat view is taken whole, like any other.
        s.select_all(&index, false);
        assert_eq!(s.len(), 6);
        // With nothing selected it is the first photo's folder.
        let mut s = Selection::default();
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [1, 3, 5]);
        assert_eq!(lead(&s), Some((0, 1)));
    }

    // A month's section has no folder and has a header all the same: asked "has it a
    // folder", select all took a folder's scattered photos from under a month's header.
    #[test]
    fn select_all_takes_the_lead_photos_month_under_a_date_grouping() {
        let month = |month: i8, day: i8| {
            jiff::civil::date(2024, month, day)
                .at(12, 0, 0, 0)
                .to_zoned(jiff::tz::TimeZone::UTC)
                .unwrap()
                .timestamp()
                .as_second()
        };
        // Newest first, as a date grouping lays the grid out; two folders in each month.
        let entries = vec![
            entry(1, 7, month(6, 20)),
            entry(2, 8, month(6, 10)),
            entry(3, 7, month(5, 20)),
            entry(4, 8, month(5, 10)),
            entry(5, 7, month(5, 5)),
        ];
        let index = GridIndex::build(entries, Layout::Periods(PeriodUnit::Month));
        assert_eq!(index.sections().len(), 2);
        let mut s = Selection::default();
        s.select(&index, 3);
        s.select_all(&index, true);
        assert_eq!(selected(&s, &index), [3, 4, 5]);
        assert_eq!(anchor(&s), Some((2, 3)));
    }

    #[test]
    fn select_all_in_an_empty_grid_selects_nothing() {
        let index = folders(&[]);
        let mut s = Selection::default();
        s.select_all(&index, true);
        s.select_all(&index, false);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
    }

    #[test]
    fn escape_clears_the_selection_the_lead_and_the_anchor() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 2);
        s.extend(&index, 5);
        s.clear();
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
        assert_eq!(anchor(&s), None);
    }

    #[test]
    fn the_selected_photos_are_handed_out_in_the_grids_order() {
        let index = ten();
        let mut s = Selection::default();
        // Every photo, clicked in no order: a set kept by hash hands them out in none.
        for offset in [9, 0, 4, 7, 2, 5, 1, 8, 3, 6] {
            s.toggle(&index, offset);
        }
        assert_eq!(s.ids(&index), [10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
    }

    // ---- a new grid ----

    // A scan that indexes photos into an earlier folder shifts every later offset.
    #[test]
    fn the_selection_stays_on_its_photos_when_the_grid_is_renumbered() {
        let old = folders(&[&[10, 11, 12], &[20, 21, 22, 23]]);
        let mut s = Selection::default();
        s.select(&old, 4);
        s.extend(&old, 6);
        let new = folders(&[&[10, 11, 12, 13, 14], &[20, 21, 22, 23]]);
        assert_eq!(s.follow(&old, &new), None);
        assert_eq!(selected(&s, &new), [21, 22, 23]);
        assert_eq!(lead(&s), Some((8, 23)));
        // The anchor of a range stays the range's start, on its own photo: carried with
        // the lead it would start the next range from a photo nobody clicked.
        assert_eq!(anchor(&s), Some((6, 21)));
        s.extend(&new, 7);
        assert_eq!(selected(&s, &new), [21, 22]);
    }

    #[test]
    fn a_selected_photo_that_left_is_no_longer_selected() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 2);
        s.extend(&old, 5);
        let new = folders(&[&[10, 11, 12, 14, 15, 16, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), None);
        assert_eq!(selected(&s, &new), [12, 14, 15]);
        assert_eq!(s.len(), 3);
        assert_eq!(lead(&s), Some((4, 15)));
    }

    #[test]
    fn a_lead_whose_photo_left_lands_on_the_next_photo_that_stays() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 4);
        let new = folders(&[&[10, 11, 12, 13, 15, 16, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), Some(Landed { left: 14, at: 4 }));
        // Nothing else was selected: the photo landed on is, alone, and is the anchor.
        assert_eq!(selected(&s, &new), [15]);
        assert_eq!(lead(&s), Some((4, 15)));
        assert_eq!(anchor(&s), Some((4, 15)));
    }

    // In Duplicates, hiding one of a pair takes its partner out of the view too, and the
    // partner is the photo after it.
    #[test]
    fn a_landing_passes_over_every_photo_that_left_with_the_lead() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 4);
        let new = folders(&[&[10, 11, 12, 13, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), Some(Landed { left: 14, at: 4 }));
        assert_eq!(lead(&s), Some((4, 17)));
        assert_eq!(selected(&s, &new), [17]);
    }

    #[test]
    fn a_lead_with_nothing_after_it_lands_on_the_photo_before() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 7);
        s.extend(&old, 9);
        // The selection ran to the end of the grid, and all of it left.
        let new = folders(&[&[10, 11, 12, 13, 14, 15, 16]]);
        assert_eq!(s.follow(&old, &new), Some(Landed { left: 19, at: 6 }));
        assert_eq!(selected(&s, &new), [16]);
        assert_eq!(lead(&s), Some((6, 16)));
    }

    #[test]
    fn a_whole_selection_that_left_lands_after_it_not_at_the_leads_old_offset() {
        let old = ten();
        let mut s = Selection::default();
        // The lead is in the middle of the run: anchor 2, range to 6, lead back to 4.
        s.select(&old, 2);
        s.extend(&old, 6);
        s.toggle(&old, 4);
        s.toggle(&old, 4);
        assert_eq!(lead(&s), Some((4, 14)));
        let new = folders(&[&[10, 11, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), Some(Landed { left: 14, at: 2 }));
        assert_eq!(selected(&s, &new), [17]);
    }

    #[test]
    fn a_lead_lands_nowhere_in_a_grid_left_empty() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 4);
        let new = folders(&[]);
        assert_eq!(s.follow(&old, &new), None);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
        assert_eq!(anchor(&s), None);
    }

    #[test]
    fn a_landing_is_not_selected_beside_photos_that_stayed_selected() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 2);
        s.toggle(&old, 3);
        s.toggle(&old, 6);
        // The lead's photo (16) is purged; the two others stay selected.
        let new = folders(&[&[10, 11, 12, 13, 14, 15, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), Some(Landed { left: 16, at: 6 }));
        assert_eq!(selected(&s, &new), [12, 13]);
        assert_eq!(lead(&s), Some((6, 17)));
        assert!(!s.is_selected(17));
    }

    #[test]
    fn an_anchor_whose_photo_left_is_no_anchor() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 2);
        s.extend(&old, 5);
        // The anchor's photo (12) leaves; the lead's stays.
        let new = folders(&[&[10, 11, 13, 14, 15, 16, 17, 18, 19]]);
        assert_eq!(s.follow(&old, &new), None);
        assert_eq!(anchor(&s), None);
        assert_eq!(lead(&s), Some((4, 15)));
        assert_eq!(selected(&s, &new), [13, 14, 15]);
    }

    #[test]
    fn nothing_selected_follows_a_new_grid_as_nothing() {
        let old = ten();
        let new = folders(&[&[10, 11]]);
        let mut s = Selection::default();
        assert_eq!(s.follow(&old, &new), None);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
    }

    // ---- a rubber band ----

    #[test]
    fn a_band_replaces_the_selection_with_what_it_covers() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 8);
        assert!(!s.banding());
        s.band_begin(false);
        assert!(s.banding());
        s.band_to(&index, &[(1, 2), (5, 6)]);
        assert_eq!(selected(&s, &index), [11, 12, 15, 16]);
        // Dragged smaller, photos come off: each frame is what the band covers now.
        s.band_to(&index, &[(1, 1)]);
        assert_eq!(selected(&s, &index), [11]);
        s.band_end(&index, &[(1, 2)]);
        assert!(!s.banding());
        assert_eq!(selected(&s, &index), [11, 12]);
    }

    #[test]
    fn an_adding_band_adds_to_what_it_started_with_and_shrinks_back_again() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 8);
        s.toggle(&index, 9);
        s.band_begin(true);
        s.band_to(&index, &[(1, 3)]);
        assert_eq!(selected(&s, &index), [11, 12, 13, 18, 19]);
        s.band_to(&index, &[(1, 1)]);
        assert_eq!(selected(&s, &index), [11, 18, 19]);
        // Over a photo that was selected already, it stays selected.
        s.band_to(&index, &[(7, 8)]);
        assert_eq!(selected(&s, &index), [17, 18, 19]);
        s.band_end(&index, &[(1, 1)]);
        assert_eq!(selected(&s, &index), [11, 18, 19]);
    }

    #[test]
    fn a_band_does_not_move_the_lead_until_it_is_let_go_of() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 8);
        s.band_begin(false);
        s.band_to(&index, &[(1, 3)]);
        assert_eq!(lead(&s), Some((8, 18)));
        assert_eq!(anchor(&s), Some((8, 18)));
        // Let go of: on the band's first photo, both.
        s.band_end(&index, &[(2, 3), (5, 5)]);
        assert_eq!(lead(&s), Some((2, 12)));
        assert_eq!(anchor(&s), Some((2, 12)));
        // So a Shift+click afterwards ranges from where the band began.
        s.extend(&index, 4);
        assert_eq!(selected(&s, &index), [12, 13, 14]);
    }

    // Dragging over empty room is how a selection is cleared with the mouse.
    #[test]
    fn a_band_over_nothing_clears_the_selection() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 8);
        s.band_begin(false);
        s.band_to(&index, &[]);
        assert!(s.is_empty());
        s.band_end(&index, &[]);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
        assert_eq!(anchor(&s), None);
        assert!(!s.banding());
    }

    #[test]
    fn an_adding_band_over_nothing_leaves_what_it_started_with() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 7);
        s.toggle(&index, 8);
        s.band_begin(true);
        s.band_to(&index, &[(1, 2)]);
        s.band_end(&index, &[]);
        assert_eq!(selected(&s, &index), [17, 18]);
        // With its lead, which the band never moved.
        assert_eq!(lead(&s), Some((8, 18)));
        assert_eq!(anchor(&s), Some((8, 18)));
    }

    #[test]
    fn an_abandoned_band_puts_the_selection_back_and_leaves_the_lead_alone() {
        let index = ten();
        for adding in [false, true] {
            let mut s = Selection::default();
            s.select(&index, 7);
            s.toggle(&index, 8);
            s.band_begin(adding);
            s.band_to(&index, &[(1, 3)]);
            s.band_cancel();
            assert!(!s.banding());
            assert_eq!(selected(&s, &index), [17, 18], "{adding}");
            assert_eq!(lead(&s), Some((8, 18)), "{adding}");
            assert_eq!(anchor(&s), Some((8, 18)), "{adding}");
        }
    }

    #[test]
    fn a_range_past_the_end_of_the_grid_is_held_to_it() {
        let index = ten();
        let mut s = Selection::default();
        s.band_begin(false);
        s.band_to(&index, &[(8, 40)]);
        assert_eq!(selected(&s, &index), [18, 19]);
        s.band_end(&index, &[(30, 40)]);
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
    }

    #[test]
    fn nothing_is_a_band_before_one_begins_or_after_it_ends() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 4);
        s.band_to(&index, &[(1, 2)]);
        s.band_end(&index, &[(1, 2)]);
        s.band_cancel();
        assert_eq!(selected(&s, &index), [14]);
        assert_eq!(lead(&s), Some((4, 14)));
    }

    // A scan's grid arrives under the drag: the rectangle covers what it covers there,
    // and a photo that left comes back with neither the next frame nor an Escape.
    #[test]
    fn a_photo_that_left_under_a_band_is_in_neither_of_its_sets() {
        let old = ten();
        let new = folders(&[&[10, 11, 12, 13, 14, 15, 16, 19]]);
        for end in ["frame", "cancel"] {
            let mut s = Selection::default();
            s.select(&old, 7);
            s.toggle(&old, 8);
            s.toggle(&old, 9);
            s.band_begin(true);
            s.band_to(&old, &[(1, 2)]);
            s.follow(&old, &new);
            assert!(s.banding());
            if end == "frame" {
                s.band_to(&new, &[(1, 1)]);
                assert_eq!(selected(&s, &new), [11, 19]);
            } else {
                s.band_cancel();
                assert_eq!(selected(&s, &new), [19]);
            }
            assert_eq!(s.len(), selected(&s, &new).len(), "{end}");
        }
    }

    // A step's grid: the photos are another set, and the band began in the one before.
    #[test]
    fn clearing_the_selection_forgets_a_band_being_drawn() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 7);
        s.band_begin(true);
        s.band_to(&index, &[(1, 2)]);
        s.clear();
        assert!(!s.banding());
        // Nothing of it comes back with a frame or a release that arrives late.
        s.band_to(&index, &[(1, 2)]);
        s.band_end(&index, &[(1, 2)]);
        s.band_cancel();
        assert!(s.is_empty());
        assert_eq!(lead(&s), None);
    }

    // The lead's photo leaves under a band, and so does everything else that was
    // selected before it: `follow` lands the lead without selecting it, the band's own
    // photos being selected then. Abandoned, the band puts back a selection that is
    // gone, and the lead would be on a photo with nothing selected.
    #[test]
    fn a_lead_that_landed_under_a_band_is_selected_when_the_band_is_abandoned() {
        let old = ten();
        let mut s = Selection::default();
        s.select(&old, 8);
        s.band_begin(false);
        s.band_to(&old, &[(1, 2)]);
        let new = folders(&[&[10, 11, 12, 13, 14, 15, 16, 17, 19]]);
        s.follow(&old, &new);
        assert_eq!(lead(&s), Some((8, 19)));
        s.band_cancel();
        assert_eq!(selected(&s, &new), [19]);
        assert_eq!(anchor(&s), Some((8, 19)));
        // With something of the old selection left, that is what is put back.
        let mut s = Selection::default();
        s.select(&old, 7);
        s.toggle(&old, 8);
        s.band_begin(false);
        s.band_to(&old, &[(1, 2)]);
        s.follow(&old, &new);
        s.band_cancel();
        assert_eq!(selected(&s, &new), [17]);
    }

    // ---- the photo the star key goes by ----

    #[test]
    fn the_star_key_goes_by_the_lead_when_it_is_selected() {
        let index = ten();
        let mut s = Selection::default();
        assert_eq!(s.deciding(&index), None);
        s.select(&index, 2);
        s.extend(&index, 6);
        assert_eq!(s.deciding(&index), Some(16));
    }

    // A Ctrl+click that takes a photo out leaves the lead on it. Read regardless, the
    // key took its direction from a photo with no ring on it.
    #[test]
    fn a_lead_that_is_not_selected_gives_way_to_the_first_selected_photo_in_the_grid() {
        let index = ten();
        let mut s = Selection::default();
        s.select(&index, 7);
        s.toggle(&index, 3);
        s.toggle(&index, 5);
        s.toggle(&index, 5);
        assert_eq!(lead(&s), Some((5, 15)));
        assert_eq!(s.deciding(&index), Some(13));
    }
}
