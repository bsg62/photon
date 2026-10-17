//! Where each photo and header is drawn while the grid moves from one layout to another
//! (a column change, the user's own write). The grid view asks for every key's place in
//! every frame; this says where between the old rows and the new it is.
//!
//! Places are measured from the grid's top edge (the row's layout top less the scroll
//! position), so the screen moves as one: a scroll during a glide moves both ends of
//! every photo by the same distance, and a photo is never drawn by a path through a
//! place the user has already scrolled away from.

use std::collections::{HashMap, HashSet};

use photon_core::grid::{GridEntry, Period};

use crate::animate::{DURATION_MS, ease_out};

/// What a drawn thing is, across two layouts: a photo by its id, a header by what it
/// heads. Not an offset, which names another thing in the new rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Item(i64),
    Header {
        folder: Option<i64>,
        period: Option<Period>,
    },
}

/// A rectangle in the grid's coordinates, `y` from the grid's top edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Place {
    fn towards(self, to: Place, t: f64) -> Place {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Place {
            x: mix(self.x, to.x),
            y: mix(self.y, to.y),
            width: mix(self.width, to.width),
            height: mix(self.height, to.height),
        }
    }

    fn down(self, by: f64) -> Place {
        Place {
            y: self.y + by,
            ..self
        }
    }
}

/// A photo the new grid does not hold, kept to be faded out where it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaving {
    pub entry: GridEntry,
    pub place: Place,
}

/// A photo fading out, and the opacity it fades from: less than whole for one that was
/// already fading when a glide began over it.
#[derive(Clone, Copy)]
struct Fading {
    gone: Leaving,
    from: f32,
}

#[derive(Default)]
pub struct Glides {
    from: HashMap<Key, Place>,
    leaving: Vec<Fading>,
    started_ms: f64,
    position_at_start: f64,
}

impl Glides {
    /// Starts a glide. `before` and `leaving` are places on screen at `old_position`
    /// (the layout top less it), as far as the snapshot reaches; `new_position` is where
    /// the grid is once it has the new rows - a column change moves it, by the pin, far
    /// from where it was. A key already gliding starts from where it is drawn at `now_ms`
    /// instead (`before` is where its last glide was going). Every start is kept as the
    /// place it is on screen, measured from `new_position`, so that `place` at
    /// `new_position` draws each exactly where it stood.
    ///
    /// A photo still fading out goes on fading from where and how faint it is drawn:
    /// the columns changing while a hidden photo fades - the window resized, the
    /// sidebar toggled - would otherwise take it away at once.
    pub fn begin(
        &mut self,
        before: impl IntoIterator<Item = (Key, Place)>,
        leaving: Vec<Leaving>,
        old_position: f64,
        new_position: f64,
        now_ms: f64,
    ) {
        let running = self.running(now_ms);
        let t = self.progress(now_ms);
        let still_fading = self.leaving(old_position, now_ms).into_iter();
        let mut fading: Vec<Fading> = still_fading
            .map(|(gone, from)| Fading { gone, from })
            .collect();
        fading.extend(leaving.into_iter().map(|gone| Fading { gone, from: 1.0 }));
        let scrolled = self.position_at_start - old_position;
        let mut from = HashMap::new();
        for (key, old) in before {
            let start = match self.from.get(&key) {
                // Drawn between its old place (moved by the scroll) and the one it
                // was going to, which is what `before` holds for it.
                Some(was) if running => was.down(scrolled).towards(old, t),
                _ => old,
            };
            from.insert(key, start);
        }
        self.from = from;
        self.leaving = fading;
        self.started_ms = now_ms;
        self.position_at_start = new_position;
    }

