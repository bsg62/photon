//! What is said where there are no photos: nothing before the grid is known, why when the
//! library could not be read, and each view's own line when it is simply empty. The Svelte
//! UI's `grid-state.ts`, and the notices `Grid.svelte` wrote beside it.
//!
//! No egui here.

use crate::grid::labels::photo_count as counted;
use crate::nav::Place;
use crate::sidebar::list::Collections;
use photon_core::{grid::GridView, library::WatchedFolder};
use photon_engine::engine::NOT_BUILT;

/// The grid on hand, as far as these rules need it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridState {
    pub version: u64,
    pub len: usize,
    /// Why the grid is empty, when it is only because the library could not be read at
    /// launch (`Engine::build_first_grid`).
    pub build_error: Option<String>,
}

/// Whether the engine has built a grid at all. `Engine::open` leaves the first build to its
/// startup thread and holds an empty index at `NOT_BUILT` until it lands.
pub fn grid_built(grid: &GridState) -> bool {
    grid.version > NOT_BUILT
}

/// What is said in place of photos when the library could not be read at launch.
pub fn build_failure(grid: &GridState) -> Option<String> {
    grid.build_error
        .as_ref()
        .map(|why| format!("photon could not read the library: {why}"))
}

/// Whether the grid should say it is empty. Not before the first build: the index is empty
/// then because nothing has been read, and on a large library the line stood on screen for
/// as long as the read took. Nor when that build failed: that grid is empty because
/// nothing *could* be read.
pub fn show_empty_notice(grid: &GridState) -> bool {
    grid_built(grid) && grid.len == 0 && grid.build_error.is_none()
}

/// The status bar's count, or nothing before the first build and after a failed one, when
/// "0 photos" would be the same misreading.
pub fn photo_count(grid: &GridState) -> Option<String> {
    (grid_built(grid) && grid.build_error.is_none()).then(|| counted(grid.len))
}

/// The line an empty view shows in place of its photos. All photos and Recent have none
/// here: an empty library says why in a panel of its own. Nor has the view of a photo's
/// copies, which nothing native opens yet. An album and a person are named from the lists
/// the sidebar holds.
pub fn view_notice(grid: &GridState, place: &Place, collections: &Collections) -> Option<String> {
    if let Some(failure) = build_failure(grid) {
        return Some(failure);
    }
    if !show_empty_notice(grid) {
        return None;
    }
    match place.view {
        GridView::Starred => {
            Some("No starred photos. Star one in the viewer, or in Picasa.".to_owned())
        }
        GridView::Search => Some(format!("No photos match “{}”", place.arg)),
        GridView::Album => {
            // An album of photon's own is the user's to fill. One of Picasa's is not, and
            // neither is one deleted while it was shown.
            let own = (collections.albums.iter())
                .find(|album| album.id.to_string() == place.arg && !album.picasa);
            Some(match own {
                Some(album) => {
                    format!("“{}” is empty. Right-click a photo to add it.", album.name)
                }
                None => "This album has no photos in the library.".to_owned(),
            })
        }
        GridView::Person => {
            // A person deleted while shown has no name left to read.
            let name = (collections.people.iter())
                .find(|person| person.key == place.arg)
                .map_or("this person", |person| person.name.as_str());
            Some(format!("No photos of {name}."))
        }
        GridView::Tag => Some(format!("No photos tagged “{}”.", place.arg)),
        GridView::Duplicates => {
            Some("No duplicates. Every photo in the library is the only copy of itself.".to_owned())
        }
        GridView::Videos => Some(
            "No videos. photon finds MP4, M4V, MOV and WebM files in your watched folders."
                .to_owned(),
        ),
        GridView::Hidden => Some(
            "No hidden photos. Right-click a photo and choose Hide to put it away here.".to_owned(),
        ),
        _ => None,
    }
}

/// What the library says in place of photos while it shows none: an offer to add the
/// first folder, that a scan is looking, that the watched folders have given no photos,
/// or that every photo there is has been hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyLibrary {
    FirstRun,
    Scanning,
    NoPhotos,
    AllHidden,
}

