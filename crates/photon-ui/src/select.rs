//! The logic of photon's own dropdown: which option is active, when the list is open, and
//! what each key does - the WAI-ARIA "select-only combobox". `select.svelte.ts` on a clock
//! that is a number; `select_view.rs` draws it.
//!
//! It holds no value of its own. What is selected, the labels and whether the control can
//! be opened at all are the caller's, given with every call (`Options`), and a choice is
//! answered, not applied: the sort a control shows is the one the user is going to, and a
//! refused change puts it back by itself.
//!
//! No egui here.

/// How long a pause ends a type-ahead word: the APG's suggested half second.
pub const TYPEAHEAD_RESET_MS: f64 = 500.0;
/// How far PageUp and PageDown move.
const PAGE: usize = 10;

/// A key, as the list reads it. `Char` is a printable character typed with no Ctrl, Alt
/// or Command beside it - a shortcut is not type-ahead - and a space is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectKey {
    ArrowDown,
    ArrowUp {
        alt: bool,
    },
    Home,
    End,
    PageDown,
    PageUp,
    Escape,
    Enter,
    Tab,
    Char(char),
    /// Any other key: none of the list's.
    Other,
}

/// What the control is asked about: the caller's, at every call.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    pub labels: &'a [&'a str],
    /// The option the control holds.
    pub selected: Option<usize>,
    /// While set the list cannot be opened: no click, no key.
    pub disabled: bool,
}

/// What a key came to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Whether the control used the key: the caller's cue to keep it from anything else.
    /// Tab chooses and still reports false: it has to move the focus on.
    pub used: bool,
    /// The option chosen, when it is another than the one held: choosing the one already
    /// held is not a change.
    pub chosen: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Select {
    open: bool,
    active: usize,
    /// The word being typed, and when its last letter was.
    typed: String,
    typed_at: f64,
}

impl Select {
    pub fn open(&self) -> bool {
        self.open
    }

    /// The option the keys and the pointer are on, while the list is open.
    pub fn active(&self) -> usize {
        self.active
    }

    fn show_at(&mut self, at: Option<usize>, options: &Options<'_>) {
        let count = options.labels.len();
        if options.disabled || count == 0 {
            return;
        }
        self.active = at.unwrap_or(0).min(count - 1);
        self.open = true;
    }

    /// Opens the list on the option held.
    pub fn show(&mut self, options: &Options<'_>) {
        self.show_at(options.selected, options);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.typed.clear();
    }

    /// A press on the closed control, or on it while its list is open.
    pub fn toggle(&mut self, options: &Options<'_>) {
        if self.open {
            self.close();
        } else {
            self.show(options);
        }
    }

    /// The pointer over an option makes it the active one, as a native list does.
    pub fn hover(&mut self, index: usize, options: &Options<'_>) {
        self.active = index.min(options.labels.len().saturating_sub(1));
    }

    /// Closes the list on `index`. Answers it when it is another than the one held.
    pub fn commit(&mut self, index: usize, options: &Options<'_>) -> Option<usize> {
        self.close();
        (Some(index) != options.selected).then_some(index)
    }

    /// Moves to the next option whose label starts with what has been typed. A word being
    /// typed is matched from the active option itself, so "de" stays on "Date..." while it
    /// grows; a single letter repeated steps past it, cycling through the options that
    /// start with that letter, which is how a native select answers "d", "d", "d".
    fn typeahead(&mut self, typed: char, now_ms: f64, labels: &[&str]) {
        if now_ms - self.typed_at >= TYPEAHEAD_RESET_MS {
            self.typed.clear();
        }
        self.typed_at = now_ms;
        self.typed.extend(typed.to_lowercase());
        let mut letters = self.typed.chars();
        let first = letters.next().unwrap_or(typed);
        let cycling = self.typed.chars().count() > 1 && letters.all(|letter| letter == first);
        let word = if cycling {
            first.to_string()
        } else {
            self.typed.clone()
        };
        let from = if word.chars().count() == 1 {
            self.active + 1
        } else {
            self.active
        };
        let count = labels.len();
        let found = (0..count)
            .map(|step| (from + step) % count)
            .find(|&index| labels[index].to_lowercase().starts_with(&word));
        if let Some(index) = found {
            self.active = index;
        }
    }