    /// The grid was put at `new_position` from `old_position` without a scroll - the pin
    /// finding the photo at the top in rows a pixel wider, the end held to rows that
    /// grew shorter - and with no glide begun: what each key glides from stays where it
    /// stands on screen. Taken for a scroll, every photo would jump by the distance the
    /// pin moved, which deep in a library is a row for every row above the top.
    pub fn repositioned(&mut self, old_position: f64, new_position: f64) {
        self.position_at_start += new_position - old_position;
    }

    /// Where `key` is drawn at `now_ms` given its new place (relative to `position`):
    /// the old place moved by the scroll since the start, eased towards `target`.
    /// A key with no old place is drawn at `target`.
    pub fn place(&self, key: Key, target: Place, position: f64, now_ms: f64) -> Place {
        match self.from.get(&key) {
            Some(old) if self.running(now_ms) => old
                .down(self.position_at_start - position)
                .towards(target, self.progress(now_ms)),
            _ => target,
        }
    }

    /// The keys whose old place, moved by the scroll, is in view: drawn during the glide
    /// though their new place is out of view.
    pub fn visible_before(&self, position: f64, viewport: f64) -> Vec<Key> {
        let scrolled = self.position_at_start - position;
        self.from
            .iter()
            .filter(|(_, old)| {
                let top = old.y + scrolled;
                top < viewport && top + old.height > 0.0
            })
            .map(|(key, _)| *key)
            .collect()
    }

    /// The photos that left, each with its opacity at `now_ms`, drawn where they were.
    pub fn leaving(&self, position: f64, now_ms: f64) -> Vec<(Leaving, f32)> {
        if !self.running(now_ms) {
            return Vec::new();
        }
        let left = (1.0 - self.progress(now_ms)) as f32;
        let scrolled = self.position_at_start - position;
        self.leaving
            .iter()
            .map(|fading| {
                let moved = Leaving {
                    place: fading.gone.place.down(scrolled),
                    ..fading.gone
                };
                (moved, fading.from * left)
            })
            .collect()
    }

    pub fn running(&self, now_ms: f64) -> bool {
        (!self.from.is_empty() || !self.leaving.is_empty())
            && now_ms - self.started_ms < DURATION_MS
    }

    pub fn stop(&mut self) {
        self.from.clear();
        self.leaving.clear();
    }

    /// The eased share of the way, 0 before the start and 1 from the end on.
    fn progress(&self, now_ms: f64) -> f64 {
        ease_out((now_ms - self.started_ms) / DURATION_MS)
    }
}

/// The user's own writes that change which photos a view holds, by their numbers on the
/// queue of writes, for the grid each produces: that grid glides, and any other does not.
/// The engine publishes inside the command, before the answer is pushed, so the grid is
/// adopted while the write is on its way, or after its answer has been read - and then
/// only if a grid published was still waiting when the answer was read. Without that
/// last, an answer read in the frame after its grid was put on screen, or a write
/// refused, left the next grid adopted to glide, however late: a scan's, minutes on.
#[derive(Default)]
pub struct Moving {
    on_way: HashSet<u64>,
    /// A write answered while its grid had still to be adopted.
    landing: bool,
}

impl Moving {
    pub fn pushed(&mut self, number: u64) {
        self.on_way.insert(number);
    }

    /// The write `number` answered, made or refused. `waiting` says whether the engine
    /// has published a grid that is not on screen yet, asked only of a write counted here.
    pub fn answered(&mut self, number: u64, waiting: impl FnOnce() -> bool) {
        if self.on_way.remove(&number) && waiting() {
            self.landing = true;
        }
    }