/// Which of them applies, or none when nothing can be said yet.
///
/// Hidden photos first: with any, the library is not empty, and "photon has found no
/// photos" beside a sidebar row reading "Hidden 240" is false - Hide folder on the only
/// folder is all it takes.
///
/// Then a running scan, whatever the folder list holds: on a first run the engine watches
/// the Pictures folder by itself and scans it, and the list read at launch may be from
/// before that.
///
/// Otherwise nothing while the watched folders have not been read (`known`): until the
/// list lands, "none is watched" is not something the interface knows - said anyway, a
/// user with folders was told to add one for as long as the list took.
///
/// `scanning` is what the interface has been told, which is not the whole truth: the
/// watcher's scan of one changed directory says nothing until its 64th file or its end,
/// and a folder photon may not read is scanned and reported exactly like an empty one. So
/// `NoPhotos` is never worded as a finished search (`no_photos_line`).
pub fn empty_library(
    known: bool,
    watched: usize,
    scanning: bool,
    hidden: usize,
) -> Option<EmptyLibrary> {
    if hidden > 0 {
        Some(EmptyLibrary::AllHidden)
    } else if scanning {
        Some(EmptyLibrary::Scanning)
    } else if !known {
        None
    } else if watched == 0 {
        Some(EmptyLibrary::FirstRun)
    } else {
        Some(EmptyLibrary::NoPhotos)
    }
}

/// What is said of watched folders that have given no photos.
///
/// "Has found none", never "looked and found none": a folder photon is not allowed to
/// read is scanned and reported by the engine exactly as an empty one is. A folder on a
/// drive that is not connected is said to be out of reach rather than empty: it may hold
/// every photo the user has.
pub fn no_photos_line(watched: &[WatchedFolder]) -> String {
    let all = watched.len();
    let away = watched.iter().filter(|folder| !folder.online).count();
    match (watched, away) {
        ([only], 0) => format!(
            "photon watches {} and has found no photos or videos there.",
            only.path
        ),
        (_, 0) => {
            format!("photon watches {all} folders and has found no photos or videos in them.")
        }
        ([only], _) => format!("photon cannot reach {} right now.", only.path),
        _ if away == all => format!("photon cannot reach the {all} folders it watches right now."),
        _ => format!(
            "photon has found no photos or videos in the folders it can reach; {away} of the \
             {all} it watches cannot be reached right now."
        ),
    }
}

/// A button of the empty library's panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelButton {
    ShowHidden,
    AddFolder,
    WatchedFolders,
}

impl PanelButton {
    pub fn label(self) -> &'static str {
        match self {
            PanelButton::ShowHidden => "Show hidden photos",
            PanelButton::AddFolder => "Add folder…",
            PanelButton::WatchedFolders => "Watched folders…",
        }
    }

    /// The one the panel puts forward.
    pub fn primary(self) -> bool {
        self == PanelButton::AddFolder
    }

    /// Whether it does anything yet. Adding a folder and the list of watched folders come
    /// with a later part of the native interface: their buttons are drawn where they will
    /// be, and take no press.
    pub fn works(self) -> bool {
        self == PanelButton::ShowHidden
    }
}

/// What the library itself says where its photos would be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub title: &'static str,
    pub text: String,
    pub buttons: &'static [PanelButton],
}

/// What the panel is decided from.
#[derive(Clone, Copy, Debug)]
pub struct LibraryFacts<'a> {
    /// Whether the watched folders have been read once.
    pub known: bool,
    pub watched: &'a [WatchedFolder],
    pub scanning: bool,
    /// How many photos are hidden, and whether that has been read once: before it has,
    /// "none" is not known, and a library whose photos are all hidden said for a moment
    /// that photon had found none.
    pub hidden: Option<usize>,
}

