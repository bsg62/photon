//! The thumbnails held as textures: which ones, how many bytes, what to upload this frame
//! and what to let go. Bookkeeping only - the texture itself is whatever `T` the caller
//! makes, so none of this names egui and all of it is tested without a GPU.

use photon_core::thumbs::ThumbSize;
use std::collections::{HashMap, HashSet, VecDeque};

/// What a texture is of. A thumbnail key names one picture (`Item::thumb_key`), and the
/// cache holds it at more than one size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexKey {
    pub key: u64,
    pub size: ThumbSize,
}

impl TexKey {
    pub fn grid(key: u64) -> Self {
        Self {
            key,
            size: ThumbSize::Grid,
        }
    }
}

/// A decoded picture on its way to the GPU: straight (not premultiplied) RGBA, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The most bytes of texture kept before the least recently drawn are let go. A grid
/// thumbnail is at most 256x256, so 262,144 bytes: this is about a thousand of them.
pub const DEFAULT_LIMIT: usize = 256 << 20;
/// Below the limit, what is kept is at most `SLACK` times what the view wants - the room to
/// scroll back into without asking the cache again - and never held to less than
/// `LIMIT_FLOOR`. At 1440p a grid wants about a hundred thumbnails, 25 MB: the store sat at
/// its whole limit for the session once a library had been scrolled through, under the
/// viewer and the People page too.
pub const SLACK: usize = 3;
pub const LIMIT_FLOOR: usize = 64 << 20;
/// Textures uploaded in one frame. An upload is a copy to the GPU on the UI thread, and a
/// jump to a new place brings a screenful at once.
pub const UPLOADS_PER_FRAME: usize = 32;
/// How long a thumbnail that was not there to be had is left alone before it is asked for
/// again, in seconds. Without it a photo whose thumbnail cannot be built in time - or that
/// is gone - would be asked for again on every frame it is in view.
pub const RETRY_SECS: f64 = 5.0;

/// How soon after it was asked for a thumbnail counts as already there, in seconds: by the
/// next frame or the one after at 60Hz - a thumbnail read from photon's cache, which would
/// have been on screen before a fade could be seen starting (`tile-fade.ts`'s
/// `INSTANT_LOAD_MS`).
pub const INSTANT_SECS: f64 = 0.032;
/// How long a thumbnail that arrives late takes to fade in, in seconds: the Svelte tile's
/// `transition: opacity 120ms`.
pub const FADE_SECS: f64 = 0.12;

struct Held<T> {
    texture: T,
    bytes: usize,
    /// The frame it was last drawn in.
    drawn: u64,
}

pub struct Textures<T> {
    held: HashMap<TexKey, Held<T>>,
    /// Decoded and not uploaded yet, oldest first.
    waiting: VecDeque<(TexKey, Pixels)>,
    waiting_keys: HashSet<TexKey>,
    /// Pictures that could not be made. Remembered so the tile draws its mark and nothing
    /// asks again on every frame.
    failed: HashSet<TexKey>,
    /// Thumbnails that were unavailable, and when each may be asked for again.
    put_off: HashMap<TexKey, f64>,
    /// Thumbnails that were unavailable and have not arrived since. Longer-lived than
    /// `put_off`, whose entry goes when the delay is over: the mark must not blink off for
    /// as long as the next attempt takes.
    troubled: HashSet<TexKey>,
    /// When each thumbnail on its way was first asked for; those the loader handed over
    /// late, waiting for their upload; and when each one fading in arrived: a late one
    /// fades in (`upload`'s `fade`).
    asked: HashMap<TexKey, f64>,
    late: HashSet<TexKey>,
    fading: HashMap<TexKey, f64>,
    bytes: usize,
    limit: usize,
    frame: u64,
}