    /// A grid adopted: whether it is the user's own write landing.
    pub fn adopted(&mut self) -> bool {
        let users = !self.on_way.is_empty() || self.landing;
        self.landing = false;
        users
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Place {
        Place {
            x,
            y,
            width: 100.0,
            height: 100.0,
        }
    }

    fn one() -> Key {
        Key::Item(1)
    }

    fn started() -> Glides {
        let mut glides = Glides::default();
        glides.begin([(one(), at(8.0, 0.0))], Vec::new(), 0.0, 0.0, 1000.0);
        glides
    }

    #[test]
    fn a_photo_glides_from_its_old_place_to_its_new() {
        let glides = started();
        let target = at(8.0, 108.0);
        let half = glides.place(one(), target, 0.0, 1100.0);
        assert!((half.y - 108.0 * ease_out(0.5)).abs() < 1e-9);
        assert_eq!(glides.place(one(), target, 0.0, 1000.0), at(8.0, 0.0));
        assert_eq!(glides.place(one(), target, 0.0, 1200.0), target);
    }

    #[test]
    fn a_scroll_during_a_glide_moves_both_ends() {
        let glides = started();
        // Scrolled down by 50: the target, given relative to the new position, is 50
        // higher than it was, and so is the place the photo started from.
        let target = at(8.0, 108.0 - 50.0);
        let drawn = glides.place(one(), target, 50.0, 1100.0);
        let unscrolled = glides.place(one(), at(8.0, 108.0), 0.0, 1100.0);
        assert!((drawn.y - (unscrolled.y - 50.0)).abs() < 1e-9);
    }

    #[test]
    fn a_key_already_gliding_starts_again_from_where_it_is_drawn() {
        let mut glides = started();
        let target = at(8.0, 108.0);
        let drawn = glides.place(one(), target, 0.0, 1100.0);
        // The second glide's `before` is where the first was going.
        glides.begin([(one(), target)], Vec::new(), 0.0, 0.0, 1100.0);
        assert_eq!(glides.place(one(), at(8.0, 300.0), 0.0, 1100.0), drawn);
    }

    #[test]
    fn a_first_glide_across_a_pin_jump_starts_on_screen_where_it_was() {
        let mut glides = Glides::default();
        let seen = at(8.0, 120.0);
        glides.begin([(one(), seen)], Vec::new(), 5000.0, 4200.0, 1000.0);
        assert_eq!(glides.place(one(), at(8.0, 300.0), 4200.0, 1000.0), seen);
    }

    #[test]
    fn a_re_target_across_a_pin_jump_does_not_move_the_drawn_photo() {
        let mut glides = Glides::default();
        glides.begin([(one(), at(8.0, 0.0))], Vec::new(), 900.0, 700.0, 1000.0);
        // Scrolled on to 750; the photo was going to 108 from the grid's top at 700,
        // so on screen now it is 50 higher.
        let target = at(8.0, 108.0 - 50.0);
        let drawn = glides.place(one(), target, 750.0, 1100.0);
        // The new rows put the grid at 3000; the old target stands where it was drawn
        // for, on screen at 750.
        glides.begin([(one(), target)], Vec::new(), 750.0, 3000.0, 1100.0);
        assert_eq!(glides.place(one(), at(8.0, 400.0), 3000.0, 1100.0), drawn);
    }

    #[test]
    fn a_grid_put_elsewhere_without_a_scroll_leaves_the_start_on_screen() {
        let mut glides = Glides::default();
        glides.begin(
            [(one(), at(8.0, 120.0))],
            Vec::new(),
            5000.0,
            5000.0,
            1000.0,
        );
        // The pin moved the grid to 5250 under rows a pixel wider; the targets are
        // measured from there, and where each photo starts from is where it was.
        glides.repositioned(5000.0, 5250.0);
        assert_eq!(
            glides.place(one(), at(8.0, 300.0), 5250.0, 1000.0),
            at(8.0, 120.0)
        );
        assert_eq!(glides.visible_before(5250.0, 700.0), vec![one()]);
    }

    #[test]
    fn a_key_with_no_old_place_is_at_its_target() {
        let glides = started();
        let target = at(40.0, 40.0);
        assert_eq!(glides.place(Key::Item(2), target, 0.0, 1100.0), target);
    }

    fn entry(id: i64) -> GridEntry {
        GridEntry {
            id,
            folder_id: 1,
            taken_at: 0,
            aspect: 1.0,
            kind: photon_core::media::MediaKind::Image,
            duration_ms: None,
            starred: false,
            has_copies: false,
            thumb_key: id as u64,
            size: 0,
            mtime_ms: 0,
        }
    }

    #[test]
    fn a_leaving_photo_fades_out_and_is_gone_at_the_end() {
        let mut glides = Glides::default();
        let gone = Leaving {
            entry: entry(9),
            place: at(8.0, 8.0),
        };
        glides.begin([], vec![gone], 0.0, 0.0, 1000.0);
        assert_eq!(glides.leaving(0.0, 1000.0)[0].1, 1.0);
        let half = glides.leaving(0.0, 1100.0)[0].1;
        assert!((half - (1.0 - ease_out(0.5)) as f32).abs() < 1e-6);
        assert!(glides.running(1100.0));
        assert!(!glides.running(1200.0));
        assert!(glides.leaving(0.0, 1200.0).is_empty());
    }

    #[test]
    fn visible_before_lists_what_was_in_view() {
        let mut glides = Glides::default();
        glides.begin(
            [
                (Key::Item(1), at(0.0, 50.0)),
                (Key::Item(2), at(0.0, 900.0)),
            ],
            Vec::new(),
            0.0,
            0.0,
            0.0,
        );
        assert_eq!(glides.visible_before(0.0, 700.0), vec![Key::Item(1)]);
        // Scrolled down by 300 the second is the one in view.
        assert_eq!(glides.visible_before(300.0, 700.0), vec![Key::Item(2)]);
    }

    // A hidden photo fading when the columns change - the window resized under it - goes
    // on fading where it is drawn, from how faint it is, and is gone when the new glide
    // ends: not taken away by the glide that began over it.
    #[test]
    fn a_photo_still_fading_goes_on_through_a_glide_that_begins_over_it() {
        let mut glides = Glides::default();
        let gone = Leaving {
            entry: entry(9),
            place: at(8.0, 8.0),
        };
        glides.begin([(one(), at(116.0, 8.0))], vec![gone], 0.0, 0.0, 1000.0);
        // Scrolled on by 50, and the new rows put the grid at 3000.
        let [(drawn, faint)] = glides.leaving(50.0, 1100.0)[..] else {
            panic!("one photo fading");
        };
        glides.begin([(one(), at(8.0, 8.0))], Vec::new(), 50.0, 3000.0, 1100.0);
        assert_eq!(glides.leaving(3000.0, 1100.0), vec![(drawn, faint)]);
        let half = glides.leaving(3000.0, 1200.0)[0].1;
        assert!((half - faint * (1.0 - ease_out(0.5)) as f32).abs() < 1e-6);
        assert!(glides.leaving(3000.0, 1300.0).is_empty());
    }

    #[test]
    fn a_write_answered_after_its_grid_was_adopted_leaves_nothing_for_the_next() {
        let mut moving = Moving::default();
        moving.pushed(1);
        assert!(moving.adopted(), "its grid, adopted while it is on its way");
        moving.answered(1, || false);
        assert!(!moving.adopted(), "a scan's grid, after");
    }

    #[test]
    fn a_write_answered_before_its_grid_was_adopted_glides_it() {
        let mut moving = Moving::default();
        moving.pushed(1);
        moving.answered(1, || true);
        assert!(moving.adopted(), "its grid");
        assert!(!moving.adopted(), "the grid after it");
    }

    #[test]
    fn a_refused_write_leaves_nothing_for_the_next_grid() {
        let mut moving = Moving::default();
        moving.pushed(1);
        moving.answered(1, || false);
        assert!(!moving.adopted());
    }

    #[test]
    fn a_write_that_moves_nothing_is_not_asked_about() {
        let mut moving = Moving::default();
        moving.answered(1, || panic!("asked"));
        assert!(!moving.adopted());
    }
}