    /// Applies a key at `now_ms`.
    pub fn key(&mut self, key: SelectKey, now_ms: f64, options: &Options<'_>) -> Answer {
        let count = options.labels.len();
        let used = Answer {
            used: true,
            chosen: None,
        };
        if count == 0 || options.disabled {
            return Answer::default();
        }
        let last = count - 1;
        if !self.open {
            match key {
                SelectKey::ArrowDown | SelectKey::ArrowUp { .. } | SelectKey::Enter => {
                    self.show(options);
                }
                SelectKey::Char(' ') => self.show(options),
                SelectKey::Home => self.show_at(Some(0), options),
                SelectKey::End => self.show_at(Some(last), options),
                SelectKey::Char(typed) => {
                    self.show(options);
                    self.typeahead(typed, now_ms, options.labels);
                }
                // Escape and Tab above all: they fall through to whatever else answers
                // them.
                _ => return Answer::default(),
            }
            return used;
        }
        match key {
            SelectKey::ArrowDown => self.active = (self.active + 1).min(last),
            SelectKey::ArrowUp { alt: true } | SelectKey::Enter => {
                return Answer {
                    used: true,
                    chosen: self.commit(self.active, options),
                };
            }
            SelectKey::ArrowUp { alt: false } => self.active = self.active.saturating_sub(1),
            SelectKey::Home => self.active = 0,
            SelectKey::End => self.active = last,
            SelectKey::PageDown => self.active = (self.active + PAGE).min(last),
            SelectKey::PageUp => self.active = self.active.saturating_sub(PAGE),
            SelectKey::Escape => self.close(),
            SelectKey::Tab => {
                return Answer {
                    used: false,
                    chosen: self.commit(self.active, options),
                };
            }
            // Mid-word a space is part of the word ("Date m..."), not a choice.
            SelectKey::Char(' ')
                if self.typed.is_empty() || now_ms - self.typed_at >= TYPEAHEAD_RESET_MS =>
            {
                return Answer {
                    used: true,
                    chosen: self.commit(self.active, options),
                };
            }
            SelectKey::Char(typed) => self.typeahead(typed, now_ms, options.labels),
            SelectKey::Other => return Answer::default(),
        }
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: [&str; 4] = ["Date taken", "Date modified", "Name", "Size"];

    /// A select with the option it holds, as a control would hold both.
    struct Held {
        select: Select,
        labels: &'static [&'static str],
        held: usize,
        disabled: bool,
        chosen: Vec<usize>,
        now: f64,
    }

    impl Held {
        fn new(held: usize) -> Self {
            Self {
                select: Select::default(),
                labels: &LABELS,
                held,
                disabled: false,
                chosen: Vec::new(),
                now: 0.0,
            }
        }

        fn options(&self) -> Options<'static> {
            Options {
                labels: self.labels,
                selected: Some(self.held),
                disabled: self.disabled,
            }
        }

        fn took(&mut self, chosen: Option<usize>) {
            if let Some(index) = chosen {
                self.chosen.push(index);
                self.held = index;
            }
        }

        /// Presses `key`, and answers whether it was used.
        fn press(&mut self, key: SelectKey) -> bool {
            let answer = self.select.key(key, self.now, &self.options());
            self.took(answer.chosen);
            answer.used
        }

        fn typed(&mut self, letter: char) -> bool {
            self.press(SelectKey::Char(letter))
        }

