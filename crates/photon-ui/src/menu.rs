//! What a context menu is: its items, where a key steps among them, and where it goes on
//! screen. `menu-keys.ts` and `menu-place.ts` in Rust, and the shape of the markup
//! `Menu.svelte` styles. `menu_view.rs` draws one; whoever opens it says what its items
//! do, by the type its picks answer with.

/// How an item that can be picked is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Plain,
    /// Under a heading, and set in from it.
    Sub,
    /// Something that cannot be taken back.
    Danger,
}

/// One line of a menu. `A` is what picking an item answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item<A> {
    Pick {
        label: String,
        /// The key that does the same, at the trailing edge.
        hint: Option<&'static str>,
        /// Whether it can be picked. One that cannot is drawn fainter, is stepped over by
        /// the keys and takes no press: it is in its place for when it will work.
        works: bool,
        look: Look,
        does: A,
    },
    Separator,
    Heading(&'static str),
    Note(&'static str),
}

impl<A> Item<A> {
    /// Whether the keys stop at this item and a press picks it.
    pub fn works(&self) -> bool {
        matches!(self, Self::Pick { works: true, .. })
    }
}

/// A key that moves through a menu's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    Down,
    Up,
    Home,
    End,
}

/// The item a key moves to, among `count` items that work. `current` is the one the keys
/// are on, or none while they are on the menu itself, as when it has just opened.
///
/// The arrows wrap, as a native menu's do: down from the last item is the first. From
/// the menu itself down is the first item and up the last, so either arrow lands
/// somewhere. None in a menu with nothing to land on.
pub fn menu_step(key: MenuKey, current: Option<usize>, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (key, current) {
        (MenuKey::Home, _) | (MenuKey::Down, None) => 0,
        (MenuKey::End, _) | (MenuKey::Up, None) => count - 1,
        (MenuKey::Down, Some(at)) => (at + 1) % count,
        (MenuKey::Up, Some(at)) => (at + count - 1) % count,
    })
}

/// The room kept between a menu and the window's edge.
pub const MENU_MARGIN: f32 = 4.0;

