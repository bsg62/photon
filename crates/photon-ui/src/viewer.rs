//! What the viewer has open: which photo, where it is in the grid on screen, and whether
//! the view still holds it. The Svelte viewer's `current`, `orphaned` and `gone`, and
//! `picture.ts`.
//!
//! Most of what the Svelte viewer does around these has no successor here. It holds an
//! offset into a grid it knows a page of, and learns where its photo went by asking; this
//! window holds the whole `GridIndex`, so a photo is found again in the frame the new
//! grid arrives in (`follow`), and a picture is asked for by the photo and its key, never
//! by an offset. `createLoadSlot`, `rebind`, `renumbered` and `detailsSeq` - each there to
//! tell a renumbering from a step across a round trip - are not ported.

use crate::viewer_zoom::Zoom;
use photon_core::{grid::GridIndex, media::MediaKind};

/// How long the viewer must rest on a photo reached by hand before its full picture is
/// asked for, in milliseconds. The preview is on screen at once and covers the gap. Long
/// enough that a held arrow key, which repeats every 30 to 50 ms, asks for none of the
/// photos it passes - a decode cannot be taken back once it has begun, and the photo
/// stopped on would wait behind every one passed; short enough to be lost in the swap
/// from preview to full when the user stops.
pub const REST_MS: f64 = 130.0;

/// Folds one turn of the wheel into what it has turned so far, in notches: a step to the
/// next photo (1) or the one before (-1) once that is a whole notch, and what is carried
/// on. A mouse's notch is one photo; a trackpad's stream of small turns adds up to one,
/// so that it does not fly through a folder. `wheelStep`, with a notch for its hundred.
pub fn wheel_step(turned: f32, by: f32) -> (f32, i64) {
    // A turn the other way begins again: whatever the first way had banked would make
    // the first small turn back a step at once.
    let base = if turned * by < 0.0 { 0.0 } else { turned };
    let next = base + by;
    if next >= 1.0 {
        (0.0, 1)
    } else if next <= -1.0 {
        (0.0, -1)
    } else {
        (next, 0)
    }
}

/// What of a photo decides the picture on screen. The faces are not in it on purpose: a
/// detection landing must not make the picture again under the user's eyes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The thumbnail key: the file and the edit.
    pub key: u64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    /// Whether its thumbnail could not be made.
    pub failed: bool,
    pub video: bool,
}

/// Whether a re-read of the photo on screen shows another *picture*, and not other facts
/// about the same one. Another picture is made again, fitted, with the crop tool closed -
/// so this has to be exact.
///
/// The key covers the file and the edit; the size and the orientation cover a re-read of
/// the same bytes by a newer reader. The thumbnail's state counts only across `failed` -
/// a photo that could not be shown and now can, or the reverse: every edit sends the row
/// back to pending, and counted plainly the next change of anything - a star, a scan's
/// end - made the picture again. For a video it does not count at all: it is the poster
/// frame's, not the video's.
pub fn picture_changed(old: &Shown, fresh: &Shown) -> bool {
    old.key != fresh.key
        || old.width != fresh.width
        || old.height != fresh.height
        || old.orientation != fresh.orientation
        || (!old.video && old.failed != fresh.failed)
}

/// Whether the view holds the photo on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// It is in the grid, at the viewer's offset.
    Here,
    /// The grid has lost it and the engine has not said yet whether it still exists.
    Asked,
    /// It has left the view and is still a photo: unstarred in Starred, hidden. It stays
    /// on screen - the user is looking at it - with no place in its caption.
    Orphaned,
    /// It has left the library.
    Gone,
}

/// The picture the viewer wants made. Two that are equal are one picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Want {
    pub id: i64,
    pub key: u64,
    /// Counts the times the same key has had to be made again: a size or an orientation
    /// read anew, a photo that could not be shown and now can.
    pub again: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewer {
    id: i64,
    /// Where the photo is in the grid on screen. For a photo a newer grid of the same
    /// results no longer holds, where it was: the photo there now is the one that came
    /// after it. (In other results the number names nothing, and the application closes
    /// a viewer whose photo they do not hold.)
    offset: usize,
    key: u64,
    kind: MediaKind,
    shown: Option<Shown>,
    held: Held,
    again: u32,
    /// When the viewer came to this photo by a step, on the frame's clock; none for the
    /// photo it opened on and for one made again in place.
    stepped_at: Option<f64>,
    /// How the photo is zoomed and panned. The fit again on another photo, and on
    /// another picture of this one: what was looked at closely is not there any more.
    zoom: Zoom,
}