/// The empty library's panel: in the views that are the library itself, All photos and
/// Recent, and where an empty view may say that it is empty. Every other view says why
/// *it* is empty, in one line (`view_notice`).
pub fn library_panel(grid: &GridState, place: &Place, library: &LibraryFacts<'_>) -> Option<Panel> {
    let ours = matches!(place.view, GridView::All | GridView::Recent);
    if !ours || !show_empty_notice(grid) {
        return None;
    }
    let state = empty_library(
        library.known,
        library.watched.len(),
        library.scanning,
        library.hidden?,
    )?;
    // The scanning and the nothing-found states are one block with one sentence changing:
    // a scan of a watched folder starts and ends at any time - a file changed, a drive
    // polled - and buttons that came and went with it would be gone under a press on its
    // way.
    const CHOICES: &[PanelButton] = &[PanelButton::AddFolder, PanelButton::WatchedFolders];
    Some(match state {
        EmptyLibrary::AllHidden => Panel {
            title: "Every photo is hidden",
            text: "The library has no photo to show here: all of them are in Hidden, in the \
                   sidebar."
                .to_owned(),
            buttons: &[PanelButton::ShowHidden],
        },
        EmptyLibrary::FirstRun => Panel {
            title: "No photos yet",
            text: "Choose a folder and photon shows the photos in it. Your files stay where \
                   they are: photon never moves, changes or deletes them."
                .to_owned(),
            buttons: &[PanelButton::AddFolder],
        },
        EmptyLibrary::Scanning => Panel {
            title: "No photos yet",
            text: "Looking for photos…".to_owned(),
            buttons: CHOICES,
        },
        EmptyLibrary::NoPhotos => Panel {
            title: "No photos yet",
            text: format!(
                "{} Add the folder your photos are in: photon never moves, changes or deletes \
                 them.",
                no_photos_line(library.watched)
            ),
            buttons: CHOICES,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Collections {
        Collections::default()
    }

    fn grid(version: u64, len: usize) -> GridState {
        GridState {
            version,
            len,
            build_error: None,
        }
    }

    #[test]
    fn nothing_is_said_of_a_grid_that_has_not_been_built() {
        let unbuilt = grid(NOT_BUILT, 0);
        assert!(!grid_built(&unbuilt));
        assert!(!show_empty_notice(&unbuilt));
        assert_eq!(photo_count(&unbuilt), None);
        assert_eq!(
            view_notice(&unbuilt, &Place::of(GridView::Starred), &none()),
            None
        );
    }

    #[test]
    fn a_built_grid_is_counted_and_an_empty_one_says_so() {
        assert_eq!(photo_count(&grid(3, 1234)).as_deref(), Some("1,234 photos"));
        assert_eq!(photo_count(&grid(3, 0)).as_deref(), Some("0 photos"));
        assert!(show_empty_notice(&grid(3, 0)));
        assert!(!show_empty_notice(&grid(3, 1)));
    }

    // That grid is empty because nothing could be read: "No starred photos" would be a
    // statement about the library, and "0 photos" the same.
    #[test]
    fn a_library_that_could_not_be_read_says_that_and_nothing_else() {
        let failed = GridState {
            version: 1,
            len: 0,
            build_error: Some("disk I/O error".to_owned()),
        };
        assert!(!show_empty_notice(&failed));
        assert_eq!(photo_count(&failed), None);
        for view in [GridView::All, GridView::Starred] {
            assert_eq!(
                view_notice(&failed, &Place::of(view), &none()).as_deref(),
                Some("photon could not read the library: disk I/O error")
            );
        }
    }

    #[test]
    fn an_empty_view_says_what_it_is_empty_of() {
        let empty = grid(2, 0);
        let said = |place: &Place| view_notice(&empty, place, &none()).unwrap_or_default();
        assert!(said(&Place::of(GridView::Starred)).starts_with("No starred photos."));
        assert_eq!(
            said(&Place::search("lake 2031")),
            "No photos match “lake 2031”"
        );
        assert!(said(&Place::of(GridView::Duplicates)).starts_with("No duplicates."));
        assert!(said(&Place::of(GridView::Videos)).starts_with("No videos."));
        assert!(said(&Place::of(GridView::Hidden)).starts_with("No hidden photos."));
        // An empty library has a panel of its own for these two.
        assert_eq!(
            view_notice(&empty, &Place::of(GridView::All), &none()),
            None
        );
        assert_eq!(
            view_notice(&empty, &Place::of(GridView::Recent), &none()),
            None
        );
        // And a view with photos has no line.
        assert_eq!(
            view_notice(&grid(2, 5), &Place::of(GridView::Starred), &none()),
            None
        );
    }

    #[test]
    fn an_empty_album_person_or_tag_is_named_from_the_lists() {
        use photon_core::library::{AlbumSummary, Person};
        let album = |id, name: &str, picasa| AlbumSummary {
            id,
            name: name.to_owned(),
            count: 0,
            picasa,
        };
        let lists = Collections {
            albums: vec![album(4, "Best of", false), album(9, "Scans", true)],
            people: vec![Person {
                key: "p:7".to_owned(),
                name: "Anna".to_owned(),
                count: 0,
            }],
            ..Collections::default()
        };
        let empty = grid(2, 0);
        let at = |view, arg: &str| Place {
            view,
            arg: arg.to_owned(),
        };
        let said = |place: Place| view_notice(&empty, &place, &lists).unwrap_or_default();
        assert_eq!(
            said(at(GridView::Album, "4")),
            "“Best of” is empty. Right-click a photo to add it."
        );
        // Picasa's album is not the user's to fill here, and one that is gone has no name.
        let not_ours = "This album has no photos in the library.";
        assert_eq!(said(at(GridView::Album, "9")), not_ours);
        assert_eq!(said(at(GridView::Album, "77")), not_ours);
        assert_eq!(said(at(GridView::Person, "p:7")), "No photos of Anna.");
        assert_eq!(
            said(at(GridView::Person, "p:99")),
            "No photos of this person."
        );
        assert_eq!(
            said(at(GridView::Tag, "coast")),
            "No photos tagged “coast”."
        );
        // With photos in it there is no line.
        assert_eq!(
            view_notice(&grid(2, 3), &at(GridView::Album, "4"), &lists),
            None
        );
    }

    #[test]
    fn a_library_that_watches_nothing_offers_the_first_folder() {
        assert_eq!(
            empty_library(true, 0, false, 0),
            Some(EmptyLibrary::FirstRun)
        );
    }

    // "No photos yet. Add a folder to get started" was said of a folder added a second
    // ago, while its first scan was still walking it.
    #[test]
    fn it_is_looking_while_a_scan_runs_whatever_the_list_holds() {
        use EmptyLibrary::Scanning;
        assert_eq!(empty_library(true, 1, true, 0), Some(Scanning));
        assert_eq!(empty_library(true, 3, true, 0), Some(Scanning));
        // A scan in a folder the list does not hold yet: the Pictures folder the engine
        // watches by itself. That is no time to say "add a folder".
        assert_eq!(empty_library(true, 0, true, 0), Some(Scanning));
        assert_eq!(empty_library(false, 0, true, 0), Some(Scanning));
    }

    #[test]
    fn watched_folders_with_no_scan_known_to_run_have_given_no_photos() {
        assert_eq!(
            empty_library(true, 1, false, 0),
            Some(EmptyLibrary::NoPhotos)
        );
    }

    // With no list yet, "none is watched" is not known, and a user with folders was told
    // to add one for as long as the list took.
    #[test]
    fn nothing_is_said_until_the_watched_folders_have_been_read() {
        assert_eq!(empty_library(false, 0, false, 0), None);
    }

    // Hide folder on the only folder: the library view is empty and the sidebar says
    // "Hidden 240". "photon has found no photos" beside that is false.
    #[test]
    fn hidden_photos_come_before_everything_else() {
        use EmptyLibrary::AllHidden;
        assert_eq!(empty_library(true, 1, false, 240), Some(AllHidden));
        assert_eq!(empty_library(true, 1, true, 240), Some(AllHidden));
        assert_eq!(empty_library(false, 0, false, 1), Some(AllHidden));
    }

    fn on(path: &str) -> WatchedFolder {
        WatchedFolder {
            id: 0,
            path: path.to_owned(),
            online: true,
        }
    }

    fn off(path: &str) -> WatchedFolder {
        WatchedFolder {
            online: false,
            ..on(path)
        }
    }

    // "Has found none", never "looked and found none".
    #[test]
    fn the_one_folder_is_named_and_several_are_counted() {
        assert_eq!(
            no_photos_line(&[on("/home/ada/Pictures")]),
            "photon watches /home/ada/Pictures and has found no photos or videos there."
        );
        assert_eq!(
            no_photos_line(&[on("/a"), on("/b"), on("/c")]),
            "photon watches 3 folders and has found no photos or videos in them."
        );
    }

    // An unplugged drive has not been looked in: "found no photos" would be said of a
    // folder that may hold thousands.
    #[test]
    fn a_folder_that_cannot_be_reached_is_said_to_be_that_and_not_empty() {
        assert_eq!(
            no_photos_line(&[off("/mnt/photos")]),
            "photon cannot reach /mnt/photos right now."
        );
        assert_eq!(
            no_photos_line(&[off("/a"), off("/b")]),
            "photon cannot reach the 2 folders it watches right now."
        );
        assert_eq!(
            no_photos_line(&[on("/a"), off("/b"), off("/c")]),
            "photon has found no photos or videos in the folders it can reach; 2 of the 3 it \
             watches cannot be reached right now."
        );
    }

    fn facts(watched: &[WatchedFolder], scanning: bool, hidden: usize) -> LibraryFacts<'_> {
        LibraryFacts {
            known: true,
            watched,
            scanning,
            hidden: Some(hidden),
        }
    }

    // The library's own panel, where the library itself is shown; every other view has
    // its line.
    #[test]
    fn the_panel_is_the_empty_librarys_in_all_photos_and_recent_alone() {
        let pictures = [on("/home/ada/Pictures")];
        let empty = grid(2, 0);
        for view in [GridView::All, GridView::Recent] {
            let panel = library_panel(&empty, &Place::of(view), &facts(&pictures, false, 0));
            assert_eq!(panel.unwrap().title, "No photos yet", "{view:?}");
        }
        for view in [GridView::Starred, GridView::Hidden, GridView::Videos] {
            assert_eq!(
                library_panel(&empty, &Place::of(view), &facts(&pictures, false, 0)),
                None,
                "{view:?}"
            );
        }
        // Nor over photos, nor before the grid is built, nor when it could not be read.
        let all = Place::of(GridView::All);
        assert_eq!(
            library_panel(&grid(2, 1), &all, &facts(&pictures, false, 0)),
            None
        );
        assert_eq!(
            library_panel(&grid(NOT_BUILT, 0), &all, &facts(&pictures, false, 0)),
            None
        );
        let failed = GridState {
            build_error: Some("disk I/O error".to_owned()),
            ..grid(1, 0)
        };
        assert_eq!(
            library_panel(&failed, &all, &facts(&pictures, false, 0)),
            None
        );
    }

    #[test]
    fn the_panel_says_what_the_library_is_empty_of_and_offers_what_helps() {
        use PanelButton::{AddFolder, ShowHidden, WatchedFolders};
        let (empty, all) = (grid(2, 0), Place::of(GridView::All));
        let pictures = [on("/home/ada/Pictures")];
        let panel = |facts: &LibraryFacts<'_>| library_panel(&empty, &all, facts).unwrap();

        let hidden = panel(&facts(&pictures, true, 240));
        assert_eq!(hidden.title, "Every photo is hidden");
        assert_eq!(hidden.buttons, [ShowHidden]);

        let first = panel(&facts(&[], false, 0));
        assert_eq!(first.title, "No photos yet");
        assert!(
            first
                .text
                .starts_with("Choose a folder and photon shows the photos in it.")
        );
        assert_eq!(first.buttons, [AddFolder]);

        let none = panel(&facts(&pictures, false, 0));
        assert_eq!(
            none.text,
            "photon watches /home/ada/Pictures and has found no photos or videos there. Add \
             the folder your photos are in: photon never moves, changes or deletes them."
        );
        assert_eq!(none.buttons, [AddFolder, WatchedFolders]);
    }

    // A watched folder is rescanned at any time: one block, with one sentence changing.
    #[test]
    fn looking_and_nothing_found_are_one_block_with_one_sentence_changing() {
        let (empty, all) = (grid(2, 0), Place::of(GridView::All));
        let pictures = [on("/home/ada/Pictures")];
        let looking = library_panel(&empty, &all, &facts(&pictures, true, 0)).unwrap();
        let found_none = library_panel(&empty, &all, &facts(&pictures, false, 0)).unwrap();
        assert_eq!(looking.text, "Looking for photos…");
        assert_eq!(
            (looking.title, looking.buttons),
            (found_none.title, found_none.buttons)
        );
        assert_ne!(looking.text, found_none.text);
    }

    // Before the hidden count has been read "none are hidden" is not known: a library
    // whose photos are all hidden said for a moment that photon had found none.
    #[test]
    fn nothing_is_said_before_the_hidden_photos_have_been_counted() {
        let (empty, all) = (grid(2, 0), Place::of(GridView::All));
        let pictures = [on("/home/ada/Pictures")];
        for scanning in [false, true] {
            let uncounted = LibraryFacts {
                hidden: None,
                ..facts(&pictures, scanning, 0)
            };
            assert_eq!(library_panel(&empty, &all, &uncounted), None);
        }
    }

    // Adding a folder and the list of watched folders come later: drawn, and inactive.
    #[test]
    fn the_one_button_that_works_yet_is_the_one_that_shows_the_hidden_photos() {
        use PanelButton::{AddFolder, ShowHidden, WatchedFolders};
        assert!(ShowHidden.works() && !AddFolder.works() && !WatchedFolders.works());
        assert!(AddFolder.primary() && !ShowHidden.primary() && !WatchedFolders.primary());
    }

    // The words are the Svelte grid's, until the switch-over.
    #[test]
    fn the_panel_is_worded_as_the_svelte_grid_words_it() {
        let grid_svelte = include_str!("../../../ui/src/components/Grid.svelte");
        // Markup wraps its sentences; a Windows checkout ends its lines otherwise.
        let said: String = grid_svelte.split_whitespace().collect::<Vec<_>>().join(" ");
        let (empty, all) = (grid(2, 0), Place::of(GridView::All));
        let pictures = [on("/p")];
        let panels = [
            library_panel(&empty, &all, &facts(&pictures, false, 9)).unwrap(),
            library_panel(&empty, &all, &facts(&[], false, 0)).unwrap(),
            library_panel(&empty, &all, &facts(&pictures, true, 0)).unwrap(),
        ];
        for panel in &panels {
            assert!(
                said.contains(&format!("<h2>{}</h2>", panel.title)),
                "{}",
                panel.title
            );
            assert!(said.contains(&panel.text), "{}", panel.text);
            for button in panel.buttons {
                assert!(
                    said.contains(&format!(">{}</button>", button.label())),
                    "{button:?}"
                );
            }
        }
        assert!(said.contains(
            "Add the folder your photos are in: photon never moves, changes or deletes them."
        ));
        let rules = include_str!("../../../ui/src/lib/grid-state.ts");
        for words in [
            "and has found no photos or videos there.",
            "folders and has found no photos or videos in them.",
            "right now.",
            "it watches cannot be reached right now.",
        ] {
            assert!(rules.contains(words), "{words}");
        }
    }
}