/// Where a menu of `size` opened at the pointer `at` goes so that all of it is in a
/// window of `window`, as its top left corner.
///
/// It opens down and to the right of the pointer, as a native menu does, and turns to the
/// other side on an axis where that would cross the window's edge - so a menu opened near
/// the bottom grows upward from the pointer and does not slide up underneath it. Only
/// when the other side does not fit either is it pushed against the margin. A menu larger
/// than the window is held to the top left margin, and scrolls.
pub fn place_menu(at: (f32, f32), size: (f32, f32), window: (f32, f32), margin: f32) -> (f32, f32) {
    let axis = |from: f32, extent: f32, room: f32| {
        let start = if from + extent + margin <= room {
            from
        } else {
            from - extent
        };
        // The margin wins where the two disagree: `clamp` would panic on a menu larger
        // than the window, whose least place is past its greatest.
        start.min(room - extent - margin).max(margin)
    };
    (axis(at.0, size.0, window.0), axis(at.1, size.1, window.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- menu-keys.test.ts ----

    #[test]
    fn a_key_moves_down_and_up_one_item() {
        assert_eq!(menu_step(MenuKey::Down, Some(0), 4), Some(1));
        assert_eq!(menu_step(MenuKey::Up, Some(2), 4), Some(1));
    }

    #[test]
    fn the_arrows_wrap_at_both_ends() {
        assert_eq!(menu_step(MenuKey::Down, Some(3), 4), Some(0));
        assert_eq!(menu_step(MenuKey::Up, Some(0), 4), Some(3));
    }

    #[test]
    fn from_the_menu_itself_down_is_the_first_item_and_up_the_last() {
        assert_eq!(menu_step(MenuKey::Down, None, 4), Some(0));
        assert_eq!(menu_step(MenuKey::Up, None, 4), Some(3));
    }

    #[test]
    fn home_and_end_go_to_either_end() {
        assert_eq!(menu_step(MenuKey::Home, Some(2), 4), Some(0));
        assert_eq!(menu_step(MenuKey::End, Some(1), 4), Some(3));
        assert_eq!(menu_step(MenuKey::Home, None, 4), Some(0));
        assert_eq!(menu_step(MenuKey::End, None, 4), Some(3));
    }

    #[test]
    fn a_key_stays_on_the_only_item() {
        assert_eq!(menu_step(MenuKey::Down, Some(0), 1), Some(0));
        assert_eq!(menu_step(MenuKey::Up, Some(0), 1), Some(0));
    }

    #[test]
    fn nothing_is_stepped_to_in_a_menu_with_no_item_to_land_on() {
        for key in [MenuKey::Down, MenuKey::Up, MenuKey::Home, MenuKey::End] {
            assert_eq!(menu_step(key, None, 0), None, "{key:?}");
        }
    }

    #[test]
    fn only_an_item_that_can_be_picked_works() {
        let pick = |works| Item::Pick {
            label: "Hide photo".to_owned(),
            hint: Some("H"),
            works,
            look: Look::Plain,
            does: (),
        };
        assert!(pick(true).works());
        assert!(!pick(false).works());
        assert!(!Item::<()>::Separator.works());
        assert!(!Item::<()>::Heading("Add to album").works());
        assert!(!Item::<()>::Note("No albums of your own yet.").works());
    }

    // ---- menu-place.test.ts ----

    const WINDOW: (f32, f32) = (1000.0, 800.0);
    const MENU: (f32, f32) = (200.0, 150.0);

    fn placed(at: (f32, f32), size: (f32, f32), window: (f32, f32)) -> (f32, f32) {
        place_menu(at, size, window, MENU_MARGIN)
    }

    #[test]
    fn a_menu_opens_down_and_to_the_right_of_the_pointer_when_it_fits() {
        assert_eq!(placed((100.0, 100.0), MENU, WINDOW), (100.0, 100.0));
    }

    #[test]
    fn it_grows_upward_from_the_pointer_near_the_bottom_edge() {
        assert_eq!(placed((100.0, 700.0), MENU, WINDOW), (100.0, 550.0));
    }

    #[test]
    fn it_opens_to_the_left_of_the_pointer_near_the_right_edge() {
        assert_eq!(placed((900.0, 100.0), MENU, WINDOW), (700.0, 100.0));
    }

    #[test]
    fn it_turns_both_ways_in_the_bottom_right_corner() {
        assert_eq!(placed((990.0, 790.0), MENU, WINDOW), (790.0, 640.0));
    }

    // Turned at a pointer that is itself inside the far margin, the menu would end
    // there with it: it is held a margin off the edge.
    #[test]
    fn a_menu_turned_at_the_very_edge_keeps_its_margin() {
        assert_eq!(placed((998.0, 799.0), MENU, WINDOW), (796.0, 646.0));
    }

    // 448 + 150 = 598 fits a window 600 tall only without the margin, so it opens upward.
    #[test]
    fn the_margin_counts_in_deciding_to_turn() {
        assert_eq!(placed((10.0, 448.0), MENU, (1000.0, 600.0)), (10.0, 298.0));
    }

    // 80 below the pointer and 120 above it: neither holds 150, so the menu turned
    // upward, which would start above the window, is pushed down to the top margin.
    #[test]
    fn it_is_pushed_against_the_margin_when_neither_side_fits() {
        assert_eq!(
            placed((10.0, 120.0), MENU, (1000.0, 200.0)),
            (10.0, MENU_MARGIN)
        );
    }

    #[test]
    fn a_menu_taller_than_the_window_is_held_to_the_top_margin() {
        assert_eq!(placed((10.0, 50.0), (200.0, 900.0), WINDOW).1, MENU_MARGIN);
        // And one wider than it to the left margin.
        assert_eq!(placed((50.0, 10.0), (1200.0, 100.0), WINDOW).0, MENU_MARGIN);
    }
}