impl<T> Textures<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            held: HashMap::new(),
            waiting: VecDeque::new(),
            waiting_keys: HashSet::new(),
            failed: HashSet::new(),
            put_off: HashMap::new(),
            troubled: HashSet::new(),
            asked: HashMap::new(),
            late: HashSet::new(),
            fading: HashMap::new(),
            bytes: 0,
            limit,
            frame: 0,
        }
    }

    /// Once per frame, before anything is drawn.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The texture for `key`, counted as drawn this frame.
    pub fn get(&mut self, key: TexKey) -> Option<&T> {
        let held = self.held.get_mut(&key)?;
        held.drawn = self.frame;
        Some(&held.texture)
    }

    /// Whether there is a texture for `key`, without counting it as drawn.
    pub fn holds(&self, key: TexKey) -> bool {
        self.held.contains_key(&key)
    }

    pub fn fail(&mut self, key: TexKey) {
        self.failed.insert(key);
    }

    pub fn failed(&self, key: TexKey) -> bool {
        self.failed.contains(&key)
    }

    /// The thumbnail was not there to be had at `now` (seconds): not built in time, or its
    /// photo gone. It is asked for again once `RETRY_SECS` have passed.
    pub fn put_off(&mut self, key: TexKey, now: f64) {
        self.put_off.insert(key, now + RETRY_SECS);
        self.troubled.insert(key);
    }

    /// A decoded thumbnail, to be uploaded. Dropped - `false` - when it is no longer
    /// wanted or is here already: a scroll has moved on, or the photo has another key by
    /// now (an edit, a rewritten file) and this is a picture nothing draws.
    pub fn offer(&mut self, key: TexKey, pixels: Pixels, wanted: &HashSet<TexKey>) -> bool {
        if !wanted.contains(&key)
            || self.held.contains_key(&key)
            || self.waiting_keys.contains(&key)
        {
            return false;
        }
        self.waiting_keys.insert(key);
        self.waiting.push_back((key, pixels));
        true
    }

    /// Whether the thumbnail was not to be had the last time it was asked for and has not
    /// arrived since. The tile shows its mark meanwhile, where `failed` is for good.
    pub fn troubled(&self, key: TexKey) -> bool {
        self.troubled.contains(&key)
    }

    /// When the earliest put-off thumbnail among `wanted` may be asked for again: the
    /// frame to ask for, since a still grid draws none by itself and would never retry.
    pub fn next_retry(&self, wanted: &HashSet<TexKey>) -> Option<f64> {
        self.put_off
            .iter()
            .filter(|(key, _)| wanted.contains(key))
            .map(|(_, until)| *until)
            .min_by(f64::total_cmp)
    }

    /// The keys of `wanted`, in its order, that have neither a texture, nor pixels waiting
    /// for upload, nor a failure, and are not put off past `now`: what to ask the loader for.
    pub fn missing(&mut self, wanted: &[TexKey], now: f64) -> Vec<TexKey> {
        self.put_off.retain(|_, until| *until > now);
        let missing: Vec<TexKey> = wanted
            .iter()
            .copied()
            .filter(|key| {
                !self.held.contains_key(key)
                    && !self.waiting_keys.contains(key)
                    && !self.failed.contains(key)
                    && !self.put_off.contains_key(key)
            })
            .collect();
        missing
    }

    /// `missing`, asked of the loader at `now`: when each was first asked for, for as long
    /// as it is still `wanted`. Not while the grid defers its asking - a scroll too fast
    /// to settle - or every tile the grid lands on would count from before it was asked
    /// for, and fade in, cached ones too: the Svelte tile counts from its `src` being set.
    pub fn asking(&mut self, missing: &[TexKey], wanted: &HashSet<TexKey>, now: f64) {
        self.asked.retain(|key, _| wanted.contains(key));
        for key in missing {
            self.asked.entry(*key).or_insert(now);
        }
    }

    /// The loader has handed over the thumbnail under `key` at `now`: late, if later than
    /// `INSTANT_SECS` after it was asked for. Counted here and not at its upload, which
    /// waits its turn - a few a frame - so that a screenful of thumbnails from photon's
    /// cache, uploaded over several frames, does not count as late and fade in.
    pub fn arrived(&mut self, key: TexKey, now: f64) {
        if let Some(asked) = self.asked.remove(&key)
            && now - asked > INSTANT_SECS
        {
            self.late.insert(key);
        }
    }

    /// How much of the thumbnail under `key` is seen at `now`: fading in from its arrival
    /// over `FADE_SECS`, or all of it.
    pub fn shown(&self, key: TexKey, now: f64) -> f32 {
        self.fading
            .get(&key)
            .map_or(1.0, |at| ((now - at) / FADE_SECS).clamp(0.0, 1.0) as f32)
    }

    /// Lets go of the fades that are over at `now`: whether any is still running, and a
    /// frame owed for it.
    pub fn fades(&mut self, now: f64) -> bool {
        self.fading.retain(|_, at| now - *at < FADE_SECS);
        !self.fading.is_empty()
    }

    /// Uploads up to `budget` waiting thumbnails through `make`, then lets go of the least
    /// recently drawn textures until the limit holds - `SLACK` times what `wanted` holds,
    /// between `LIMIT_FLOOR` and the limit. A texture in `wanted` is never let go, so a
    /// view that needs more than the limit keeps what it shows. `true` when thumbnails are
    /// still waiting, and another frame is owed.
    ///
    /// With `fade`, one the loader handed over late (`arrived`) fades in from `now`
    /// (`shown`), as the Svelte tile's `fadesIn` has it: one the cache had
    /// would be on screen before a fade could be seen, and one that arrives while the grid
    /// moves is sliding into view, where a fade still running is the blank tile the
    /// grid's lead is there to prevent - the caller says `fade` only of a still grid.
    pub fn upload(
        &mut self,
        budget: usize,
        wanted: &HashSet<TexKey>,
        (now, fade): (f64, bool),
        mut make: impl FnMut(TexKey, &Pixels) -> T,
    ) -> bool {
        let mut uploaded = 0;
        while uploaded < budget {
            let Some((key, pixels)) = self.waiting.pop_front() else {
                break;
            };
            self.waiting_keys.remove(&key);
            // Decoded for a place the grid has left since: not worth an upload.
            if !wanted.contains(&key) {
                continue;
            }
            if self.late.remove(&key) && fade {
                self.fading.insert(key, now);
            }
            let bytes = pixels.rgba.len();
            self.troubled.remove(&key);
            self.put_off.remove(&key);
            self.held.insert(
                key,
                Held {
                    texture: make(key, &pixels),
                    bytes,
                    drawn: self.frame,
                },
            );
            self.bytes += bytes;
            uploaded += 1;
        }
        let in_view: usize = (self.held.iter())
            .filter(|(key, _)| wanted.contains(key))
            .map(|(_, held)| held.bytes)
            .sum();
        let limit = (in_view.saturating_mul(SLACK).max(LIMIT_FLOOR)).min(self.limit);
        while self.bytes > limit {
            let oldest = self
                .held
                .iter()
                .filter(|(key, _)| !wanted.contains(key))
                .min_by_key(|(_, held)| held.drawn)
                .map(|(key, _)| *key);
            let Some(key) = oldest else {
                break;
            };
            if let Some(held) = self.held.remove(&key) {
                self.bytes -= held.bytes;
            }
        }
        !self.waiting.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The view is covered - the viewer, compare, the People page - and draws nothing:
    /// lets go of every texture but those drawn in its last frame, which are on screen
    /// again the moment it is uncovered, and of what waited to be uploaded.
    pub fn rest(&mut self) {
        let frame = self.frame;
        self.held.retain(|_, held| held.drawn == frame);
        self.bytes = self.held.values().map(|held| held.bytes).sum();
        self.waiting.clear();
        self.waiting_keys.clear();
    }

    /// Lets go of everything: the textures, what waited to be one, and what was known
    /// of those that could not be had. The limit and the count of frames stay.
    pub fn clear(&mut self) {
        *self = Self {
            limit: self.limit,
            frame: self.frame,
            ..Self::new(self.limit)
        };
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 picture: 16 bytes.
    fn pixels() -> Pixels {
        Pixels {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        }
    }

    fn keys(range: std::ops::Range<u64>) -> Vec<TexKey> {
        range.map(TexKey::grid).collect()
    }

    fn set(keys: &[TexKey]) -> HashSet<TexKey> {
        keys.iter().copied().collect()
    }

    /// Offers and uploads every key of `wanted`, as a frame with a large budget does.
    fn load(textures: &mut Textures<u64>, wanted: &[TexKey]) {
        let wanted_set = set(wanted);
        for key in wanted {
            textures.offer(*key, pixels(), &wanted_set);
        }
        textures.upload(usize::MAX, &wanted_set, (0.0, false), |key, _| key.key);
    }

    /// A thumbnail asked for at `asked` and uploaded at `arrived`, with `fade` said of the
    /// grid then: how much of it is seen as it arrives, and `FADE_SECS` later.
    fn arriving(asked: f64, arrived: f64, fade: bool) -> (f32, f32, bool) {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..1);
        let wanted_set = set(&wanted);
        assert_eq!(textures.missing(&wanted, asked), wanted);
        textures.asking(&wanted, &wanted_set, asked);
        textures.offer(wanted[0], pixels(), &wanted_set);
        textures.arrived(wanted[0], arrived);
        textures.upload(usize::MAX, &wanted_set, (arrived, fade), |key, _| key.key);
        let at_once = textures.shown(wanted[0], arrived);
        let after = textures.shown(wanted[0], arrived + FADE_SECS);
        (at_once, after, textures.fades(arrived + FADE_SECS))
    }

    // A thumbnail that arrives late onto a still grid fades in over `FADE_SECS`, and then
    // asks for no more frames; one the cache had at once, and one that arrives while the
    // grid moves, are there whole (`tile-fade.ts`'s `fadesIn`).
    #[test]
    fn a_thumbnail_that_arrives_late_on_a_still_grid_fades_in() {
        assert_eq!(arriving(1.0, 1.5, true), (0.0, 1.0, false));
        assert_eq!(
            arriving(1.0, 1.0 + INSTANT_SECS / 2.0, true),
            (1.0, 1.0, false)
        );
        assert_eq!(arriving(1.0, 1.5, false), (1.0, 1.0, false));
        // Half way, half of it, and a frame owed for the rest.
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..1);
        let wanted_set = set(&wanted);
        textures.missing(&wanted, 0.0);
        textures.asking(&wanted, &wanted_set, 0.0);
        textures.offer(wanted[0], pixels(), &wanted_set);
        textures.arrived(wanted[0], 1.0);
        textures.upload(usize::MAX, &wanted_set, (1.0, true), |key, _| key.key);
        assert!((textures.shown(wanted[0], 1.0 + FADE_SECS / 2.0) - 0.5).abs() < 1e-3);
        assert!(textures.fades(1.0 + FADE_SECS / 2.0));
    }

    #[test]
    fn an_uploaded_thumbnail_is_there_to_draw() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..3);
        assert_eq!(textures.missing(&wanted, 0.0), wanted);
        load(&mut textures, &wanted);
        assert_eq!(textures.get(TexKey::grid(1)), Some(&1));
        assert_eq!(textures.get(TexKey::grid(7)), None);
        assert_eq!((textures.len(), textures.bytes()), (3, 48));
        assert!(textures.missing(&wanted, 0.0).is_empty());
    }

    #[test]
    fn no_more_than_the_budget_is_uploaded_in_a_frame() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..5);
        let wanted_set = set(&wanted);
        for key in &wanted {
            assert!(textures.offer(*key, pixels(), &wanted_set));
        }
        let mut made = Vec::new();
        let more = textures.upload(2, &wanted_set, (0.0, false), |key, _| made.push(key.key));
        assert_eq!(made, [0, 1], "oldest first");
        assert!(more, "three are still waiting");
        // Waiting for upload is not missing: the loader is not asked for it again.
        assert!(textures.missing(&wanted, 0.0).is_empty());
        assert!(
            !textures.upload(usize::MAX, &wanted_set, (0.0, false), |key, _| made
                .push(key.key))
        );
        assert_eq!(made, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn the_least_recently_drawn_go_first_once_over_the_limit() {
        // Room for three 16-byte textures.
        let mut textures = Textures::new(48);
        load(&mut textures, &keys(0..3));
        textures.begin_frame();
        // 0 and 2 are drawn again; 1 is not.
        textures.get(TexKey::grid(0));
        textures.get(TexKey::grid(2));
        textures.begin_frame();
        load(&mut textures, &keys(3..4));
        assert_eq!(
            textures.get(TexKey::grid(1)),
            None,
            "the one not drawn went"
        );
        for kept in [0, 2, 3] {
            assert_eq!(textures.get(TexKey::grid(kept)), Some(&kept));
        }
        assert_eq!(textures.bytes(), 48);
    }

    // A view that needs more than the limit - small tiles on a large screen - keeps what
    // it shows: letting go of a texture on screen would blank a tile to save memory.
    #[test]
    fn a_wanted_texture_is_never_let_go() {
        let mut textures = Textures::new(32);
        let wanted = keys(0..5);
        load(&mut textures, &wanted);
        assert_eq!(textures.len(), 5);
        assert_eq!(textures.bytes(), 80, "over the limit, and all of it wanted");
        // Once the view moves on, the limit holds again.
        load(&mut textures, &keys(5..6));
        assert!(textures.bytes() <= 32);
        assert_eq!(textures.get(TexKey::grid(5)), Some(&5));
    }

    /// A picture of a mebibyte: four grid thumbnails.
    fn large() -> Pixels {
        Pixels {
            width: 512,
            height: 512,
            rgba: vec![0; 1 << 20],
        }
    }

    fn load_large(textures: &mut Textures<u64>, wanted: &[TexKey]) {
        let wanted_set = set(wanted);
        for key in wanted {
            textures.offer(*key, large(), &wanted_set);
        }
        textures.upload(usize::MAX, &wanted_set, (0.0, false), |key, _| key.key);
    }

    // Under the limit, what is kept is three times what the view wants, and no less than
    // the floor: a hundred megabytes scrolled through and thirty in view keep ninety; ten
    // in view keep sixty-four.
    #[test]
    fn what_is_kept_is_three_times_what_is_wanted_above_a_floor() {
        let mib = 1 << 20;
        let mut textures = Textures::new(DEFAULT_LIMIT);
        load_large(&mut textures, &keys(0..100));
        assert_eq!(textures.bytes(), 100 * mib);
        textures.begin_frame();
        load_large(&mut textures, &keys(70..100));
        assert_eq!(textures.bytes(), 90 * mib);
        for key in 70..100 {
            assert!(textures.holds(TexKey::grid(key)), "{key} is in view");
        }
        textures.begin_frame();
        load_large(&mut textures, &keys(90..100));
        assert_eq!(textures.bytes(), LIMIT_FLOOR);
    }

    // Covered, the view keeps what it drew in its last frame, and nothing waits to be
    // uploaded for it.
    #[test]
    fn at_rest_only_what_was_drawn_last_is_kept() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        load(&mut textures, &keys(0..4));
        textures.begin_frame();
        textures.get(TexKey::grid(1));
        textures.get(TexKey::grid(2));
        let wanted = set(&keys(5..6));
        assert!(textures.offer(TexKey::grid(5), pixels(), &wanted));
        textures.rest();
        assert_eq!((textures.len(), textures.bytes()), (2, 32));
        assert!(textures.holds(TexKey::grid(1)) && textures.holds(TexKey::grid(2)));
        assert_eq!(textures.missing(&keys(5..6), 0.0), keys(5..6));
    }

    // A result that comes back after the scroll has moved on, or under a key the photo no
    // longer has, is a picture nothing will draw.
    #[test]
    fn a_thumbnail_no_longer_wanted_is_dropped() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let now_wanted = set(&keys(10..12));
        assert!(!textures.offer(TexKey::grid(3), pixels(), &now_wanted));
        // Wanted when it was decoded, not by the frame that would upload it.
        assert!(textures.offer(TexKey::grid(10), pixels(), &now_wanted));
        let moved_on = set(&keys(20..22));
        assert!(!textures.upload(usize::MAX, &moved_on, (0.0, false), |key, _| key.key));
        assert!(textures.is_empty());
        assert_eq!(
            textures.missing(&keys(10..11), 0.0),
            keys(10..11),
            "and may be asked for again"
        );
    }

    #[test]
    fn a_thumbnail_already_here_is_not_taken_twice() {
        let mut textures = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..1);
        let wanted_set = set(&wanted);
        assert!(textures.offer(wanted[0], pixels(), &wanted_set));
        assert!(
            !textures.offer(wanted[0], pixels(), &wanted_set),
            "already waiting"
        );
        textures.upload(usize::MAX, &wanted_set, (0.0, false), |key, _| key.key);
        assert!(
            !textures.offer(wanted[0], pixels(), &wanted_set),
            "already held"
        );
        assert_eq!(textures.bytes(), 16);
    }

    #[test]
    fn a_thumbnail_that_was_unavailable_is_left_alone_for_a_while() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..2);
        textures.put_off(wanted[0], 100.0);
        assert_eq!(textures.missing(&wanted, 100.0), keys(1..2));
        assert_eq!(
            textures.missing(&wanted, 100.0 + RETRY_SECS - 0.1),
            keys(1..2)
        );
        assert_eq!(textures.missing(&wanted, 100.0 + RETRY_SECS), wanted);
        // And it is not a failure: nothing draws a mark for it.
        assert!(!textures.failed(wanted[0]));
    }

    // A video with no poster, a photo on a drive that is not there: until its thumbnail
    // arrives the tile has a mark to show, and is not a blank the eye waits on.
    #[test]
    fn a_thumbnail_that_was_not_to_be_had_is_marked_until_it_arrives() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..2);
        let wanted_set = set(&wanted);
        assert!(!textures.troubled(wanted[0]));
        textures.put_off(wanted[0], 100.0);
        assert!(textures.troubled(wanted[0]));
        // Still marked once the delay is over and it is being asked for again.
        assert_eq!(textures.missing(&wanted, 100.0 + RETRY_SECS), wanted);
        assert!(textures.troubled(wanted[0]));
        // And no longer once the picture is there.
        assert!(textures.offer(wanted[0], pixels(), &wanted_set));
        textures.upload(usize::MAX, &wanted_set, (0.0, false), |key, _| key.key);
        assert!(!textures.troubled(wanted[0]));
        assert!(!textures.troubled(wanted[1]));
    }

    // A still grid draws no frame by itself, so the retry has to be asked for.
    #[test]
    fn the_next_retry_is_the_earliest_among_what_is_wanted() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = set(&keys(0..2));
        assert_eq!(textures.next_retry(&wanted), None);
        textures.put_off(TexKey::grid(1), 20.0);
        textures.put_off(TexKey::grid(0), 10.0);
        // Put off, but scrolled away from: nothing waits on it.
        textures.put_off(TexKey::grid(9), 1.0);
        assert_eq!(textures.next_retry(&wanted), Some(10.0 + RETRY_SECS));
    }

    #[test]
    fn a_failure_is_remembered_and_not_asked_for_again() {
        let mut textures: Textures<u64> = Textures::new(DEFAULT_LIMIT);
        let wanted = keys(0..2);
        textures.fail(wanted[0]);
        assert!(textures.failed(wanted[0]));
        assert!(!textures.failed(wanted[1]));
        assert_eq!(textures.missing(&wanted, 0.0), keys(1..2));
    }
}