        fn toggle(&mut self) {
            let options = self.options();
            self.select.toggle(&options);
        }
    }

    const UP: SelectKey = SelectKey::ArrowUp { alt: false };

    #[test]
    fn a_disabled_select_does_not_open_from_a_click_or_a_key_and_opens_once_it_is_not() {
        let mut held = Held::new(2);
        held.disabled = true;
        held.toggle();
        assert!(!held.select.open());
        for key in [
            SelectKey::ArrowDown,
            UP,
            SelectKey::Enter,
            SelectKey::Char(' '),
            SelectKey::Home,
            SelectKey::End,
            SelectKey::Char('d'),
        ] {
            assert!(!held.press(key), "{key:?}");
            assert!(!held.select.open(), "{key:?}");
        }
        assert_eq!(held.select.active(), 0);
        held.disabled = false;
        held.toggle();
        assert!(held.select.open());
        assert_eq!(held.select.active(), 2);
    }

    #[test]
    fn a_closed_select_opens_on_the_held_option_from_the_keys_it_answers() {
        for key in [
            SelectKey::ArrowDown,
            UP,
            SelectKey::Enter,
            SelectKey::Char(' '),
        ] {
            let mut held = Held::new(2);
            assert!(held.press(key), "{key:?}");
            assert!(held.select.open());
            assert_eq!(held.select.active(), 2);
        }
        let mut home = Held::new(2);
        home.press(SelectKey::Home);
        assert_eq!(home.select.active(), 0);
        let mut end = Held::new(0);
        end.press(SelectKey::End);
        assert_eq!(end.select.active(), 3);
    }

    // Escape and Tab fall through to whatever else answers them, and a key that is none
    // of the list's is none of its business.
    #[test]
    fn a_closed_select_leaves_alone_the_keys_it_has_no_use_for() {
        for key in [SelectKey::Escape, SelectKey::Tab, SelectKey::Other] {
            let mut held = Held::new(0);
            assert!(!held.press(key), "{key:?}");
            assert!(!held.select.open());
        }
    }

    #[test]
    fn the_keys_move_within_the_list_without_wrapping_and_enter_chooses() {
        let mut held = Held::new(0);
        for _ in 0..5 {
            held.press(SelectKey::ArrowDown);
        }
        assert_eq!(held.select.active(), 3);
        held.press(UP);
        assert_eq!(held.select.active(), 2);
        held.press(SelectKey::Home);
        assert_eq!(held.select.active(), 0);
        held.press(UP);
        assert_eq!(held.select.active(), 0);
        held.press(SelectKey::End);
        held.press(SelectKey::PageUp);
        assert_eq!(held.select.active(), 0);
        held.press(SelectKey::PageDown);
        assert_eq!(held.select.active(), 3);
        held.press(UP);
        assert!(held.press(SelectKey::Enter));
        assert!(!held.select.open());
        assert_eq!(held.chosen, [2]);
    }

    #[test]
    fn escape_closes_without_choosing_and_tab_chooses_but_lets_the_focus_move() {
        let mut escaped = Held::new(0);
        escaped.press(SelectKey::ArrowDown);
        escaped.press(SelectKey::ArrowDown);
        assert!(escaped.press(SelectKey::Escape));
        assert!(!escaped.select.open());
        assert!(escaped.chosen.is_empty());

        let mut tabbed = Held::new(0);
        tabbed.press(SelectKey::ArrowDown);
        tabbed.press(SelectKey::ArrowDown);
        assert!(!tabbed.press(SelectKey::Tab));
        assert!(!tabbed.select.open());
        assert_eq!(tabbed.chosen, [1]);
    }

    #[test]
    fn space_chooses_and_so_does_alt_with_arrow_up() {
        let mut space = Held::new(0);
        space.press(SelectKey::ArrowDown);
        space.press(SelectKey::ArrowDown);
        space.typed(' ');
        assert_eq!(space.chosen, [1]);
        let mut alt = Held::new(0);
        alt.press(SelectKey::ArrowDown);
        alt.press(SelectKey::End);
        alt.press(SelectKey::ArrowUp { alt: true });
        assert_eq!(alt.chosen, [3]);
        assert!(!alt.select.open());
    }

    #[test]
    fn the_held_option_chosen_again_is_not_a_change() {
        let mut held = Held::new(2);
        held.press(SelectKey::Enter);
        held.press(SelectKey::Enter);
        assert!(held.chosen.is_empty());
        assert!(!held.select.open());
    }

    #[test]
    fn typing_goes_to_a_word_from_the_active_option_and_a_repeated_letter_cycles() {
        let mut held = Held::new(3);
        held.press(SelectKey::ArrowDown); // open on Size
        held.typed('d');
        assert_eq!(held.select.active(), 0);
        // The word grows on the option it is on: "da" is still Date taken, not the next
        // option that begins so.
        held.typed('a');
        assert_eq!(held.select.active(), 0);
        for letter in ['t', 'e', ' ', 'm'] {
            held.typed(letter);
        }
        assert_eq!(held.select.active(), 1);
        // The space in the middle of the word was part of the word, not a choice.
        assert!(held.select.open());
        held.now += TYPEAHEAD_RESET_MS;
        held.typed('d');
        assert_eq!(held.select.active(), 0);
        held.typed('d');
        assert_eq!(held.select.active(), 1);
        held.typed('d');
        assert_eq!(held.select.active(), 0);
        // A capital is the same letter.
        held.now += TYPEAHEAD_RESET_MS;
        held.typed('N');
        assert_eq!(held.select.active(), 2);
    }

    #[test]
    fn a_pause_starts_a_new_word() {
        let mut held = Held::new(0);
        held.press(SelectKey::ArrowDown);
        held.typed('n');
        assert_eq!(held.select.active(), 2);
        // Without the pause "ns" is no option's beginning, and nothing moves.
        held.typed('s');
        assert_eq!(held.select.active(), 2);
        held.now += TYPEAHEAD_RESET_MS;
        held.typed('s');
        assert_eq!(held.select.active(), 3);
        // And after a pause a space chooses again: the word is over.
        held.now += TYPEAHEAD_RESET_MS;
        held.typed(' ');
        assert_eq!(held.chosen, [3]);
        // A list closed ends the word too, however soon it is opened again.
        held.press(SelectKey::ArrowDown);
        held.typed('n');
        assert_eq!(held.select.active(), 2);
        held.press(SelectKey::Escape);
        held.press(SelectKey::ArrowDown);
        held.typed('d');
        assert_eq!(held.select.active(), 0);
    }

    #[test]
    fn a_letter_opens_a_closed_select_and_goes_to_its_option() {
        let mut held = Held::new(0);
        assert!(held.typed('s'));
        assert!(held.select.open());
        assert_eq!(held.select.active(), 3);
    }

    #[test]
    fn the_list_follows_the_pointer_and_the_option_clicked_is_chosen() {
        let mut held = Held::new(0);
        held.toggle();
        let options = held.options();
        held.select.hover(2, &options);
        assert_eq!(held.select.active(), 2);
        held.select.hover(40, &options);
        assert_eq!(held.select.active(), 3, "held to the list");
        let chosen = held.select.commit(3, &options);
        held.took(chosen);
        assert!(!held.select.open());
        assert_eq!(held.chosen, [3]);
        held.toggle();
        held.toggle();
        assert!(!held.select.open());
    }

    #[test]
    fn a_select_with_no_options_has_nothing_to_open_on() {
        let mut held = Held::new(0);
        held.labels = &[];
        assert!(!held.press(SelectKey::ArrowDown));
        held.toggle();
        assert!(!held.select.open());
    }
}