impl Viewer {
    /// The viewer opened on the photo `id` of `index`; none if the grid does not hold it.
    pub fn open(id: i64, index: &GridIndex) -> Option<Self> {
        let offset = index.position_of(id)?;
        let entry = index.rows(offset, 1).first()?;
        Some(Self {
            id,
            offset,
            key: entry.thumb_key,
            kind: entry.kind,
            shown: None,
            held: Held::Here,
            again: 0,
            stepped_at: None,
            zoom: Zoom::default(),
        })
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The key of the picture to show: the grid's, until the library has said more.
    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn is_video(&self) -> bool {
        self.kind == MediaKind::Video
    }

    /// What the library last said decides the picture; none until it has answered for
    /// this photo.
    pub fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// The photo has left the view and is still a photo.
    pub fn orphaned(&self) -> bool {
        self.held == Held::Orphaned
    }

    /// The photo has left the library.
    pub fn gone(&self) -> bool {
        self.held == Held::Gone
    }

    /// How the photo is zoomed and panned.
    pub fn zoom(&self) -> &Zoom {
        &self.zoom
    }

    /// The same, for the view to change.
    pub fn zoom_mut(&mut self) -> &mut Zoom {
        &mut self.zoom
    }

    /// The picture to make.
    pub fn want(&self) -> Want {
        Want {
            id: self.id,
            key: self.key,
            again: self.again,
        }
    }

    /// Whether there is a photo to step to that way: what the buttons for the one before
    /// and the one after answer to. A photo the view has lost holds no place of its own -
    /// the photo at its offset is the one that came after it - so its next is that one.
    pub fn can_step(&self, dir: i64, len: usize) -> bool {
        if dir < 0 {
            self.offset > 0 && len > 0
        } else if self.held == Held::Here {
            self.offset + 1 < len
        } else {
            self.offset < len
        }
    }

    /// One photo on (`dir` 1) or back (-1), at `now` on the frame's clock. Answers whether
    /// the viewer is on another photo for it.
    pub fn step(&mut self, dir: i64, index: &GridIndex, now: f64) -> bool {
        if !self.can_step(dir, index.len()) {
            return false;
        }
        let target = match (dir < 0, self.held == Held::Here) {
            (true, _) => self.offset - 1,
            (false, true) => self.offset + 1,
            // Whatever stands at a lost photo's offset is its right-hand neighbour.
            (false, false) => self.offset,
        };
        self.land(target.min(index.len() - 1), index, now)
    }

    /// To the photo at `offset`: the first, the last. Answers whether that is another.
    pub fn go_to(&mut self, offset: usize, index: &GridIndex, now: f64) -> bool {
        if index.is_empty() {
            return false;
        }
        let target = offset.min(index.len() - 1);
        if self.held == Held::Here && target == self.offset {
            return false;
        }
        self.land(target, index, now)
    }

    fn land(&mut self, target: usize, index: &GridIndex, now: f64) -> bool {
        let Some(entry) = index.rows(target, 1).first() else {
            return false;
        };
        *self = Self {
            id: entry.id,
            offset: target,
            key: entry.thumb_key,
            kind: entry.kind,
            shown: None,
            held: Held::Here,
            again: 0,
            stepped_at: Some(now),
            zoom: Zoom::default(),
        };
        true
    }

    /// A new grid is on screen. The photo is found in it by its id: an offset names a
    /// photo against one grid only, and one photo indexed into an earlier folder moves
    /// every offset after it. Found, it is at its new offset under the key the new grid
    /// has for it, and nothing else happens: no picture is made again for a renumbering.
    /// Not found, it has left the view or the library, and which is the engine's to say
    /// (`lost`): until it has, the photo is neither.
    pub fn follow(&mut self, index: &GridIndex) {
        match index.position_of(self.id) {
            Some(offset) => {
                self.offset = offset;
                self.held = Held::Here;
                if let Some(entry) = index.rows(offset, 1).first() {
                    self.key = entry.thumb_key;
                }
            }
            None if self.held == Held::Here => self.held = Held::Asked,
            None => {}
        }
    }

    /// The engine's answer about a photo the grid lost: it `exists` still, and has only
    /// left this view; or it does not. An answer that comes after the photo is back in
    /// the grid, or after a step, is not this photo's any more and changes nothing.
    pub fn lost(&mut self, id: i64, exists: bool) {
        if id == self.id && self.held != Held::Here {
            self.held = if exists { Held::Orphaned } else { Held::Gone };
        }
    }

    /// What the library says of photo `id` now. Answers whether the picture is another
    /// than the one it said before (`picture_changed`): it is then made again, and what
    /// was done to the old one on screen - its zoom, a crop begun - is over.
    pub fn took(&mut self, id: i64, fresh: Shown) -> bool {
        if id != self.id {
            return false;
        }
        let changed = (self.shown.as_ref()).is_some_and(|old| picture_changed(old, &fresh));
        if changed {
            // Under the key the picture had before, the question would be the one already
            // answered: turned and turned back. Not under the key the viewer has, which a
            // new grid has already moved to the new picture's (`follow`) - the picture of
            // an edit was asked for under it at once, and asked again it was made twice.
            if (self.shown.as_ref()).is_some_and(|old| old.key == fresh.key) {
                self.again += 1;
            }
            // Made again in place: nothing is being stepped past.
            self.stepped_at = None;
            self.zoom = Zoom::default();
        }
        self.key = fresh.key;
        self.shown = Some(fresh);
        changed
    }

    /// The photo's place in its section and how many that holds - a folder's photos, a
    /// month's, or the whole of a view with no headers. None for a photo the view does
    /// not hold.
    pub fn position(&self, index: &GridIndex) -> Option<(usize, usize)> {
        if self.held != Held::Here {
            return None;
        }
        let section = (index.sections().iter()).rfind(|section| section.offset <= self.offset)?;
        // Held to the section: the offset can outrun the sections for the frame a
        // shorter grid arrives in.
        Some((
            (self.offset - section.offset + 1).min(section.count),
            section.count,
        ))
    }

    /// Whether the full picture may be asked for at `now`: at once for the photo the
    /// viewer opened on, and after `REST_MS` on a photo it stepped to.
    pub fn rested(&self, now: f64) -> bool {
        self.rest_due().is_none_or(|due| now >= due)
    }

    /// When the rest is over, for the frame to ask for; none where there is none to wait
    /// out.
    pub fn rest_due(&self) -> Option<f64> {
        self.stepped_at.map(|at| at + REST_MS)
    }

    /// What the grid is handed when the viewer closes: the offset to show, and the photo
    /// to select there. A photo the view has lost hands back no id - whatever is at its
    /// offset now is another photo, and the selection must not hold one no tile shows.
    pub fn hand_back(&self, index: &GridIndex) -> (usize, Option<i64>) {
        let offset = self.offset.min(index.len().saturating_sub(1));
        (offset, (self.held == Held::Here).then_some(self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::grid::{GridEntry, Layout};

    fn entry(id: i64, folder_id: i64) -> GridEntry {
        GridEntry {
            id,
            folder_id,
            taken_at: 0,
            aspect: 1.5,
            kind: if id % 100 == 99 {
                MediaKind::Video
            } else {
                MediaKind::Image
            },
            duration_ms: None,
            starred: false,
            has_copies: false,
            thumb_key: id as u64 * 10,
            size: 0,
            mtime_ms: 0,
        }
    }

    /// A grid laid out by folder: each slice is a folder's photos, by id.
    fn folders(folders: &[&[i64]]) -> GridIndex {
        let entries = (folders.iter().enumerate())
            .flat_map(|(folder, ids)| ids.iter().map(move |id| entry(*id, folder as i64 + 1)))
            .collect();
        GridIndex::build(entries, Layout::Folders)
    }

    fn shown(key: u64) -> Shown {
        Shown {
            key,
            width: 4000,
            height: 3000,
            orientation: 1,
            failed: false,
            video: false,
        }
    }

    #[test]
    fn it_opens_on_a_photo_of_the_grid_and_on_none_the_grid_lacks() {
        let grid = folders(&[&[1, 2], &[3, 4, 5]]);
        let viewer = Viewer::open(4, &grid).unwrap();
        assert_eq!((viewer.id(), viewer.offset(), viewer.key()), (4, 3, 40));
        assert!(!viewer.orphaned() && !viewer.gone() && !viewer.is_video());
        assert_eq!(
            viewer.want(),
            Want {
                id: 4,
                key: 40,
                again: 0
            }
        );
        assert_eq!(Viewer::open(9, &grid), None);
        assert!(Viewer::open(199, &folders(&[&[199]])).unwrap().is_video());
    }

    #[test]
    fn a_step_goes_to_the_photo_beside_it_and_stops_at_the_ends() {
        let grid = folders(&[&[1, 2], &[3]]);
        let mut viewer = Viewer::open(1, &grid).unwrap();
        assert!(!viewer.can_step(-1, 3) && viewer.can_step(1, 3));
        assert!(
            !viewer.step(-1, &grid, 0.0),
            "before the first there is none"
        );
        assert_eq!(viewer.id(), 1);
        assert!(viewer.step(1, &grid, 0.0));
        assert_eq!((viewer.id(), viewer.offset(), viewer.key()), (2, 1, 20));
        assert!(viewer.step(1, &grid, 0.0));
        assert_eq!(viewer.id(), 3);
        assert!(!viewer.can_step(1, 3));
        assert!(!viewer.step(1, &grid, 0.0), "after the last there is none");
        assert!(viewer.step(-1, &grid, 0.0));
        assert_eq!(viewer.id(), 2);
    }

    #[test]
    fn home_and_end_go_to_the_first_and_the_last() {
        let grid = folders(&[&[1, 2, 3, 4]]);
        let mut viewer = Viewer::open(2, &grid).unwrap();
        assert!(viewer.go_to(usize::MAX, &grid, 0.0));
        assert_eq!(viewer.id(), 4);
        assert!(!viewer.go_to(3, &grid, 0.0), "it is there already");
        assert!(viewer.go_to(0, &grid, 0.0));
        assert_eq!(viewer.id(), 1);
        assert!(!viewer.go_to(0, &folders(&[]), 0.0));
    }

    // What the library said of the photo left is not said of the one stepped to.
    #[test]
    fn a_step_forgets_what_was_said_of_the_photo_left() {
        let grid = folders(&[&[1, 2]]);
        let mut viewer = Viewer::open(1, &grid).unwrap();
        viewer.took(1, shown(10));
        assert!(viewer.shown().is_some());
        viewer.step(1, &grid, 0.0);
        assert_eq!(viewer.shown(), None);
        // And an answer about the photo left, arriving now, is not this photo's.
        assert!(!viewer.took(1, shown(11)));
        assert_eq!((viewer.shown(), viewer.key()), (None, 20));
    }

    // An offset names a photo against one grid only: a photo indexed into an earlier
    // folder moves every offset after it, and by its offset the viewer would go on under
    // the same caption with the photo before.
    #[test]
    fn a_new_grid_is_followed_by_the_photos_id() {
        let mut viewer = Viewer::open(4, &folders(&[&[1, 2], &[3, 4, 5]])).unwrap();
        let before = viewer.want();
        viewer.follow(&folders(&[&[1, 7, 8, 2], &[3, 4, 5]]));
        assert_eq!((viewer.id(), viewer.offset()), (4, 5));
        // A renumbering makes no picture again.
        assert_eq!(viewer.want(), before);
        assert_eq!(
            viewer.position(&folders(&[&[1, 7, 8, 2], &[3, 4, 5]])),
            Some((2, 3))
        );
    }

    // An edit made from elsewhere while the photo is on screen: the new grid has it
    // under another key, which is the picture to show from then on.
    #[test]
    fn a_new_grid_brings_the_photos_new_key() {
        let mut viewer = Viewer::open(2, &folders(&[&[1, 2, 3]])).unwrap();
        assert_eq!(viewer.key(), 20);
        let mut entries: Vec<GridEntry> = (1..=3).map(|id| entry(id, 1)).collect();
        entries[1].thumb_key = 777;
        viewer.follow(&GridIndex::build(entries, Layout::Folders));
        assert_eq!(viewer.key(), 777);
        assert_eq!(
            viewer.want(),
            Want {
                id: 2,
                key: 777,
                again: 0
            }
        );
    }

    // Unstarred in Starred, hidden, taken off a person in that person's view: the photo
    // is not in the new grid and is a photo still. It stays, with no place, and the next
    // step goes on from where it was.
    #[test]
    fn a_photo_the_view_has_lost_stays_and_the_next_step_goes_on_from_its_place() {
        let grid = folders(&[&[1, 2, 3, 4]]);
        let mut viewer = Viewer::open(2, &grid).unwrap();
        let without = folders(&[&[1, 3, 4]]);
        viewer.follow(&without);
        assert!(
            !viewer.orphaned(),
            "not until the engine has said it exists"
        );
        assert_eq!(viewer.position(&without), None);
        // Nor is it handed back meanwhile as a photo the grid shows.
        assert_eq!(viewer.hand_back(&without), (1, None));
        viewer.lost(2, true);
        assert!(viewer.orphaned() && !viewer.gone());
        assert_eq!((viewer.id(), viewer.offset()), (2, 1));
        // The same grid again changes nothing.
        viewer.follow(&without);
        assert!(viewer.orphaned());
        assert_eq!(viewer.hand_back(&without), (1, None));
        // On: the photo that came after it, which stands where it stood.
        assert!(viewer.can_step(1, 3));
        let mut on = viewer.clone();
        assert!(on.step(1, &without, 0.0));
        assert_eq!((on.id(), on.offset(), on.orphaned()), (3, 1, false));
        // Back: the one before it.
        assert!(viewer.step(-1, &without, 0.0));
        assert_eq!((viewer.id(), viewer.offset()), (1, 0));
    }

    #[test]
    fn the_last_photo_of_a_view_lost_has_none_after_it() {
        let mut viewer = Viewer::open(3, &folders(&[&[1, 2, 3]])).unwrap();
        let without = folders(&[&[1, 2]]);
        viewer.follow(&without);
        viewer.lost(3, true);
        assert!(!viewer.can_step(1, 2));
        assert!(!viewer.step(1, &without, 0.0));
        // Its offset is past the end of the grid that lost it: the grid is handed the
        // last place it has.
        assert_eq!(viewer.hand_back(&without), (1, None));
        assert!(viewer.step(-1, &without, 0.0));
        assert_eq!(viewer.id(), 2);
        // The one before the last, lost: the last stands where it stood, and is next.
        let mut middle = Viewer::open(2, &folders(&[&[1, 2, 3]])).unwrap();
        let without = folders(&[&[1, 3]]);
        middle.follow(&without);
        middle.lost(2, true);
        assert!(middle.can_step(1, 2));
        assert!(middle.step(1, &without, 0.0));
        assert_eq!((middle.id(), middle.offset()), (3, 1));
        // And where it was the only one, there is nowhere to go.
        let mut alone = Viewer::open(1, &folders(&[&[1]])).unwrap();
        alone.follow(&folders(&[]));
        alone.lost(1, true);
        assert!(!alone.can_step(1, 0) && !alone.can_step(-1, 0));
        assert_eq!(alone.hand_back(&folders(&[])), (0, None));
    }

    #[test]
    fn a_photo_that_has_left_the_library_is_gone_and_one_that_comes_back_is_here() {
        let grid = folders(&[&[1, 2]]);
        let mut viewer = Viewer::open(2, &grid).unwrap();
        viewer.follow(&folders(&[&[1]]));
        viewer.lost(2, false);
        assert!(viewer.gone() && !viewer.orphaned());
        // Back in the grid - unhidden from elsewhere, its drive plugged in again.
        viewer.follow(&grid);
        assert!(!viewer.gone() && !viewer.orphaned());
        assert_eq!(viewer.hand_back(&grid), (1, Some(2)));
    }

    // A photo looked at closely is the fit again on the next photo and when it is another
    // picture - turned from elsewhere, say: the place that was zoomed into is not there.
    // Nothing else puts it back: not a star, not a new grid, not leaving the view.
    #[test]
    fn the_zoom_is_the_fit_again_on_another_photo_and_another_picture_and_on_nothing_else() {
        use crate::viewer_zoom::Pan;
        let grid = folders(&[&[1, 2, 3]]);
        let area = (800.0, 600.0);
        let zoomed = |viewer: &mut Viewer| {
            viewer.zoom_mut().to(2.0, Pan { x: 100.0, y: 0.0 }, area);
            *viewer.zoom()
        };
        let mut viewer = Viewer::open(2, &grid).unwrap();
        assert!(viewer.zoom().is_fit());
        let was = zoomed(&mut viewer);
        assert!(!was.is_fit());
        // What the library says, the first time and again the same; a new grid; the
        // photo gone from the view.
        viewer.took(2, shown(20));
        viewer.took(2, shown(20));
        viewer.follow(&folders(&[&[9, 1, 2, 3]]));
        viewer.follow(&folders(&[&[1, 3]]));
        viewer.lost(2, true);
        assert_eq!(*viewer.zoom(), was);
        // Another picture of it.
        viewer.took(2, shown(21));
        assert!(viewer.zoom().is_fit());
        // Another photo: a step, and Home.
        let mut viewer = Viewer::open(2, &grid).unwrap();
        zoomed(&mut viewer);
        assert!(viewer.step(1, &grid, 0.0));
        assert!(viewer.zoom().is_fit());
        zoomed(&mut viewer);
        assert!(viewer.go_to(0, &grid, 0.0));
        assert!(viewer.zoom().is_fit());
        // A step that leads nowhere leaves it.
        let was = zoomed(&mut viewer);
        assert!(!viewer.step(-1, &grid, 0.0));
        assert_eq!(*viewer.zoom(), was);
    }

    // The engine's answer is a round trip behind: by then the photo may be back, or the
    // user on another.
    #[test]
    fn an_answer_about_a_lost_photo_counts_only_while_it_is_that_photo_and_lost() {
        let grid = folders(&[&[1, 2, 3]]);
        let mut viewer = Viewer::open(2, &grid).unwrap();
        viewer.follow(&folders(&[&[1, 3]]));
        viewer.follow(&grid);
        viewer.lost(2, false);
        assert!(!viewer.gone(), "it is in the grid again");
        viewer.follow(&folders(&[&[1, 3]]));
        viewer.step(1, &folders(&[&[1, 3]]), 0.0);
        viewer.lost(2, false);
        assert!(!viewer.gone(), "the answer is about the photo left");
        assert_eq!(viewer.id(), 3);
        // Nor when the photo stepped to has been lost in its turn, and is still to be
        // answered about.
        viewer.follow(&folders(&[&[1]]));
        viewer.lost(2, false);
        assert!(!viewer.gone() && !viewer.orphaned());
        viewer.lost(3, true);
        assert!(viewer.orphaned());
    }

    #[test]
    fn a_picture_is_another_by_its_key_its_size_its_orientation_or_across_failed() {
        let old = shown(10);
        assert!(!picture_changed(&old, &old));
        assert!(picture_changed(&old, &Shown { key: 11, ..old }));
        assert!(picture_changed(&old, &Shown { width: 3000, ..old }));
        assert!(picture_changed(
            &old,
            &Shown {
                height: 4000,
                ..old
            }
        ));
        assert!(picture_changed(
            &old,
            &Shown {
                orientation: 6,
                ..old
            }
        ));
        assert!(picture_changed(
            &old,
            &Shown {
                failed: true,
                ..old
            }
        ));
        assert!(picture_changed(
            &Shown {
                failed: true,
                ..old
            },
            &old
        ));
        // A video's poster failing, or arriving, is not the video.
        let video = Shown { video: true, ..old };
        assert!(!picture_changed(
            &video,
            &Shown {
                failed: true,
                ..video
            }
        ));
    }

    #[test]
    fn what_the_library_says_makes_the_picture_again_only_when_it_is_another() {
        let grid = folders(&[&[1, 2]]);
        let mut viewer = Viewer::open(1, &grid).unwrap();
        // The first answer is what there is to compare with, not a change.
        assert!(!viewer.took(1, shown(10)));
        assert_eq!(
            viewer.want(),
            Want {
                id: 1,
                key: 10,
                again: 0
            }
        );
        // The same again: a star, a keyword, a face.
        assert!(!viewer.took(1, shown(10)));
        assert_eq!(viewer.want().again, 0);
        // An edit: another key, which is another picture by itself.
        assert!(viewer.took(1, shown(77)));
        assert_eq!(
            viewer.want(),
            Want {
                id: 1,
                key: 77,
                again: 0
            }
        );
        // The same bytes read anew: the key stands, and the picture is asked for again.
        assert!(viewer.took(
            1,
            Shown {
                orientation: 6,
                ..shown(77)
            }
        ));
        assert_eq!(
            viewer.want(),
            Want {
                id: 1,
                key: 77,
                again: 1
            }
        );
        assert!(viewer.took(
            1,
            Shown {
                orientation: 6,
                failed: true,
                ..shown(77)
            }
        ));
        assert_eq!(viewer.want().again, 2);
    }

    #[test]
    fn a_photo_is_numbered_in_its_section() {
        let grid = folders(&[&[1, 2], &[3, 4, 5]]);
        let position = |id| Viewer::open(id, &grid).unwrap().position(&grid);
        assert_eq!(position(1), Some((1, 2)));
        assert_eq!(position(2), Some((2, 2)));
        assert_eq!(position(3), Some((1, 3)));
        assert_eq!(position(5), Some((3, 3)));
        // A view with no headers is one run, numbered whole.
        let flat = GridIndex::build((1..=4).map(|id| entry(id, id)).collect(), Layout::Flat);
        assert_eq!(
            Viewer::open(3, &flat).unwrap().position(&flat),
            Some((3, 4))
        );
        // Against a grid shorter than the viewer's offset, for the frame it arrives in.
        let viewer = Viewer::open(5, &grid).unwrap();
        assert_eq!(viewer.position(&folders(&[&[1, 2], &[3]])), Some((1, 1)));
    }

    // A held arrow key repeats every 30 to 50 ms: none of the photos it passes is asked
    // for at full size, and the one it stops on is, 130 ms after.
    #[test]
    fn the_full_picture_waits_for_a_rest_after_a_step_and_not_at_the_opening() {
        let grid = folders(&[&[1, 2, 3]]);
        let mut viewer = Viewer::open(1, &grid).unwrap();
        assert!(viewer.rested(0.0));
        assert_eq!(viewer.rest_due(), None);
        viewer.step(1, &grid, 1000.0);
        assert!(!viewer.rested(1000.0) && !viewer.rested(1129.0));
        assert_eq!(viewer.rest_due(), Some(1130.0));
        assert!(viewer.rested(1130.0));
        // Stepped again before the rest was over: it begins again.
        viewer.step(1, &grid, 1040.0);
        assert!(!viewer.rested(1130.0));
        assert!(viewer.rested(1170.0));
        // A picture made again in place is not a photo stepped past.
        viewer.took(3, shown(30));
        viewer.step(-1, &grid, 2000.0);
        viewer.took(2, shown(20));
        assert!(!viewer.rested(2000.0));
        viewer.took(2, shown(21));
        assert!(viewer.rested(2000.0));
    }

    #[test]
    fn the_wheel_steps_at_a_whole_notch_and_carries_what_is_less() {
        assert_eq!(wheel_step(0.0, 1.0), (0.0, 1));
        assert_eq!(wheel_step(0.0, -1.0), (0.0, -1));
        // A trackpad: small turns add up.
        let (turned, step) = wheel_step(0.0, 0.4);
        assert_eq!((turned, step), (0.4, 0));
        let (turned, step) = wheel_step(turned, 0.4);
        assert_eq!(step, 0);
        assert_eq!(wheel_step(turned, 0.4), (0.0, 1));
        // What is over a notch is not carried: a flick is one photo, not two.
        assert_eq!(wheel_step(0.0, 3.5), (0.0, 1));
    }

    // Back the other way begins from nothing: what was banked one way is not a head
    // start the other, nor a debt.
    #[test]
    fn a_turn_the_other_way_begins_again() {
        let (turned, _) = wheel_step(0.0, 0.9);
        let (turned, step) = wheel_step(turned, -0.3);
        assert_eq!((turned, step), (-0.3, 0));
        let (turned, step) = wheel_step(turned, -0.3);
        assert_eq!(step, 0);
        assert!((turned + 0.6).abs() < 1e-6);
        let (_, step) = wheel_step(turned, -0.5);
        assert_eq!(step, -1);
    }

    #[test]
    fn closing_hands_the_grid_the_photo_and_where_it_is() {
        let grid = folders(&[&[1, 2, 3]]);
        let mut viewer = Viewer::open(1, &grid).unwrap();
        viewer.step(1, &grid, 0.0);
        assert_eq!(viewer.hand_back(&grid), (1, Some(2)));
    }
}
