//! The application: the engine, the shell around the grid, and the order of a frame.

use crate::{
    dialog_view::{DialogView, Done},
    dialogs::{Confirm, Dialog, Subject},
    dirs::Dirs,
    drop_card,
    empty::{self, GridState, LibraryFacts, Panel, PanelButton},
    events::{Event, UiEvents},
    export_dialog::{self, ExportDialog, Job},
    folder_drop::{self, FolderDrop, Watched},
    grid::{
        labels::folder_label,
        scroll::Notch,
        view::{Align, Ask, Asked, GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
    menu::Item,
    menu_view::{MenuView, Outcome},
    nav::{Landed, LastFolder, Nav, Place, Step},
    probe::{Facts, Move, Outside, Probe, Report},
    rename::{Commit, Of, Rename},
    scans::Scans,
    search_bar::{SearchAction, SearchBarData},
    search_box::{SearchBox, saved_search_for},
    selection::Selection,
    shell::{Action, Shell, ShellData},
    side_menu::{self, FolderFacts, SideAction},
    sidebar::{
        list::{Collections, Group, Held, List, Sources, What},
        rows::{Counts, Fixed, Today},
        view::{Named, Naming},
    },
    status::Line,
    tag_picker::{Mode, Submit, TagPicker},
    tasks::{Latest, Queue},
    theme,
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
    tile_menu::{self, Deciding, TileAction},
    toasts::{Toast, Toasts},
    viewer::{Shown, Viewer, Want},
    viewer_caption::{self, Captioned},
    viewer_levels,
    viewer_view::{Level, Message, ViewerAction, ViewerData, ViewerView},
    window_layout::{Layout, STATUS_BAR, TOP_BAR},
};
use eframe::egui;
use jiff::tz::TimeZone;
use photon_core::{
    grid::{GridEntry, GridIndex, GridView as View},
    library::{Folder, GridTile, WatchedFolder},
    media::MediaKind,
    sort::Sort,
};
use photon_engine::{
    commands::{self, FolderList, ViewerItem},
    engine::{Engine, EngineConfig},
    error::AppError,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::mpsc::Receiver, time::Duration};

/// How long a thumbnail that has not been built is waited for, as `protocol.rs`'s
/// `THUMB_TIMEOUT` bounds the same wait.
const THUMB_TIMEOUT: Duration = Duration::from_secs(30);
/// Threads decoding cached thumbnails. A decode is a tenth of a millisecond; two keep up
/// with a scroll and leave the cores to the engine's renders.
const DECODERS: usize = 2;
/// How often the engine is asked whether its first scans are started, until they are.
const LAUNCH_POLL: Duration = Duration::from_millis(50);
/// How long a closing window waits for the writes it was asked for. Longer than the five
/// seconds a write waits for the library's lock, and short enough that a folder on a
/// share that has gone does not keep the window open.
const WRITES_PATIENCE: Duration = Duration::from_secs(10);

pub struct App {
    engine: Arc<Engine>,
    events: Receiver<Event>,
    view: GridView,
    thumbs: Thumbs,
    /// The grid as last published. Read from the engine when it says the library changed,
    /// never per frame: `published` takes the engine's lock.
    index: Arc<GridIndex>,
    layout_gen: u64,
    /// The published grid's version and length, and why it is empty when it could not be
    /// read: what the rules about saying nothing yet are asked of.
    grid: GridState,
    /// The folder list and the collections as last read: what the headers are named
    /// from and the sidebar's list is built from.
    held: Held,
    folder_list: Latest<(), Option<FolderList>>,
    /// The folders photon watches, as last read. Whether that is known yet is the
    /// launch's to say (`Scans::known`): until then an empty list is "not read yet", not
    /// "no folder is watched".
    watched: Vec<WatchedFolder>,
    /// What photon is doing in the background, as the engine has reported it.
    scans: Scans,
    /// Where the user is and is going, and the queue the steps there are made on.
    nav: Nav,
    /// The photos picked in the grid on screen.
    selection: Selection,
    /// The menu that is open: one for the whole window, so opening any closes any other.
    menu: Option<Open>,
    menu_view: MenuView,
    /// The name being typed in the sidebar.
    rename: Rename,
    /// The dialog that is open: one at a time, over the whole window.
    dialog: Option<Dialog>,
    dialog_view: DialogView,
    /// The albums and saved searches whose delete has been asked for and which the
    /// lists on hand still hold, each with whether its delete has been answered. Until
    /// the lists have been read after that answer the row is one of something that is
    /// going, and has no menu (`menu_items`): an id is handed out again once it is
    /// free, and a second "Delete…" asked of the row while the first was on its way
    /// deleted whatever had been made under the number meanwhile.
    deleting: Vec<(Subject, bool)>,
    /// The viewer, while it is open: the shell is not drawn then.
    viewer: Option<Viewing>,
    viewer_view: ViewerView,
    /// The previews the viewer shows until a photo's full picture is made: the cache's
    /// larger thumbnails, held apart from the grid's.
    previews: Thumbs,
    /// What the library says of the photo in the viewer, asked at every step and at
    /// every change of the library: with the photo it was asked of.
    viewer_items: Latest<i64, (i64, Result<ViewerItem, AppError>)>,
    /// The full picture, rendered by the engine and halved into its levels off this
    /// thread. One question at a time, and the latest wins: a decode cannot be taken
    /// back, and the photo stopped on must not wait behind those passed.
    pictures: Latest<PictureAsk, (PictureAsk, Made)>,
    /// The picture being made: asked for, and not answered yet. Kept beside the task
    /// and not with the viewer, which can close and open again on the same photo while
    /// its picture is on its way.
    asking: Option<PictureAsk>,
    /// How many pictures have been asked for: each is a whole decode, and what a test
    /// of "once" counts.
    asked_pictures: u64,
    /// Whether a folder dropped on the window can arrive at all: asked of the window
    /// once (`folder_drop::can_arrive`). Where none can, none is offered.
    drops: bool,
    /// What is held over the window.
    drop: FolderDrop,
    /// How many paths the system says are held over the window, and those it has
    /// dropped on it since a frame last read them: taken out of each frame's input
    /// before egui reads it (`raw_input_hook`).
    hovered: usize,
    fell: Vec<String>,
    /// What a notch of the wheel moves, and who says: the system, which is asked when
    /// the window is made and whenever it comes back to the front (`system_wheel`).
    wheel: fn() -> Notch,
    notch: Notch,
    /// The path in the export's field, asked about off this thread: with the text that
    /// was asked about, since the field has been typed in since.
    checking: Latest<String, (String, Result<String, String>)>,
    /// Whether an export applies edits, as stored: read when the export's dialog opens.
    edits: Latest<(), Option<bool>>,
    /// The export, on a thread of its own: it runs for minutes, and a star pressed
    /// meanwhile must not wait for it. Answers how many photos were asked for, where
    /// to, and what came of it.
    exports: Queue<Job, (usize, String, Result<commands::ExportReport, String>)>,
    /// Whether an export is running: one at a time.
    exporting: bool,
    /// A step made answers where it led, and with it what the engine said when it made
    /// the step and could not finish the rest of it (a sort shown and not stored).
    steps: Queue<Step, Result<(Landed, Option<String>), String>>,
    /// The search box's text and the send that typing has made due.
    search: SearchBox,
    /// What the user has asked to be written to the library, made in the order asked and
    /// off this thread.
    writes: Queue<Write, Result<Wrote, Unwritten>>,
    /// Whether the engine has published a grid that is not on screen yet. It waits while
    /// a step is on its way and no answer says what it shows (`Nav::adopt`).
    stale: bool,
    /// What the library holds of each kind, for the sidebar. Asked for by the layout
    /// generation held, so that the answer need not carry the layout.
    counts: Counts,
    /// Whether they have been read for the library as it is: before that "no photo is
    /// hidden" is not known. Not at launch, and not when a grid that held photos comes
    /// back empty - the count on hand is then of the library before, and every photo
    /// hidden at once (Picasa's flags, a folder hidden) said for a round trip that photon
    /// had found none.
    counted: bool,
    counting: Latest<u64, Counts>,
    collecting: Latest<(), Option<Collections>>,
    /// The sidebar's entries, built again only when what they are built from has moved.
    list: List,
    /// The folder the sidebar's list was last drawn marking.
    marked: Option<i64>,
    /// A folder to put at the top of the grid in the next frame drawn, looked up in the
    /// grid that frame draws.
    going: Option<i64>,
    /// A photo to bring into sight in the next frame drawn, looked up in the grid that
    /// frame draws: the one the viewer closed on.
    showing: Option<i64>,
    /// The folder at the top of All photos, as photon remembers it.
    last_folder: LastFolder,
    /// Stores the folder remembered, off this thread, the latest alone.
    remembering: Latest<i64, ()>,
    today: Today,
    shell: Shell,
    layout: Layout,
    /// Writes the layout to its file, off this thread, the latest alone.
    layout_store: Latest<Layout, ()>,
    toasts: Toasts,
    size: GridTile,
    zone: TimeZone,
    visible: VisibleReport,
    last: Option<GridOutput>,
    /// The gate's scroll programme, when this run is a measurement.
    probe: Option<ProbeRun>,
    /// The GPU the window is drawn with, for the log and the gate's report.
    adapter: Option<String>,
    closed: bool,
}

/// A menu that is open: where it was opened, and whose it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Open {
    at: egui::Pos2,
    of: MenuOf,
}

/// What a menu is the menu of. Its items are built from this in every frame, from the
/// selection and the lists as they are then - so a menu whose album, saved search or
/// folder is no longer listed has no items, and closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuOf {
    /// The photos selected: the tile's menu.
    Selection,
    Album(i64),
    Search(i64),
    /// A folder: on its row in the sidebar, and on its header in the grid.
    Folder(i64),
}

/// What an item of whichever menu is open does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Picked {
    Tile(TileAction),
    Side(SideAction),
}

/// Something to write to the library.
enum Write {
    /// The search in the box, under a name: the bookmark.
    SaveSearch {
        name: String,
        query: String,
    },
    /// The size of the tiles, as the size control was last pressed.
    TileSize(GridTile),
    /// Stars set on photos, or taken off: a `star=` line for each in its folder's
    /// `.picasa.ini`, written by the engine (`commands::set_stars`). The one write
    /// photon makes outside its own library.
    Stars {
        ids: Vec<i64>,
        starred: bool,
    },
    Hidden {
        ids: Vec<i64>,
        hidden: bool,
    },
    AlbumAdd {
        album: i64,
        ids: Vec<i64>,
    },
    AlbumRemove {
        album: i64,
        ids: Vec<i64>,
    },
    /// "Not this person", for each of the photos. The name is read before the write,
    /// while the list surely still holds it: it is what the message afterwards says.
    PersonRemove {
        person: i64,
        name: String,
        ids: Vec<i64>,
    },
    /// A name typed in the sidebar, and the number its field was sent under
    /// (`rename::Commit`): a new album's, an album's, a saved search's.
    AlbumNew {
        name: String,
        sent: u64,
    },
    AlbumRename {
        album: i64,
        name: String,
        sent: u64,
    },
    SearchRename {
        search: i64,
        name: String,
        sent: u64,
    },
    /// A folder's name in photon - the directory keeps its own - or none, which shows
    /// the directory's again. `sent` when it was typed in a field, which waits for it.
    FolderAlias {
        folder: i64,
        name: Option<String>,
        sent: Option<u64>,
    },
    /// A folder hidden - its photos, and any added to it later - or brought back.
    FolderHidden {
        folder: i64,
        hidden: bool,
    },
    /// A watched folder scanned again.
    Rescan {
        watched: i64,
    },
    /// An album of the user's own deleted - its photos stay - or a saved search.
    Delete(Subject),
    /// A keyword added to photos, or taken off them, in the library: keywords are read
    /// from the photos and never written to them.
    Tag(Submit),
    /// The paths of one drop, each offered to the engine as a folder to watch: in
    /// order, the next after the one before has been answered, since two folders of
    /// one drop can overlap and the engine can refuse the second only once it has
    /// finished the first.
    Watch {
        paths: Vec<String>,
    },
    /// The export's checkbox, stored as it is ticked; `was` is what it goes back to if
    /// the library will not store it.
    ExportApplyEdits {
        apply: bool,
        was: bool,
    },
}

/// A write that was made.
enum Wrote {
    Search,
    TileSize,
    /// How many stars were written, of how many asked.
    Stars {
        asked: usize,
        done: usize,
        starred: bool,
    },
    /// A write whose rebuild says everything there is to say: a hide.
    Grid,
    /// A write the lists are read again for: the engine announces a change of the
    /// albums only while an album is on screen (`Engine::albums_changed`), and the
    /// counts in the sidebar are wanted in every view.
    Lists,
    Removed {
        name: String,
        removed: usize,
        kept: usize,
        asked: usize,
    },
    /// A name stored that the sidebar's lists show: its field closes, and they are read
    /// again.
    Named {
        sent: u64,
    },
    /// Something of a folder stored - its name, whether it is hidden: the folder list
    /// is read again, and the field that sent a name, if one did, closes.
    Folder {
        sent: Option<u64>,
    },
    /// An album or a saved search deleted: the lists are read again, and a grid that
    /// showed the album is left.
    Deleted(Subject),
    /// A keyword written: which way, under which name the library stored it, and how
    /// many of the photos asked for it reached.
    Tagged {
        mode: Mode,
        stored: String,
        done: usize,
        asked: usize,
    },
    /// What came of a drop: the folders now watched, and the engine's words for each
    /// path it refused.
    Watched {
        folders: Vec<WatchedFolder>,
        refused: Vec<String>,
    },
    /// A setting stored that nothing on screen is read from.
    Stored,
}

/// A write the library refused, and what it said.
enum Unwritten {
    Search {
        query: String,
        said: String,
    },
    TileSize {
        said: String,
    },
    Said(String),
    /// A delete the library refused: what was to go is still there.
    Delete {
        of: Subject,
        said: String,
    },
    /// A name the library would not take: its field stays open, to be put right.
    Name {
        sent: u64,
        said: String,
    },
    /// The export's checkbox, which goes back to what is stored.
    Edits {
        was: bool,
        said: String,
    },
}

/// The gate's programme, where its report goes, and what the report says besides.
struct ProbeRun {
    probe: Probe,
    outside: Outside,
    out: PathBuf,
}

/// The report as it is written: the programme's, and what the application knows about
/// the run it was measured in.
#[derive(serde::Serialize)]
struct Written<'a> {
    app: &'static str,
    /// The window's size in points, and how many device pixels a point is.
    window: [f32; 2],
    scale: f32,
    adapter: Option<&'a str>,
    /// photon's own memory at the end of the run (`commands::memory_usage`).
    memory_bytes: Option<u64>,
    #[serde(flatten)]
    report: &'a Report,
}

impl App {
    /// Opens the library in `dirs` and starts the engine. `pictures` is the folder a
    /// library that watches nothing starts by watching (`Engine::startup`).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dirs: Dirs,
        pictures: Option<PathBuf>,
    ) -> Result<Self, String> {
        let ctx = cc.egui_ctx.clone();
        let drops = {
            use raw_window_handle::HasWindowHandle;
            folder_drop::can_arrive(cc.window_handle().map(|window| window.as_raw()))
        };
        theme::apply::install(&ctx);
        // A notch of the wheel as far as the web view of this system moved it: said to
        // egui in every frame (`ui`), a page being a page of what is in view then.
        let notch = system_wheel();
        icons::install(&ctx);
        theme::fonts::install(&ctx);

        let (events, receiver) = UiEvents::new(ctx.clone());
        // How the window was left, here and not by a task, for the reason the theme is
        // read here: read after the first frame, the sidebar would open at the default
        // width and jump.
        let layout = Layout::load(&dirs.layout_path);
        let layout_path = dirs.layout_path;
        let config = EngineConfig {
            db_path: dirs.db_path,
            cache_dir: dirs.cache_dir,
            workers: photon_core::thumbs::default_workers(),
        };
        let engine = Engine::open(config, Arc::new(events))
            .map_err(|err| format!("could not open the photon library: {err}"))?;
        // The theme and the tile size, here and not by a task: this is before the first
        // frame, where a read is allowed and its answer is wanted. Read after it, a theme
        // pinned against the desktop's showed the desktop's for a frame at every launch -
        // what `app.rs` in the Tauri shell reads the theme in `setup` to avoid - and the
        // grid laid itself out twice. `Engine::open` has just read two settings from the
        // same table on this thread. A read that fails leaves the defaults.
        theme::apply::choose(&ctx, commands::theme(&engine).unwrap_or_default());
        let size = commands::grid_tile(&engine).unwrap_or_default();
        engine.startup(pictures);

        let repaint = |ctx: &egui::Context| {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        let loader = Loader::spawn(
            Arc::new(EngineThumbs::grid(engine.clone())),
            DECODERS,
            THUMB_TIMEOUT,
            repaint(&ctx),
        );
        let mut folder_list = Latest::spawn(
            "folders",
            {
                let engine = engine.clone();
                move |()| commands::list_folders(&engine).ok()
            },
            repaint(&ctx),
        );
        folder_list.ask(());

        // A step that moves the engine's view rebuilds the grid where it is made: here,
        // one at a time and in the order asked.
        let steps = Queue::spawn(
            "views",
            {
                let engine = engine.clone();
                move |step: Step| {
                    let mut said = None;
                    let shown_at = match step {
                        Step::View(view) => commands::set_grid_view(&engine, view),
                        Step::Search(query) => commands::set_search_query(&engine, &query),
                        Step::Album(id) => commands::set_album_view(&engine, id),
                        Step::Person(key) => commands::set_person_view(&engine, &key),
                        Step::Tag(tag) => commands::set_tag_view(&engine, &tag),
                        // The one setter whose error does not mean "nothing moved": the
                        // engine sorts first and stores second (`Engine::set_sort`), so
                        // a sort it could not store is published all the same. That is
                        // a step made, with something to say - answered as refused, the
                        // photos were in one order and the controls said the other, for
                        // the rest of the session. Its grid is whichever is published.
                        Step::Sort(sort) => commands::set_sort(&engine, sort).or_else(|err| {
                            if engine.sort() == sort {
                                said = Some(err.message);
                                Ok(None)
                            } else {
                                Err(err)
                            }
                        }),
                    }
                    .map_err(|err| err.message)?;
                    // Where the step led, read here: after it is made and before the next
                    // one begins, which is the only time the engine's view is known to be
                    // this step's. The interface must not read it when a grid arrives.
                    let (view, arg) = engine.view_and_arg();
                    let landed = Landed {
                        place: Place { view, arg },
                        sort: engine.sort(),
                        shown_at,
                    };
                    Ok((landed, said))
                }
            },
            repaint(&ctx),
        );
        let writes = Queue::spawn(
            "writes",
            {
                let engine = engine.clone();
                move |write: Write| match write {
                    Write::SaveSearch { name, query } => {
                        match commands::save_search(&engine, &name, &query) {
                            Ok(_) => Ok(Wrote::Search),
                            Err(err) => Err(Unwritten::Search {
                                query,
                                said: err.message,
                            }),
                        }
                    }
                    Write::TileSize(size) => match commands::set_grid_tile(&engine, size) {
                        Ok(()) => Ok(Wrote::TileSize),
                        Err(err) => Err(Unwritten::TileSize { said: err.message }),
                    },
                    Write::Stars { ids, starred } => commands::set_stars(&engine, &ids, starred)
                        .map(|done| Wrote::Stars {
                            asked: ids.len(),
                            done,
                            starred,
                        })
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::Hidden { ids, hidden } => {
                        commands::set_items_hidden(&engine, &ids, hidden)
                            .map(|_| Wrote::Grid)
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                    Write::AlbumAdd { album, ids } => commands::add_to_album(&engine, album, &ids)
                        .map(|()| Wrote::Lists)
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::AlbumRemove { album, ids } => {
                        commands::remove_from_album(&engine, album, &ids)
                            .map(|()| Wrote::Lists)
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                    Write::PersonRemove { person, name, ids } => {
                        commands::remove_from_person(&engine, person, &ids)
                            .map(|came| Wrote::Removed {
                                name,
                                removed: usize::try_from(came.removed).unwrap_or(0),
                                kept: usize::try_from(came.kept_by_picasa).unwrap_or(0),
                                asked: ids.len(),
                            })
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                    Write::AlbumNew { name, sent } => commands::create_album(&engine, &name)
                        .map(|_| Wrote::Named { sent })
                        .map_err(|err| Unwritten::Name {
                            sent,
                            said: err.message,
                        }),
                    Write::AlbumRename { album, name, sent } => {
                        commands::rename_album(&engine, album, &name)
                            .map(|()| Wrote::Named { sent })
                            .map_err(|err| Unwritten::Name {
                                sent,
                                said: err.message,
                            })
                    }
                    Write::SearchRename { search, name, sent } => {
                        commands::rename_saved_search(&engine, search, &name)
                            .map(|()| Wrote::Named { sent })
                            .map_err(|err| Unwritten::Name {
                                sent,
                                said: err.message,
                            })
                    }
                    Write::FolderAlias { folder, name, sent } => {
                        let stored = commands::set_folder_alias(&engine, folder, name);
                        match (stored, sent) {
                            (Ok(_), sent) => Ok(Wrote::Folder { sent }),
                            (Err(err), Some(sent)) => Err(Unwritten::Name {
                                sent,
                                said: err.message,
                            }),
                            (Err(err), None) => Err(Unwritten::Said(err.message)),
                        }
                    }
                    Write::FolderHidden { folder, hidden } => {
                        commands::set_folder_hidden(&engine, folder, hidden)
                            .map(|_| Wrote::Folder { sent: None })
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                    Write::Rescan { watched } => commands::rescan_folder(&engine, watched)
                        .map(|()| Wrote::Grid)
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::Delete(of) => {
                        let deleted = match of {
                            Subject::Album(album) => commands::delete_album(&engine, album),
                            Subject::Search(search) => {
                                commands::delete_saved_search(&engine, search)
                            }
                        };
                        deleted
                            .map(|()| Wrote::Deleted(of))
                            .map_err(|err| Unwritten::Delete {
                                of,
                                said: err.message,
                            })
                    }
                    Write::Watch { paths } => {
                        let (mut folders, mut refused) = (Vec::new(), Vec::new());
                        for path in paths {
                            match commands::add_folder(&engine, &path) {
                                Ok(folder) => folders.push(folder),
                                Err(err) => refused.push(err.message),
                            }
                        }
                        Ok(Wrote::Watched { folders, refused })
                    }
                    Write::ExportApplyEdits { apply, was } => {
                        commands::set_export_apply_edits(&engine, apply)
                            .map(|()| Wrote::Stored)
                            .map_err(|err| Unwritten::Edits {
                                was,
                                said: err.message,
                            })
                    }
                    Write::Tag(Submit { mode, tag, ids }) => {
                        let written = match mode {
                            Mode::Add => commands::add_items_tag(&engine, &ids, &tag),
                            Mode::Remove => commands::remove_items_tag(&engine, &ids, &tag),
                        };
                        written
                            .map(|written| Wrote::Tagged {
                                mode,
                                stored: written.tag,
                                done: written.count,
                                asked: ids.len(),
                            })
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                }
            },
            repaint(&ctx),
        );
        let mut counting = Latest::spawn(
            "counts",
            {
                let engine = engine.clone();
                move |layout_gen: u64| {
                    let info = commands::grid_info(&engine, Some(layout_gen));
                    Counts {
                        starred: info.starred_count,
                        duplicates: info.duplicate_count,
                        hidden: info.hidden_count,
                        videos: info.video_count,
                        copies_of: info.copies_of.map(|photo| photo.file_name),
                    }
                }
            },
            repaint(&ctx),
        );
        // The albums, saved searches, people and tags: read together, since they change
        // together - at a data change - and the tag counts alone are 220 ms in a library
        // of 300,000 photos. Not asked for here: the first grid the engine publishes is
        // a data change, built or failed, and asking here as well read them twice at
        // every launch.
        let collecting = Latest::spawn(
            "collections",
            {
                let engine = engine.clone();
                move |()| {
                    // All of them or none: a read that failed leaves the lists as they
                    // were, and the next change to the library reads them again.
                    let read = || -> Result<Collections, AppError> {
                        Ok(Collections {
                            albums: commands::list_albums(&engine)?,
                            searches: commands::list_saved_searches(&engine)?,
                            people: commands::list_people(&engine)?,
                            to_name: usize::try_from(commands::people_to_name(&engine)?)
                                .unwrap_or(0),
                            tags: commands::list_tags(&engine)?,
                        })
                    };
                    read()
                        .inspect_err(|err| {
                            tracing::warn!(err = %err.message, "the sidebar's lists were not read");
                        })
                        .ok()
                }
            },
            repaint(&ctx),
        );
        // The path typed in the export's field: whether it is a folder, and one the
        // engine will write into. Asked as it is typed, so only the latest counts.
        let checking = Latest::spawn(
            "export destination",
            {
                let engine = engine.clone();
                let home = ::dirs::home_dir();
                move |text: String| {
                    let answer = check_dest(&engine, &text, home.as_deref());
                    (text, answer)
                }
            },
            repaint(&ctx),
        );
        let preview_loader = Loader::spawn(
            Arc::new(EngineThumbs::preview(engine.clone())),
            DECODERS,
            THUMB_TIMEOUT,
            repaint(&ctx),
        );
        let viewer_items = Latest::spawn(
            "viewer item",
            {
                let engine = engine.clone();
                move |id: i64| (id, commands::viewer_item(&engine, id))
            },
            repaint(&ctx),
        );
        let pictures = Latest::spawn(
            "viewer picture",
            {
                let engine = engine.clone();
                move |asked: PictureAsk| (asked, make_picture(&engine, asked))
            },
            repaint(&ctx),
        );
        let edits = Latest::spawn(
            "export edits",
            {
                let engine = engine.clone();
                move |()| commands::export_apply_edits(&engine).ok()
            },
            repaint(&ctx),
        );
        let exports = Queue::spawn(
            "export",
            {
                let engine = engine.clone();
                move |job: Job| {
                    let done = commands::export_items(
                        &engine,
                        &job.ids,
                        &job.dest,
                        job.apply_edits,
                        job.max_edge,
                    );
                    (job.ids.len(), job.dest, done.map_err(|err| err.message))
                }
            },
            repaint(&ctx),
        );
        // The folder to come back to, here and not by a task, as the theme is: it is
        // wanted with the first grid, and it is one row of the settings table.
        let last_folder = LastFolder::new(commands::last_folder(&engine).ok().flatten());
        let remembering = Latest::spawn(
            "last folder",
            {
                let engine = engine.clone();
                move |folder: i64| {
                    if let Err(err) = commands::set_last_folder(&engine, folder) {
                        tracing::warn!(err = %err.message, "the folder shown was not remembered");
                    }
                }
            },
            || {},
        );
        // Nothing is drawn from this, so nothing is asked to be drawn for it.
        let layout_store = Latest::spawn(
            "layout",
            move |layout: Layout| {
                if let Err(err) = layout.save(&layout_path) {
                    tracing::warn!(%err, path = %layout_path.display(), "the window's layout was not stored");
                }
            },
            || {},
        );

        let (version, index, build_error, layout_gen) = engine.published();
        counting.ask(layout_gen);
        let (view, arg) = engine.view_and_arg();
        let zone = TimeZone::system();
        Ok(Self {
            engine: engine.clone(),
            events: receiver,
            view: GridView::default(),
            thumbs: Thumbs::new(loader),
            grid: GridState {
                version,
                len: index.len(),
                build_error,
            },
            index,
            layout_gen,
            held: Held::default(),
            folder_list,
            watched: Vec::new(),
            scans: Scans::default(),
            nav: Nav::new(Place { view, arg }, engine.sort()),
            selection: Selection::default(),
            menu: None,
            menu_view: MenuView::default(),
            rename: Rename::default(),
            dialog: None,
            dialog_view: DialogView::default(),
            deleting: Vec::new(),
            viewer: None,
            viewer_view: ViewerView::default(),
            previews: Thumbs::previews(preview_loader),
            viewer_items,
            pictures,
            asking: None,
            asked_pictures: 0,
            drops,
            drop: FolderDrop::default(),
            hovered: 0,
            fell: Vec::new(),
            wheel: system_wheel,
            notch,
            checking,
            edits,
            exports,
            exporting: false,
            steps,
            search: SearchBox::default(),
            writes,
            stale: false,
            counts: Counts::default(),
            counted: false,
            counting,
            collecting,
            list: List::default(),
            marked: None,
            going: None,
            showing: None,
            last_folder,
            remembering,
            today: today(&zone),
            shell: Shell::default(),
            layout,
            layout_store,
            toasts: Toasts::default(),
            size,
            zone,
            visible: VisibleReport::default(),
            last: None,
            probe: None,
            adapter: cc
                .wgpu_render_state
                .as_ref()
                .map(|state| format!("{:?}", state.adapter.get_info())),
            closed: false,
        })
    }

    /// With `wheel` to say what a notch of the wheel moves, where the system says it:
    /// for a test, which may not change a setting of the machine it runs on.
    pub fn with_wheel(mut self, wheel: fn() -> Notch) -> Self {
        self.wheel = wheel;
        self.notch = wheel();
        self
    }

    /// As in a window that takes no drop: for a test, which has no window to be one.
    pub fn without_drops(mut self) -> Self {
        self.drops = false;
        self
    }

    /// Makes this run a measurement: the gate's scroll programme drives the grid from the
    /// first frame, writes its report to `out` and closes the window.
    /// `started_epoch_ms` is when the harness started the process, for the launch time.
    pub fn with_probe(mut self, out: PathBuf, started_epoch_ms: Option<f64>) -> Self {
        self.last_folder = LastFolder::off();
        self.probe = Some(ProbeRun {
            probe: Probe::new(started_epoch_ms),
            outside: Outside::default(),
            out,
        });
        self
    }

    /// Makes this run the launch the gate throws away before the one it measures: it
    /// ends, with a report nobody reads, as soon as the grid shows its pictures.
    pub fn with_warm_up(mut self, out: PathBuf, started_epoch_ms: Option<f64>) -> Self {
        self.last_folder = LastFolder::off();
        self.probe = Some(ProbeRun {
            probe: Probe::warm_up(started_epoch_ms),
            outside: Outside::default(),
            out,
        });
        self
    }

    /// The engine under the interface.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// The GPU the window is drawn with, as wgpu describes it.
    pub fn adapter(&self) -> Option<&str> {
        self.adapter.as_deref()
    }

    /// How many photos the grid holds.
    pub fn photos(&self) -> usize {
        self.index.len()
    }

    /// What the last frame of the grid came to.
    pub fn last_frame(&self) -> Option<&GridOutput> {
        self.last.as_ref()
    }

    /// The size the grid draws its tiles at.
    pub fn tile_size(&self) -> GridTile {
        self.size
    }

    /// The sort the user is going to: what the controls in the top bar show.
    pub fn sort(&self) -> Sort {
        self.nav.sort_target()
    }

    /// The sort of the grid on screen.
    pub fn settled_sort(&self) -> Sort {
        self.nav.sort()
    }

    /// The folders the headers are named from.
    pub fn folders(&self) -> &HashMap<i64, Folder> {
        self.held.folders()
    }

    /// Whether a step is on its way: asked of the engine, or made and its grid not yet
    /// on screen. What is on screen may be the grid of a step before it.
    pub fn busy(&self) -> bool {
        self.nav.busy()
    }

    /// Where the user is: the place the last step asked leads to, or the one shown.
    pub fn place(&self) -> Place {
        self.nav.target()
    }

    /// The place the published grid shows.
    pub fn settled(&self) -> &Place {
        self.nav.settled()
    }

    /// What the library holds of each kind, as the sidebar last learnt it.
    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// The albums, saved searches, people and tags, as last read.
    pub fn collections(&self) -> &Collections {
        self.held.collections()
    }

    /// What the search box holds.
    pub fn search_text(&self) -> &str {
        &self.search.text
    }

    /// Whether the panel that lists what the search box understands is open.
    pub fn search_help_open(&self) -> bool {
        self.shell.search_help_open()
    }

    /// The folder the sidebar's list was last drawn marking.
    pub fn marked(&self) -> Option<i64> {
        self.marked
    }

    /// Where the sidebar's list is.
    pub fn sidebar_position(&self) -> f64 {
        self.shell.sidebar_position()
    }

    /// How the window is laid out.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The messages showing.
    pub fn toasts(&self) -> &[Toast] {
        self.toasts.held()
    }

    /// The version of the grid on screen. Every grid the engine publishes has another.
    pub fn version(&self) -> u64 {
        self.grid.version
    }

    /// The photos selected, in the grid's order.
    pub fn selected(&self) -> Vec<i64> {
        self.selection.ids(&self.index)
    }

    /// The photo the keyboard moves on from.
    pub fn lead(&self) -> Option<i64> {
        self.selection.lead().map(|lead| lead.id)
    }

    /// Where the photo at grid offset `offset` was drawn in the last frame: what a test
    /// presses.
    pub fn tile_rect(&self, offset: usize) -> Option<egui::Rect> {
        self.view.rect_of(offset)
    }

    /// The photos of the grid on screen, in its order.
    pub fn shown(&self) -> Vec<i64> {
        (self.index.rows(0, self.index.len()).iter())
            .map(|entry| entry.id)
            .collect()
    }

    /// Those of them that are starred.
    pub fn starred(&self) -> Vec<i64> {
        (self.index.rows(0, self.index.len()).iter())
            .filter(|entry| entry.starred)
            .map(|entry| entry.id)
            .collect()
    }

    /// Whether a menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The name being typed in the sidebar: what for, what it holds, and whether it is
    /// being stored.
    pub fn naming(&self) -> Option<(Of, String, bool)> {
        let (of, text) = self.rename.field()?;
        Some((of.clone(), text.to_owned(), self.rename.busy()))
    }

    /// The items of the menu that is open, as things are in this frame - or none, when
    /// what it is the menu of is no longer there: nothing selected, an album deleted
    /// from elsewhere, a folder the library no longer holds.
    fn menu_items(&self, of: MenuOf) -> Option<Vec<Item<Picked>>> {
        let collections = self.held.collections();
        let side = |items: Vec<Item<SideAction>>| {
            (items.into_iter())
                .map(|item| item.map(Picked::Side))
                .collect()
        };
        match of {
            MenuOf::Selection => (!self.selection.is_empty()).then(|| {
                (self.tile_items().into_iter())
                    .map(|item| item.map(Picked::Tile))
                    .collect()
            }),
            // Not of one that is being deleted: its row is still listed until the lists
            // are read again, and is no longer a row of anything.
            MenuOf::Album(id) => (collections.albums.iter())
                .any(|album| album.id == id && !self.going(Subject::Album(id)))
                .then(|| side(side_menu::album_menu(id))),
            MenuOf::Search(id) => (collections.searches.iter())
                .any(|search| search.id == id && !self.going(Subject::Search(id)))
                .then(|| side(side_menu::search_menu(id))),
            // A folder the view no longer holds has no row and no header: its name is
            // typed in its row, and its menu was opened on one of the two.
            MenuOf::Folder(id) => (self.held.folders().get(&id))
                .filter(|_| self.list.folder(id).is_some())
                .map(|folder| {
                    side(side_menu::folder_menu(&FolderFacts {
                        id,
                        watched: folder.watched_id,
                        named: folder.alias.is_some(),
                        hidden: folder.hidden,
                        scanning: self.scans.is_scanning(folder.watched_id),
                    }))
                }),
        }
    }

    /// The viewer, while it is open.
    pub fn viewer(&self) -> Option<&Viewer> {
        self.viewer.as_ref().map(|open| &open.viewer)
    }

    /// What the library last said of the photo in the viewer.
    pub fn viewer_item(&self) -> Option<&ViewerItem> {
        self.viewer.as_ref()?.item.as_ref()
    }

    /// The widths of the levels of the full picture on screen, largest first: none
    /// until it has been made for the photo the viewer is on.
    pub fn viewer_levels(&self) -> Option<Vec<u32>> {
        let full = self.viewer.as_ref()?.picture()?;
        Some(full.levels.iter().map(|(_, width)| *width).collect())
    }

    /// Whether the full picture of the photo the viewer is on is being made: asked for,
    /// and not answered yet.
    pub fn viewer_asked(&self) -> bool {
        (self.viewer.as_ref()).is_some_and(|open| {
            self.asking
                .is_some_and(|asked| asked.want == open.viewer.want())
        })
    }

    /// How many full pictures have been asked for since the window was made.
    pub fn pictures_asked(&self) -> u64 {
        self.asked_pictures
    }

    /// Why the full picture of the photo the viewer is on could not be made, if it
    /// could not.
    pub fn viewer_failed(&self) -> Option<&str> {
        let open = self.viewer.as_ref()?;
        (open.failed.as_ref())
            .filter(|(of, _)| *of == open.viewer.want())
            .map(|(_, why)| why.as_str())
    }

    /// The size, in pixels, of the copy of the full picture that was scaled to fit the
    /// window: none until it is made, or for a photo no larger than the window.
    pub fn viewer_fitted(&self) -> Option<[usize; 2]> {
        let full = self.viewer.as_ref()?.picture()?;
        full.fitted.as_ref().map(|fitted| fitted.size())
    }

    /// The photo whose full picture is held as textures, whether or not it is the one
    /// the viewer is on.
    pub fn viewer_holds(&self) -> Option<i64> {
        Some(self.viewer.as_ref()?.full.as_ref()?.want.id)
    }

    /// Whether the preview of the photo in the viewer is a texture.
    pub fn viewer_preview(&mut self) -> bool {
        let Some(key) = self.viewer.as_ref().map(|open| open.viewer.key()) else {
            return false;
        };
        self.previews.texture(key).is_some()
    }

    /// Bytes of preview held as textures.
    pub fn preview_bytes(&self) -> usize {
        self.previews.bytes()
    }

    /// Whether the preview of the photo `id` of the grid on screen is a texture.
    pub fn preview_of(&self, id: i64) -> bool {
        (self.entry(id)).is_some_and(|entry| self.previews.holds(entry.thumb_key))
    }

    /// The dialog that is open.
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    /// Whether an export is running.
    pub fn exporting(&self) -> bool {
        self.exporting
    }

    /// Whether something with a path is held over the window.
    pub fn hovering(&self) -> bool {
        self.drop.hovering()
    }

    /// Opens `dialog`, over everything. It is opened from a menu's item, and the pick
    /// has closed that menu; the press that opened the menu stored any name half typed
    /// in the sidebar. A way in that is neither closes the one and stores the other
    /// itself.
    fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.dialog_view.reset();
    }

    /// Opens the menu of `of` at `at`, in the place of any that was open.
    fn open_menu(&mut self, ctx: &egui::Context, at: egui::Pos2, of: MenuOf) {
        self.menu = Some(Open { at, of });
        self.menu_view.reset();
        // The keys are the menu's from now on: whatever had the keyboard gives it
        // up, or a text field would go on blinking its caret under the menu.
        if let Some(focused) = ctx.memory(|memory| memory.focused()) {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
        // It is drawn first in a frame, and this one is drawn: the next one is its,
        // and needs no asking for - a menu is opened by a button going down, and egui
        // asks for another frame after any frame with input in it.
    }

    /// Does what an item of one of the sidebar's menus does.
    fn side(&mut self, action: SideAction) {
        let collections = self.held.collections();
        match action {
            SideAction::RenameAlbum(id) => {
                if let Some(album) = collections.albums.iter().find(|album| album.id == id) {
                    self.rename.start(Of::Album(id), &album.name);
                }
            }
            SideAction::RenameSearch(id) => {
                if let Some(search) = collections.searches.iter().find(|search| search.id == id) {
                    self.rename.start(Of::Search(id), &search.name);
                }
            }
            // The name is typed in the folder's row in the sidebar, wherever the menu
            // was opened: a hidden sidebar is shown for it, and the row brought into
            // sight. With the name photon shows the folder by in it, to be corrected.
            SideAction::RenameFolder(id) => {
                let Some(folder) = self.held.folders().get(&id) else {
                    return;
                };
                self.rename.start(Of::Folder(id), folder_label(folder));
                if self.layout.sidebar_hidden {
                    self.layout.sidebar_hidden = false;
                    self.layout_store.ask(self.layout);
                }
                self.shell.reveal(What::Folder(id));
            }
            SideAction::UseFolderName(folder) => {
                self.writes.push(Write::FolderAlias {
                    folder,
                    name: None,
                    sent: None,
                });
            }
            SideAction::HideFolder { folder, hidden } => {
                self.writes.push(Write::FolderHidden { folder, hidden });
            }
            SideAction::Rescan { watched } => {
                self.writes.push(Write::Rescan { watched });
            }
            // "Delete…" asks first: under the name the thing has now, which is the name
            // the question is read with, whatever it is called by the time it is
            // answered.
            SideAction::DeleteAlbum(id) => {
                if let Some(album) = collections.albums.iter().find(|album| album.id == id) {
                    let name = album.name.clone();
                    self.open_dialog(Dialog::Confirm(Confirm::Album { id, name }));
                }
            }
            SideAction::DeleteSearch(id) => {
                if let Some(search) = collections.searches.iter().find(|search| search.id == id) {
                    let name = search.name.clone();
                    self.open_dialog(Dialog::Confirm(Confirm::Search { id, name }));
                }
            }
            // What does not work yet takes no press, and so answers nothing.
            SideAction::RevealFolder(_) => {}
        }
    }

    /// Whether `subject`'s delete has been asked for, and the lists not read since it
    /// was answered.
    fn going(&self, subject: Subject) -> bool {
        self.deleting.iter().any(|(going, _)| *going == subject)
    }

    /// Whether the lists on hand hold what `confirm` asks about, under the name it was
    /// asked under.
    fn listed(&self, confirm: &Confirm) -> bool {
        let collections = self.held.collections();
        let name = match confirm.subject() {
            Subject::Album(id) => (collections.albums.iter())
                .find(|album| album.id == id)
                .map(|album| album.name.as_str()),
            Subject::Search(id) => (collections.searches.iter())
                .find(|search| search.id == id)
                .map(|search| search.name.as_str()),
        };
        name == Some(confirm.name())
    }

    /// Draws the dialog that is open and does what was answered in it.
    fn show_dialog(&mut self, ui: &egui::Ui) {
        // A question about something that is no longer there, or no longer what it was
        // called, is no question: an album's number is handed out again once it is
        // free, and "Delete the album “Porto”?" answered with yes after Porto had gone
        // from elsewhere deleted the album made since under Porto's number.
        if let Some(Dialog::Confirm(confirm)) = &self.dialog
            && !self.listed(confirm)
        {
            self.dialog = None;
        }
        let Some(dialog) = &self.dialog else {
            return;
        };
        // The library's keywords, as the sidebar holds them and in its order.
        let keywords: Vec<String> = match dialog {
            Dialog::Tags(_) => (self.held.collections().tags.iter())
                .map(|tag| tag.tag.clone())
                .collect(),
            Dialog::Confirm(_) | Dialog::Export(_) => Vec::new(),
        };
        let done = self.dialog_view.show(ui, dialog, &keywords);
        match (done, &mut self.dialog) {
            (Done::Open, _) => {}
            (Done::Closed, _) => self.dialog = None,
            (Done::Typed(text), Some(Dialog::Tags(picker))) => picker.type_in(text),
            // Closed before its write, which is the user's last word on it: left up
            // until the library answers, it reads as a click that did nothing. A blank
            // name writes nothing and leaves the dialog as it is.
            (Done::Submit { tag, typed }, Some(Dialog::Tags(picker))) => {
                if let Some(text) = typed {
                    picker.type_in(text);
                }
                if let Some(submit) = picker.submit(tag.as_deref(), &keywords) {
                    self.dialog = None;
                    self.writes.push(Write::Tag(submit));
                }
            }
            (Done::Confirmed, Some(Dialog::Confirm(confirm))) => {
                let subject = confirm.subject();
                self.dialog = None;
                self.deleting.push((subject, false));
                self.writes.push(Write::Delete(subject));
            }
            // The path is asked about as it is typed: whether it is a folder, and one
            // photon will write into.
            (Done::Path(text), Some(Dialog::Export(export))) => {
                if let Some(ask) = export.type_in(text) {
                    self.checking.ask(ask);
                }
            }
            // Stored as it is ticked and not when the export runs: an export the user
            // cancels still said how they want exports to work.
            (Done::Tick(apply), Some(Dialog::Export(export))) => {
                let was = export.apply_edits();
                export.tick(apply);
                self.writes.push(Write::ExportApplyEdits { apply, was });
            }
            (Done::Size(max_edge), Some(Dialog::Export(export))) => export.size(max_edge),
            // Closed before the export runs: it can take minutes, and a dialog over the
            // progress it started would hide it.
            (Done::Export, Some(Dialog::Export(export))) => {
                if let Some(job) = export.job() {
                    self.dialog = None;
                    self.exporting = true;
                    self.exports.push(job);
                }
            }
            // What another kind of dialog answers: this one does not draw it.
            (
                Done::Typed(_)
                | Done::Submit { .. }
                | Done::Confirmed
                | Done::Path(_)
                | Done::Tick(_)
                | Done::Size(_)
                | Done::Export,
                _,
            ) => {}
        }
    }

    /// What was dropped on the window. Under the export's dialog the first path is its
    /// destination, put in the field and asked about as a typed one is - a file dropped
    /// first is said to be no folder, as one typed is - and nothing is watched: the
    /// dialog is asking for a folder, and that is what the drop answers. Everywhere
    /// else the paths are folders to watch, offered to the engine in their order.
    fn dropped(&mut self, paths: Vec<String>) {
        let Some(first) = paths.first() else {
            return;
        };
        if let Some(Dialog::Export(export)) = &mut self.dialog {
            if let Some(ask) = export.type_in(first.clone()) {
                self.checking.ask(ask);
            }
            return;
        }
        self.writes.push(Write::Watch { paths });
    }

    /// Opens the viewer on the photo `id` of the grid on screen.
    ///
    /// Nothing has the keyboard by then, and no name is half typed in the sidebar:
    /// Enter is the grid's only while nothing else has the keyboard, and a double
    /// click's first click took it off whatever had it and stored the name, as any
    /// click on a photo does. No menu is open and no button is held either, which is
    /// the grid's to see to: it answers no Enter in a frame with a mouse button in it,
    /// nor while a press on the photos is held (`grid/view.rs`).
    fn open_viewer(&mut self, id: i64) {
        let Some(viewer) = Viewer::open(id, &self.index) else {
            return;
        };
        self.viewer_items.ask(id);
        // A view of its own: what the wheel had turned towards a next photo in the
        // viewer before this one is not a turn made in this one.
        self.viewer_view = ViewerView::default();
        self.viewer = Some(Viewing {
            viewer,
            item: None,
            full: None,
            failed: None,
            stale: None,
            within: None,
        });
    }

    /// Closes the viewer and hands its photo back to the grid: selected, alone, and
    /// brought into sight. A photo the view no longer holds hands back none: nothing
    /// is selected for it, and the grid is brought to where it stood.
    ///
    /// The place is kept as the photo that stands there and looked up in the grid the
    /// next frame draws (`take_photo`), as a folder asked for is: this is the end of a
    /// frame, and the next may draw a scan's grid with photos more above it.
    fn close_viewer(&mut self) {
        let Some(open) = self.viewer.as_ref() else {
            return;
        };
        let (offset, id) = open.viewer.hand_back(&self.index);
        if id.is_some() {
            self.selection.select(&self.index, offset);
        }
        self.showing = (self.index.rows(offset, 1).first()).map(|entry| entry.id);
        self.shut_viewer();
    }

    /// The viewer is over: what it held goes with it. Its previews too, which nothing
    /// else lets go of but their store's own limit - a hundred megabytes of texture for
    /// the rest of the session, after any stretch of looking at photos.
    fn shut_viewer(&mut self) {
        self.viewer = None;
        self.previews.clear();
    }

    /// A typed search is sent in a frame of its own, which a still window would not
    /// draw: asked for at every frame until it is sent, since a frame asked for after a
    /// delay comes a frame early.
    fn ask_for_search(&self, ctx: &egui::Context, now: f64) {
        if let Some(at) = self.search.due_at() {
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ctx.request_repaint_after(wait);
        }
    }

    /// Hands the grid the photo to bring into sight, as its place in the grid about to
    /// be drawn. One that grid does not hold moves nothing.
    fn take_photo(&mut self) {
        if let Some(id) = self.showing.take()
            && let Some(offset) = self.index.position_of(id)
        {
            self.view.go_to(offset, Align::Nearest);
        }
    }

    /// What the tasks have answered about the photo in the viewer. Each answer counts
    /// for the photo it was asked of, and for the picture it was asked for: the user
    /// may have stepped on, or the photo been edited, since.
    fn viewer_answers(&mut self, ctx: &egui::Context) -> bool {
        let mut answered = false;
        if let Some(answer) = self.viewer_items.answer() {
            answered = true;
            // (Of the photo the viewer is on: `Latest` hands out the answer to the
            // last question only, and each step asks.)
            if let (Ok((id, item)), Some(open)) = (answer, &mut self.viewer) {
                // What the library says has been read again: a picture that waited
                // for that may be asked for (`Made::Stale`).
                open.stale = None;
                match item {
                    Ok(item) => {
                        // Still a photo. If the grid has lost it, it has only left
                        // the view.
                        open.viewer.lost(id, true);
                        if open.viewer.took(id, shown_of(&item)) {
                            // Another picture: what was made is of the one before.
                            open.full = None;
                        }
                        open.item = Some(item);
                    }
                    // The engine refuses a photo that is gone, and one it has marked
                    // missing: only that is "no longer available". Anything else - the
                    // library busy for a moment - leaves what is on screen as it is.
                    Err(err) if err.kind == "notFound" => {
                        open.viewer.lost(id, false);
                        if open.viewer.gone() {
                            open.item = None;
                            open.full = None;
                        }
                    }
                    Err(_) => {}
                }
            }
        }
        if let Some(answer) = self.pictures.answer() {
            answered = true;
            // The answer to the last question asked: nothing is on its way now.
            let asked = self.asking.take();
            let (of, made) = match answer {
                Ok((of, made)) => (Some(of), made),
                // The worker panicked at it. Said to have failed: with nothing held,
                // nothing on its way and nothing said, it would be asked for again in
                // every frame.
                Err(_) => (asked, Made::Failed(CANNOT_SHOW.to_owned())),
            };
            if let (Some(asked), Some(open)) = (of, &mut self.viewer)
                && asked.want == open.viewer.want()
            {
                let want = asked.want;
                let options = egui::TextureOptions::LINEAR;
                match made {
                    Made::Levels { levels, fitted } => {
                        let levels = (levels.into_iter().enumerate())
                            .map(|(n, image)| {
                                let width = image.size[0] as u32;
                                let name = format!("viewer-{}-{n}", want.id);
                                (ctx.load_texture(name, image, options), width)
                            })
                            .collect();
                        let fitted = fitted.map(|image| {
                            ctx.load_texture(format!("viewer-{}-fitted", want.id), image, options)
                        });
                        open.full = Some(FullPicture {
                            want,
                            fit: asked.fit,
                            levels,
                            fitted,
                        });
                    }
                    Made::Failed(said) => open.failed = Some((want, said)),
                    // Made from another state of the photo than the one asked for.
                    // Usually the grid that says so is on its way, and the picture of
                    // the photo as it is will be another question. Not always: turned
                    // and turned back before any frame was drawn, the photo is under
                    // the key it had, the window never heard it was another picture
                    // meanwhile, and nothing would ask again. So what the library says
                    // of the photo is read again, and the picture asked for after that
                    // answer - which bounds the asking to one a round trip, and asks
                    // under the key the engine has by then.
                    Made::Stale => {
                        open.stale = Some(want);
                        self.viewer_items.ask(want.id);
                    }
                }
            }
        }
        // A photo the library says cannot be shown has that said in its place, so no
        // picture of it is held to be drawn: one asked for at the opening, before the
        // library had said anything, may arrive all the same.
        if let Some(open) = &mut self.viewer
            && cannot_show(&open.viewer)
        {
            open.full = None;
        }
        answered
    }

    /// One frame of the viewer: what it asks the tasks for, what it draws, and what the
    /// user did in it.
    fn show_viewer(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some(mut open) = self.viewer.take() else {
            return;
        };
        let ctx = ui.ctx().clone();
        // Tab is nobody's here. No control of the viewer takes the keyboard, but a
        // message drawn over it has a button that does, and with the keyboard on that
        // the arrows walk it on instead of stepping, and the grid's keys are dead when
        // the viewer has closed. egui has read Tab already, from the frame's raw input:
        // its walk is called off, as under a menu.
        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        let (id, key, want) = (open.viewer.id(), open.viewer.key(), open.viewer.want());
        let rested = open.viewer.rested(now);

        // The previews: this photo's first, then the two on either side of it, so that
        // a step lands on a picture. They are on disk, and a few milliseconds each to
        // read: they do not wait for the viewer to rest, as the full picture does.
        let mut wanted = vec![crate::thumbs::loader::Want { id, key }];
        wanted.extend(
            self.index
                .neighbours(id, PRELOAD)
                .into_iter()
                .filter_map(|id| {
                    self.entry(id).map(|entry| crate::thumbs::loader::Want {
                        id,
                        key: entry.thumb_key,
                    })
                }),
        );
        self.previews.frame(&ctx, &wanted, false);

        // The full picture, once the viewer has rested on a photo that has one to make.
        // (A video is asked about like a photo: the engine refuses it at once.)
        let failed = cannot_show(&open.viewer);
        // With the box it is fitted into, in pixels: the engine scales a copy to it. A
        // window that changes size is another box, and the copy for it is asked for
        // once the size has stood for a moment - a window being dragged wider is a new
        // size in every frame, and each asking is a whole decode.
        let window = ctx.content_rect().size() * ctx.pixels_per_point();
        let within = (window.x.round() as u32, window.y.round() as u32);
        if open.within.is_none_or(|(seen, _)| seen != within) {
            open.within = Some((within, now));
        }
        let stood = open.within.map_or(now, |(_, since)| since);
        let ask = PictureAsk {
            want,
            max_side: ctx.input(|input| input.max_texture_side) as u32,
            fit: within,
        };
        // Whether to ask is decided from what is held and what is on its way, never
        // from what was once asked: a picture let go of is asked for again - one photo
        // on and straight back inside the rest, a photo that was gone and is here
        // again - where "asked for once" left the photo at its preview for good.
        let coming = self.asking.filter(|asked| asked.want == want);
        let held = open.picture().map(|full| full.fit);
        let refused = (open.failed.as_ref()).is_some_and(|(of, _)| *of == want);
        // There is none of it, held or on its way, and nothing says there cannot be:
        // not a photo that is gone or cannot be shown, not one whose picture could not
        // be made, not one the library is being asked about again first.
        let none = held.is_none()
            && coming.is_none()
            && !refused
            && !open.viewer.gone()
            && !failed
            && open.stale != Some(want);
        // The newest of it, on its way or held, is for a window of another size.
        let other_size = (coming.map(|asked| asked.fit).or(held)).is_some_and(|fit| fit != within);
        let fit_due = stood + FIT_REST_MS;
        if (none && rested) || (other_size && now >= fit_due) {
            self.pictures.ask(ask);
            self.asking = Some(ask);
            self.asked_pictures += 1;
        } else if other_size {
            ctx.request_repaint_after(Duration::from_secs_f64((fit_due - now) / 1000.0));
        }
        if let Some(due) = open.viewer.rest_due().filter(|due| *due > now) {
            ctx.request_repaint_after(Duration::from_secs_f64((due - now) / 1000.0));
        }

        let preview = self.previews.texture(key).cloned();
        let has_preview = preview.is_some();
        let caption = (open.item.as_ref())
            .map(|item| {
                viewer_caption::caption(
                    Captioned {
                        file_name: &item.file_name,
                        taken_at: item.taken_at,
                        width: item.width,
                        height: item.height,
                        orientation: item.orientation,
                        size: item.size,
                    },
                    open.viewer.position(&self.index),
                )
            })
            .unwrap_or_default();
        let photo_caption = (open.item.as_ref())
            .and_then(|item| viewer_caption::caption_line(item.caption.as_deref()));
        let said = if open.viewer.gone() {
            Some((GONE.to_owned(), false))
        } else if open.viewer.is_video() {
            Some((NO_VIDEO.to_owned(), true))
        } else if failed {
            let why = (open.item.as_ref()).and_then(|item| item.thumb_error.clone());
            Some((why.unwrap_or_else(|| CANNOT_SHOW.to_owned()), false))
        } else {
            // A full picture that could not be made leaves the preview where it is, as
            // the web view left it: a photo on a drive that is not plugged in is
            // looked at from photon's own cache. Why is said only where there is no
            // picture at all.
            (open.failed.as_ref())
                .filter(|(of, _)| *of == want && !has_preview)
                .map(|(_, why)| (why.clone(), false))
        };
        let fitted = open.picture().and_then(|full| full.fitted.clone());
        let levels: Vec<Level<'_>> = (open.picture().iter())
            .flat_map(|full| full.levels.iter())
            .map(|(texture, width)| Level {
                texture,
                width: *width,
            })
            .collect();
        let len = self.index.len();
        let data = ViewerData {
            levels: &levels,
            fitted: fitted.as_ref(),
            preview: preview.as_ref(),
            message: (said.as_ref()).map(|(text, over)| Message { text, over: *over }),
            caption: &caption,
            photo_caption: photo_caption.as_deref(),
            // (Nothing is known of a photo that is gone: its facts went with it.)
            starred: open.item.as_ref().map(|item| item.starred),
            can_prev: open.viewer.can_step(-1, len),
            can_next: open.viewer.can_step(1, len),
        };
        let actions = self.viewer_view.show(ui, &data);
        drop(levels);

        // The messages lie over the viewer as over the grid: a star the folder would
        // not take is said here too.
        let (toasts, next_toast) = self.toasts.at(now);
        if let Some(at) = next_toast {
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ctx.request_repaint_after(wait);
        }
        let dismissed = crate::shell::toasts(ui, ctx.content_rect(), toasts, &theme::tokens::DARK);
        if let Some(id) = dismissed {
            self.toasts.dismiss(id);
        }

        let mut closed = false;
        for action in actions {
            let moved = match action {
                ViewerAction::Close => {
                    closed = true;
                    break;
                }
                ViewerAction::Step(dir) => open.viewer.step(dir, &self.index, now),
                ViewerAction::First => open.viewer.go_to(0, &self.index, now),
                ViewerAction::Last => open.viewer.go_to(usize::MAX, &self.index, now),
                // By the photo on screen, not by the selection behind it. Not before
                // the library has said whether it is starred or hidden: the key would
                // write the opposite of nothing.
                ViewerAction::Star => {
                    if let Some(item) = open.item.as_ref() {
                        self.write(TileAction::Star(!item.starred), vec![item.id]);
                    }
                    false
                }
                ViewerAction::Hide => {
                    if let Some(item) = open.item.as_ref() {
                        self.write(TileAction::Hide(!item.hidden), vec![item.id]);
                    }
                    false
                }
            };
            if moved {
                // Another photo: what was said and made of the one left is not its.
                // And a picture that could not be made is asked for again when the
                // photo is come back to: its drive may be there by then.
                open.item = None;
                open.full = None;
                open.failed = None;
                self.viewer_items.ask(open.viewer.id());
            }
        }
        self.viewer = Some(open);
        if closed {
            self.close_viewer();
        }
    }

    /// Stores the name in the sidebar's field: Enter, or the field left.
    fn store_name(&mut self) {
        let Some(Commit { of, name, sent }) = self.rename.commit() else {
            return;
        };
        self.writes.push(match of {
            Of::NewAlbum => Write::AlbumNew { name, sent },
            Of::Album(album) => Write::AlbumRename { album, name, sent },
            Of::Search(search) => Write::SearchRename { search, name, sent },
            // A folder's blank name is how its directory's own is asked back: the
            // library takes the empty name for none.
            Of::Folder(folder) => Write::FolderAlias {
                folder,
                name: Some(name),
                sent: Some(sent),
            },
        });
    }

    /// The photo in the grid on screen with this id.
    fn entry(&self, id: i64) -> Option<&GridEntry> {
        let at = self.index.position_of(id)?;
        self.index.rows(at, 1).first()
    }

    /// The photo the star key goes by (`Selection::deciding`).
    fn deciding(&self) -> Option<&GridEntry> {
        self.entry(self.selection.deciding(&self.index)?)
    }

    /// The items of the selection's menu, as things are in this frame: a grid that
    /// arrived while it was open may have taken photos out of the selection.
    fn tile_items(&self) -> Vec<Item<TileAction>> {
        let collections = self.held.collections();
        tile_menu::tile_menu(&tile_menu::Facts {
            count: self.selection.len(),
            deciding: self.deciding().map(|entry| Deciding {
                video: entry.kind == MediaKind::Video,
                starred: entry.starred,
            }),
            place: self.nav.settled(),
            albums: &collections.albums,
            people: &collections.people,
            finds_faces: self.engine.face_detection(),
            mac: cfg!(target_os = "macos"),
            exporting: self.exporting,
        })
    }

    /// Does what an item of the selection's menu does, to what is selected now: the ids
    /// are read here, at the pick, and not when the menu opened. Each is a write, made
    /// in its turn off this thread; the grid is told nothing, its rebuild arrives like
    /// any other, and `Selection::follow` takes out what left and lands the lead.
    fn pick(&mut self, action: TileAction) {
        let ids = self.selection.ids(&self.index);
        // The keyword picker is a dialog, and takes its photos as it opens: the
        // selection behind it can change while it is up, and its title has said how
        // many photos it writes to.
        let mode = match action {
            TileAction::AddKeyword => Some(Mode::Add),
            TileAction::RemoveKeyword => Some(Mode::Remove),
            // The export's dialog takes its photos as it opens too, and asks how the
            // last export left its checkbox.
            TileAction::Export => {
                self.open_dialog(Dialog::Export(ExportDialog::new(ids)));
                self.edits.ask(());
                return;
            }
            _ => None,
        };
        match mode {
            Some(mode) => self.open_dialog(Dialog::Tags(TagPicker::new(mode, ids))),
            None => self.write(action, ids),
        }
    }

    /// Queues what `action` writes, for the photos `ids`.
    fn write(&mut self, action: TileAction, ids: Vec<i64>) {
        let write = match action {
            TileAction::Star(starred) => Write::Stars { ids, starred },
            TileAction::Hide(hidden) => Write::Hidden { ids, hidden },
            TileAction::AddToAlbum(album) => Write::AlbumAdd { album, ids },
            TileAction::RemoveFromAlbum(album) => Write::AlbumRemove { album, ids },
            TileAction::RemoveFromPerson(person) => {
                let key = format!("p:{person}");
                let people = &self.held.collections().people;
                let name = (people.iter().find(|person| person.key == key))
                    .map_or(String::new(), |person| person.name.clone());
                Write::PersonRemove { person, name, ids }
            }
            // What does not work yet takes no press, and so answers nothing.
            TileAction::Reveal
            | TileAction::OpenInApp
            | TileAction::CopyPhoto
            | TileAction::Compare
            // (Opened by `pick`, as dialogs.)
            | TileAction::AddKeyword
            | TileAction::RemoveKeyword
            | TileAction::Export
            | TileAction::AddToPerson => return,
        };
        self.writes.push(write);
    }

    /// What a key asked of the selection: the menu's item, without the menu - for the
    /// photos that were selected at the key, which the ask carries. The frame's later
    /// keys and clicks have been answered by now, and the selection is theirs.
    fn ask(&mut self, asked: Asked) {
        let action = match asked.ask {
            // One key for the menu's two items: starred, the deciding photo has the
            // whole selection unstarred; not, all of it is starred.
            Ask::Star => {
                let starred = (asked.deciding)
                    .and_then(|id| self.entry(id))
                    .is_some_and(|entry| entry.starred);
                TileAction::Star(!starred)
            }
            Ask::Hide => TileAction::Hide(self.nav.settled().view != View::Hidden),
        };
        self.write(action, asked.ids);
    }

    /// The status bar's count of the photos shown, when there is one to give.
    pub fn photo_count(&self) -> Option<String> {
        empty::photo_count(&self.grid)
    }

    /// What photon is doing in the background, as the status bar says it.
    pub fn status(&self) -> Vec<Line> {
        self.scans.lines(&self.watched)
    }

    /// The folders photon watches, as last read.
    pub fn watched(&self) -> &[WatchedFolder] {
        &self.watched
    }

    /// What an empty library says where its photos would be: about the grid that is on
    /// screen, like the line of an empty view, and only what is known - nothing before
    /// the watched folders have been read for this launch and the hidden photos counted
    /// for this library.
    pub fn panel(&self) -> Option<Panel> {
        let library = LibraryFacts {
            known: self.scans.known(),
            watched: &self.watched,
            scanning: self.scans.scanning(),
            hidden: self.counted.then_some(self.counts.hidden),
            drops: self.drops,
        };
        empty::library_panel(&self.grid, self.nav.settled(), &library)
    }

    /// The line shown in place of photos. It is about the grid that is on screen, not
    /// about the view being gone to.
    pub fn notice(&self) -> Option<String> {
        empty::view_notice(&self.grid, self.nav.settled(), self.held.collections())
    }

    /// Asks for `step`, unless it leads to where the user already is. What a click on a
    /// row of the sidebar does, and the one way the engine's view is moved.
    ///
    /// The search box goes with it. A search - a saved one's row, On this day - is what
    /// the box then holds. Any other step is a switch away, which clears the engine's
    /// query: a search half typed is dropped, or it would land after the switch and
    /// replace the grid that was asked for, and the box is emptied as the switch is made.
    pub fn go(&mut self, step: Step) {
        if let Step::Search(query) = step {
            self.search.search(&query);
            self.search_for(query);
            return;
        }
        // A sort moves no view and clears no query: the search box is left as it is.
        if let Step::Sort(_) = step {
            if self.nav.wants(&step) {
                let number = self.steps.push(step.clone());
                self.nav.asked(number, step);
            }
            return;
        }
        if !self.nav.wants(&step) {
            // The user is there already. What they had half typed is dropped all the
            // same, and its text with it: left in the box it would be searched for by
            // nothing, and cleared by an Escape that then went to All photos.
            self.search.clear();
            return;
        }
        let number = self.steps.push(step.clone());
        self.nav.asked(number, step);
        self.search.leave(number);
    }

    /// Asks for the search for `query`, unless that is where the user already is. It
    /// takes the place of a search still waiting its turn: while one rebuild runs, each
    /// further word typed is one more search asked for, and only the last is wanted.
    fn search_for(&mut self, query: String) {
        let step = Step::Search(query);
        if !self.nav.wants(&step) {
            return;
        }
        let number = self.steps.push_or_replace(step.clone(), Step::is_search);
        self.nav.asked(number, step);
    }

    /// A click on a folder: to All photos if the user is on an excursion, and to the
    /// folder's place in the grid once the grid that holds it is on screen.
    fn enter_folder(&mut self, folder: i64) {
        // A search typed and not yet sent would land after the jump and replace the
        // grid that was jumped in.
        self.search.cancel();
        if let Some(home) = self.nav.home_of_folders() {
            self.go(home);
        }
        self.jump(folder);
    }

    /// A click on All photos: back to the whole library, at the folder last browsed there.
    ///
    /// An excursion - Starred, an album, a search - leaves All's place alone, so the folder
    /// remembered is where the user left the gallery; a click on a folder instead lands on
    /// that folder's top, which is what made going back feel like a reset. Already in All
    /// photos the user is at their own place, and the click asks for nothing: not a jump
    /// to the top of the folder they are in the middle of.
    fn return_to_all(&mut self) {
        let home = Step::View(View::All);
        if !self.nav.wants(&home) {
            self.search.clear();
            return;
        }
        // The order All photos will be shown in: a sort still on its way is ahead of
        // this step on the queue, and the grid on screen is a rebuild behind it.
        let left = self.last_folder.left(self.nav.sort_target());
        self.go(home);
        if let Some(folder) = left {
            self.jump(folder);
        }
    }

    /// Goes to `folder` in the grid: now, when the grid on screen is the one to look it
    /// up in, and otherwise when that grid is (`adopt`).
    fn jump(&mut self, folder: i64) {
        if let Some(folder) = self.nav.jump(folder) {
            self.go_to_folder(folder);
        }
    }

    /// Asks for `folder` at the top of the grid in the next frame drawn. It is kept as
    /// the folder, not as its offset: a click is acted on after its frame is drawn, and
    /// the next frame may draw another grid - a scan's, with a hundred photos more above
    /// the folder - in which the offset of the grid that was clicked in names a photo of
    /// the folder beside it.
    fn go_to_folder(&mut self, folder: i64) {
        self.going = Some(folder);
    }

    /// Hands the grid the folder asked for, as its place in the grid about to be drawn.
    /// One that grid does not hold - every photo of it gone since the click - moves
    /// nothing: staying where the grid is beats scrolling nowhere.
    fn take_folder(&mut self) {
        if let Some(folder) = self.going.take()
            && let Some(offset) = self.index.offset_of_folder(folder)
        {
            self.view.go_to(offset, Align::Start);
        }
    }

    fn act(&mut self, action: Action, now_ms: f64) {
        match action {
            Action::Search(SearchAction::Typed) => self.search.typed(now_ms),
            Action::Search(SearchAction::Clear) => {
                self.search.clear();
                self.search_for(String::new());
            }
            Action::Search(SearchAction::Save) => {
                if let Some(query) = self.search.save() {
                    let name = query.clone();
                    self.writes.push(Write::SaveSearch { name, query });
                }
            }
            Action::ToggleSidebar => {
                // A name half typed in a sidebar that is hidden is stored, as leaving
                // its field stores it: the field goes with the sidebar.
                if !self.layout.sidebar_hidden {
                    self.store_name();
                }
                self.layout.sidebar_hidden = !self.layout.sidebar_hidden;
                self.layout_store.ask(self.layout);
            }
            Action::Row(What::NewAlbum) => self.rename.start(Of::NewAlbum, ""),
            Action::Named(Named::Typed(text)) => self.rename.type_in(text),
            Action::Named(Named::Cancel) => self.rename.cancel(),
            Action::Named(Named::Commit) => self.store_name(),
            // Opened where the frame's context is at hand (`eframe::App::ui`).
            Action::RowMenu { .. } => {}
            Action::Row(What::Folder(folder)) => self.enter_folder(folder),
            Action::Row(What::Fixed(Fixed::All)) => self.return_to_all(),
            // A group is folded and unfolded where the user clicks, and stored then.
            Action::Row(What::Group(group)) => {
                let open = &mut self.layout.open;
                let fold = match group {
                    Group::Albums => &mut open.albums,
                    Group::Searches => &mut open.searches,
                    Group::People => &mut open.people,
                    Group::Tags => &mut open.tags,
                };
                *fold = !*fold;
                self.layout_store.ask(self.layout);
            }
            Action::Row(entry) => {
                // The clock itself, not the day the row was drawn with: a click is never a
                // day behind.
                if let Some(step) = entry.step(today(&self.zone)) {
                    self.go(step);
                }
            }
            Action::Width { width, store } => {
                self.layout.sidebar_width = width;
                if store {
                    self.layout_store.ask(self.layout);
                }
            }
            Action::Dismiss(id) => self.toasts.dismiss(id),
            Action::Sort(sort) => self.go(Step::Sort(sort)),
            Action::Panel(PanelButton::ShowHidden) => self.go(Step::View(View::Hidden)),
            // Drawn where they will be, and not answered yet (`PanelButton::works`).
            Action::Panel(PanelButton::AddFolder | PanelButton::WatchedFolders) => {}
            // Applied first and stored second, so the press is answered at once. A size
            // the library would not store is kept for this session all the same: going
            // back would answer a disk error with the whole grid laid out again.
            Action::Size(size) => {
                self.size = size;
                self.writes.push(Write::TileSize(size));
            }
        }
    }

    /// Takes what the engine has reported since the last frame, and says whether it had
    /// reported anything.
    fn take_events(&mut self) -> bool {
        // The launch: until the engine has settled its folders and started their scans,
        // the list on hand may be from before the Pictures folder was watched. It is
        // read again then, and that read is the one to go by.
        if self.scans.starting() && self.engine.startup_scans_started() {
            self.scans.started();
            self.folder_list.ask(());
        }
        let mut reported = false;
        let mut changed = false;
        let mut data_changed = false;
        let mut folders_changed = false;
        for event in self.events.try_iter() {
            reported = true;
            match event {
                Event::Library(library) => {
                    changed = true;
                    data_changed |= library.data_changed;
                }
                Event::Scan(scan) => {
                    let listed = self
                        .watched
                        .iter()
                        .any(|folder| folder.id == scan.watched_id);
                    let asks = self.scans.scan(scan, listed, self.grid.len);
                    folders_changed |= asks.folders;
                    // The grid on screen is read again in `adopt`, which says so.
                    changed |= asks.settle;
                }
                // Whether a folder can be reached is in the list too, and is written into
                // the list on hand here and now: the scan that found a drive back, or
                // gone, says so before it says it is done, and the list read again for it
                // is a frame behind at best - for that long a folder just found was one
                // photon "cannot reach", and one just lost "held no photos".
                Event::Folder(status) => {
                    self.scans.folder_status(status);
                    if let Some(folder) =
                        (self.watched.iter_mut()).find(|folder| folder.id == status.watched_id)
                    {
                        folder.online = status.online;
                    }
                    folders_changed = true;
                }
                Event::Export(export) => self.scans.export(export),
                Event::Face(face) => self.scans.face(face),
            }
        }
        // A grid has been published. It is taken in `adopt`, once it is known what it
        // shows, and not here.
        self.stale |= changed;
        // A folder renamed, added or given an alias: the headers are named from the list.
        // And an album made, a face named, a keyword renamed: the sidebar lists them.
        if data_changed {
            self.collecting.ask(());
        }
        if data_changed || folders_changed {
            self.folder_list.ask(());
        }
        reported
    }

    /// Takes what the tasks have answered since the last frame, and says whether any had.
    fn take_answers(&mut self, now_ms: f64) -> bool {
        let mut answered = false;
        if let Some(read) = self.folder_list.answer() {
            answered = true;
            if let Ok(Some(list)) = read {
                self.watched = list.watched;
                self.held.set_folders(list.folders);
            }
            // Read or not: the list on hand is what there is to go by. Left as "not
            // read", a read that failed at launch kept an empty library saying nothing.
            self.scans.listed(&self.watched);
        }
        if let Some(counts) = self.counting.answer() {
            answered = true;
            if let Ok(counts) = counts {
                self.counts = counts;
                self.counted = true;
            }
        }
        if let Some(read) = self.collecting.answer() {
            answered = true;
            if let Ok(Some(read)) = read {
                self.search.lists_read(&read.searches);
                self.held.set_collections(read);
                // What was deleted has left the lists now: these were read after the
                // answer that said so, a read asked for before it being dropped for
                // the one asked for at it.
                self.deleting.retain(|(_, answered)| !answered);
            }
        }
        let _ = self.layout_store.answer();
        let _ = self.remembering.answer();
        // The answers about the export's dialog count for the dialog that is open: one
        // about a path counts only for the text its field still holds
        // (`ExportDialog::checked`), and the stored checkbox not over a tick made since.
        if let Some(answer) = self.checking.answer() {
            answered = true;
            if let (Ok((asked, answer)), Some(Dialog::Export(export))) = (answer, &mut self.dialog)
            {
                export.checked(&asked, answer);
            }
        }
        if let Some(answer) = self.edits.answer() {
            answered = true;
            if let (Ok(Some(apply)), Some(Dialog::Export(export))) = (answer, &mut self.dialog) {
                export.remembered(apply);
            }
        }
        for (_, answer) in self.exports.answers() {
            answered = true;
            self.exporting = false;
            let said = match answer {
                Ok((asked, dest, Ok(done))) => export_dialog::report(
                    done.written,
                    done.failed,
                    done.reason.as_deref(),
                    asked,
                    &dest,
                ),
                // The engine would not begin: its destination had been watched since
                // the dialog asked, or was gone.
                Ok((_, _, Err(said))) => Err(said),
                Err(_) => Err("photon could not finish that export.".to_owned()),
            };
            match said {
                Ok(said) => self.toasts.done(said, now_ms),
                Err(said) => self.toasts.error(said, now_ms),
            }
        }
        for (number, answer) in self.steps.answers() {
            answered = true;
            // A step that panicked is a step the engine did not make.
            let outcome =
                answer.unwrap_or_else(|_| Err("photon could not change the view.".to_owned()));
            let outcome = outcome.map(|(landed, said)| {
                if let Some(said) = said {
                    self.toasts.error(said, now_ms);
                }
                landed
            });
            if let Some(refused) = self.nav.answered(number, outcome) {
                // The box shows what the grid shows: its text back after a refused
                // switch, the search the grid is left on after a refused search.
                // The latter only when nothing asked since is on its way: the same text
                // may be waiting its turn behind the search that was refused, and the
                // box emptied of it would then show nothing over its results.
                let asked = match &refused.step {
                    Some(Step::Search(query)) if refused.last => Some(query.as_str()),
                    _ => None,
                };
                let shown = (refused.shown.view == View::Search).then_some(&refused.shown.arg);
                self.search
                    .refused(number, asked, shown.map_or("", String::as_str));
                self.toasts.error(refused.said, now_ms);
            }
        }
        for (_, answer) in self.writes.answers() {
            answered = true;
            match answer {
                // The engine announces no data change for a search saved: the lists are
                // read again because this side knows it wrote one.
                Ok(Ok(Wrote::Search)) => self.collecting.ask(()),
                Ok(Ok(Wrote::TileSize)) => {}
                Ok(Err(Unwritten::Search { query, said })) => {
                    self.search.save_refused(&query);
                    self.toasts.error(said, now_ms);
                }
                Ok(Err(Unwritten::TileSize { said })) => self.toasts.error(said, now_ms),
                // A star the engine could not write - a folder whose INI is read-only -
                // costs a message, and not the photos beside it.
                Ok(Ok(Wrote::Stars {
                    asked,
                    done,
                    starred,
                })) => {
                    if let Some(said) = tile_menu::stars_missed(asked, done, starred) {
                        self.toasts.error(said, now_ms);
                    }
                }
                Ok(Ok(Wrote::Grid)) => {}
                Ok(Ok(Wrote::Lists)) => self.collecting.ask(()),
                Ok(Ok(Wrote::Removed {
                    name,
                    removed,
                    kept,
                    asked,
                })) => {
                    // The lists are read again by the rebuild a write of the people
                    // announces, as after any change of the library's data.
                    let said = tile_menu::removed_message(&name, removed, kept, asked);
                    self.toasts.done(said, now_ms);
                }
                Ok(Ok(Wrote::Named { sent })) => {
                    self.rename.landed(sent);
                    self.collecting.ask(());
                }
                // The folder list is read again here, and not left to the rebuild the
                // engine makes for a folder's name or its hiding: it makes none when no
                // photo changed. "Hide folder" on a folder whose photos were all hidden
                // one by one changes the folder and no photo, and its menu went on
                // offering to hide it, with no way to bring it back.
                Ok(Ok(Wrote::Folder { sent })) => {
                    if let Some(sent) = sent {
                        self.rename.landed(sent);
                    }
                    self.folder_list.ask(());
                }
                Ok(Ok(Wrote::Deleted(of))) => {
                    for (going, answered) in &mut self.deleting {
                        *answered |= *going == of;
                    }
                    self.collecting.ask(());
                    // The album on screen, or on its way there: the engine has emptied
                    // its grid, and All photos is the place to land.
                    if let Subject::Album(album) = of {
                        let shown = Place {
                            view: View::Album,
                            arg: album.to_string(),
                        };
                        if self.nav.target() == shown {
                            self.go(Step::View(View::All));
                        }
                    }
                }
                // Refused, it is still there, and its row is a row again. The lists are
                // read all the same: the commonest refusal is of something that has
                // gone from elsewhere, whose row should go too.
                Ok(Err(Unwritten::Delete { of, said })) => {
                    self.deleting.retain(|(going, _)| *going != of);
                    self.collecting.ask(());
                    self.toasts.error(said, now_ms);
                }
                Ok(Ok(Wrote::Tagged {
                    mode,
                    stored,
                    done,
                    asked,
                })) => {
                    self.toasts.done(mode.written(&stored, done, asked), now_ms);
                }
                // A drop is reported once: what is now watched, and the first refusal
                // with a count of the rest. Each folder watched has had its first scan
                // started, and is in the list on hand at once - the list read again
                // arrives a moment later, and an empty library must not say meanwhile
                // that no folder is watched, or that photon has found nothing in one it
                // has not begun to read.
                Ok(Ok(Wrote::Watched { folders, refused })) => {
                    // One folder twice in a drop - by its path, and by a link to it -
                    // is answered by the engine as itself each time, and is one folder:
                    // counted twice, "Watching 2 folders" was said of one.
                    let mut seen = Vec::new();
                    let folders: Vec<_> = (folders.into_iter())
                        .filter(|folder| {
                            !seen.contains(&folder.id) && {
                                seen.push(folder.id);
                                true
                            }
                        })
                        .collect();
                    for folder in &folders {
                        self.scans.added(folder.id);
                        if !self.watched.iter().any(|watched| watched.id == folder.id) {
                            self.watched.push(folder.clone());
                        }
                    }
                    // (The list itself is read again when the folder's scan ends, as
                    // after any scan: asked for here as well, it changed nothing.)
                    let came = Watched {
                        added: folders.into_iter().map(|folder| folder.path).collect(),
                        refused,
                    };
                    if let Some(said) = came.said() {
                        self.toasts.done(said, now_ms);
                    }
                    if let Some(refusal) = came.refusal() {
                        self.toasts.error(refusal, now_ms);
                    }
                }
                Ok(Ok(Wrote::Stored)) => {}
                Ok(Err(Unwritten::Edits { was, said })) => {
                    if let Some(Dialog::Export(export)) = &mut self.dialog {
                        export.put_back(was);
                    }
                    self.toasts.error(said, now_ms);
                }
                Ok(Err(Unwritten::Name { sent, said })) => {
                    self.rename.refused(sent);
                    self.toasts.error(said, now_ms);
                }
                Ok(Err(Unwritten::Said(said))) => self.toasts.error(said, now_ms),
                Err(_) => self
                    .toasts
                    .error("photon could not store that change.", now_ms),
            }
        }
        answered
    }

    /// Puts the grid the engine has published on screen, when it is known what it shows.
    ///
    /// The engine's own view is not asked: it moves when a step begins, a whole rebuild
    /// before the step's grid is published, so with two steps on their way the first's grid
    /// arrives under the second's view. What a grid shows comes from the answer of the
    /// step that built it, and a grid that arrives before that answer waits for it.
    fn adopt(&mut self) {
        if !self.stale {
            return;
        }
        let (version, index, build_error, layout_gen) = self.engine.published();
        let Some(shown) = self.nav.adopt(version) else {
            return;
        };
        self.stale = false;
        // A scan that ended over an empty grid counts as running until this (`Scans`).
        self.scans.settled();
        // A grid that held photos and holds none: the hidden photos are counted again
        // before anything is said of it (`counted`). Not on every empty grid, which
        // would blink the panel off at each rebuild of an empty library.
        if self.grid.len > 0 && index.is_empty() {
            self.counted = false;
        }
        self.grid = GridState {
            version,
            len: index.len(),
            build_error,
        };
        let before = std::mem::replace(&mut self.index, index);
        self.layout_gen = layout_gen;
        // Other results are another list: a place in the old one names nothing here.
        if shown.other_results {
            self.view.to_top();
            // A folder asked for in the results that were on screen is not a place in
            // these.
            self.going = None;
            // Nor are the photos picked there these, or the anchor's offset the same
            // photo in another order.
            self.selection.clear();
            // Nor is the viewer's photo, where these do not hold it: a view asked for
            // and, before its grid had landed, a photo opened from the grid still on
            // screen. It has no place in these to step on from - "where it stood" is a
            // number of another list - so the viewer closes onto the view that was
            // asked for. Where these hold it too, it is found in them below.
            let lost = (self.viewer.as_ref())
                .is_some_and(|open| self.index.position_of(open.viewer.id()).is_none());
            if lost {
                self.shut_viewer();
            }
        } else if let Some(landed) = self.selection.follow(&before, &self.index) {
            // The lead's photo has left - hidden, taken out of the album shown, purged by
            // a scan - and the lead is on the photo beside where it was. That photo is
            // brought into sight if the one that left was in sight: a person working
            // through a view goes on from where they were. If it was not, they are
            // somewhere else in the grid, and it stays where they are.
            let seen =
                (self.last.as_ref()).is_some_and(|frame| frame.on_screen.contains(&landed.left));
            if seen {
                self.view.go_to(landed.at, Align::Nearest);
            }
            // A menu of the selection open then was of the photo that left. Its items
            // act on what is selected at the pick, which may now be the photo landed
            // on: one nobody asked about.
            if self.menu.is_some_and(|open| open.of == MenuOf::Selection) {
                self.menu = None;
            }
        }
        // A folder clicked from an excursion: this is the grid it was to be looked up in.
        if let Some(folder) = shown.jump {
            self.go_to_folder(folder);
        }
        // The photo in the viewer is found in the new grid by its id, and what the
        // library says of it is read again: a star, a keyword, an edit made elsewhere.
        // One the grid has lost is asked about the same way - the answer says whether
        // it has left this view or the library (`viewer_answers`).
        if let Some(open) = &mut self.viewer {
            open.viewer.follow(&self.index);
            self.viewer_items.ask(open.viewer.id());
        }
        // The counts read SQLite when they are not cached, so they are asked for.
        self.counting.ask(layout_gen);
        // And today is read again, so that a window left open past midnight catches up
        // the next time anything in the library moves.
        self.today = today(&self.zone);
    }

    /// One frame of the gate's programme, after the frame has been drawn: tells it what
    /// the frame came to and does what it asks before the next.
    fn run_probe(&mut self, ctx: &egui::Context, output: &GridOutput, reported: bool) {
        let Some(run) = &mut self.probe else {
            return;
        };
        if run.probe.done() {
            return;
        }
        let now_ms = ctx.input(|input| input.time) * 1000.0;
        let facts = Facts {
            now_ms,
            max: self.view.max_position(),
            settled: output.settled,
            photos: self.index.len(),
            marked: output.marked,
            outside: run.outside.frame(now_ms, reported),
        };
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
        match run.probe.frame(facts, epoch_ms) {
            Move::To(position) => {
                self.view.move_to(position);
                ctx.request_repaint();
            }
            Move::Wait => ctx.request_repaint(),
            // Nothing is asked for but the frame that ends the rest: what is drawn before
            // it is what the grid draws by itself.
            Move::Rest { for_ms } => {
                ctx.request_repaint_after(Duration::from_secs_f64(for_ms.max(0.0) / 1000.0));
            }
            Move::Done => {
                let written = Written {
                    app: "native",
                    window: ctx.input(|input| input.content_rect().size()).into(),
                    scale: ctx.pixels_per_point(),
                    adapter: self.adapter.as_deref(),
                    // The measurement is over: this read of /proc costs it nothing.
                    memory_bytes: commands::memory_usage().ok().map(|usage| usage.bytes),
                    report: run.probe.report(),
                };
                let json = serde_json::to_string_pretty(&written).unwrap_or_default();
                if let Err(err) = std::fs::write(&run.out, json) {
                    tracing::error!(%err, out = %run.out.display(), "could not write the probe's report");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn close(&mut self) {
        if !std::mem::replace(&mut self.closed, true) {
            // What the user asked to be written is written before the engine goes: a
            // star or a hide pressed in the instant before the window was closed was
            // dropped with the queue, without a word. Not for ever - a write can hang,
            // and the window has to close.
            if !self.writes.finish(WRITES_PATIENCE) {
                tracing::warn!("the window closed before every change was stored");
            }
            self.engine.shutdown();
        }
    }
}

impl eframe::App for App {
    /// What the system says is held over the window and has dropped on it is taken out
    /// of the frame's input before egui reads it. egui asks for the next frame at once
    /// for as long as it is told that something is held (`InputState::
    /// wants_repaint_after`): a folder held still over the window drew a frame at
    /// every refresh, and a drag the system never reports gone - winit on X11, when the
    /// report that a file is held crosses the one that the drag has left - drew them
    /// for the rest of the session. The system's own reports each draw a frame without
    /// that (`egui-winit` asks for one at each), which is all the card needs.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        self.hovered = (input.hovered_files.drain(..))
            .filter(|file| file.path.is_some())
            .count();
        self.fell.extend(
            (input.dropped_files.drain(..)).map(|file| file.path().to_string_lossy().into_owned()),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Whether this frame was asked for from outside the grid: by the engine, which
        // reported, or by the person, whose mouse or keys are in the frame's input.
        let touched = ui.input(|input| !input.events.is_empty());
        // What a notch of the wheel moves is asked again when the window comes back to
        // the front: it is set in another window, the system's own, and photon is not
        // told of the change. And said to egui for the frames after this one, in points:
        // a page is a page of what is in view now.
        let fronted = ui.input(|input| {
            (input.events.iter()).any(|event| matches!(event, egui::Event::WindowFocused(true)))
        });
        if fronted {
            self.notch = (self.wheel)();
        }
        let in_view = ui.ctx().content_rect().height() - TOP_BAR - STATUS_BAR;
        crate::grid::view::set_wheel_notch(ui.ctx(), self.notch.points(f64::from(in_view)));
        // A rubber band owns the window while it is held: Escape abandons it, and
        // every other key is nobody's. Not the grid's keys alone - Ctrl+F put the caret
        // in the search box under a band, where the box then answered the Escape meant
        // for the band, and Ctrl+B built the rows again under its corner. Escape goes
        // with the rest: the band reads it from the key being down, not from its event.
        if self.last.as_ref().is_some_and(|frame| frame.banding) {
            ui.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Key { pressed, .. } => !*pressed,
                    _ => true,
                });
            });
            // Tab egui has read already, from the frame's raw input and before anything
            // is drawn: it would walk the keyboard to the first widget that takes it.
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        let reported = self.take_events() || touched;
        let now = ui.input(|input| input.time) * 1000.0;
        // A task's answer asked for this frame as an engine's report does: the frame is
        // not one the grid drew by itself.
        let reported = self.take_answers(now) || reported;
        self.adopt();
        let reported = self.viewer_answers(ui.ctx()) || reported;
        // The launch's way back, in the frame its first photos are on screen and before
        // they are drawn.
        let (photos, view, sort) = (self.grid.len, self.nav.settled().view, self.nav.sort());
        if let Some(folder) = self.last_folder.restore(photos, view, sort) {
            self.go_to_folder(folder);
        }
        self.take_folder();
        self.take_photo();
        // Nothing reports the moment the engine has started its first scans: it is asked
        // for, a few times a second, for as long as the launch takes and no longer.
        if self.scans.starting() {
            ui.ctx().request_repaint_after(LAUNCH_POLL);
        }
        // A search typed, whose time has come.
        if let Some(query) = self.search.take_due(now) {
            self.search_for(query);
        }

        // What the system says is held over the window, and what was dropped on it
        // (`raw_input_hook`). A pointer that moves or is pressed in the window ends a
        // drag the system still calls held (`FolderDrop::frame`).
        let fell = std::mem::take(&mut self.fell);
        if self.drops {
            let pointer = ui.input(|input| {
                (input.events.iter()).any(|event| {
                    matches!(
                        event,
                        egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }
                    )
                })
            });
            let dropped = self.drop.frame(self.hovered, pointer, fell);
            self.dropped(dropped);
        }

        // A dialog, before anything else is drawn: nothing behind it reads a key, and it
        // takes them out of the frame when it has read its own (`dialog_view.rs`).
        let under_dialog = self.dialog.is_some();
        self.show_dialog(ui);

        // The menu, before anything else is drawn: while it is open the keys are its
        // own, and it takes them out of the frame before the search box, the sidebar
        // or the grid can read them (`menu_view.rs`).
        if let Some(open) = self.menu {
            match self.menu_items(open.of) {
                // What it was the menu of is gone: nothing is selected any more - a
                // grid of other results, or one that has lost the menu's photos and has
                // none to land on - or its album, search or folder is no longer listed.
                None => self.menu = None,
                Some(items) => match self.menu_view.show(ui, open.at, &items) {
                    Outcome::Open => {}
                    Outcome::Closed => self.menu = None,
                    Outcome::Picked(picked) => {
                        self.menu = None;
                        match picked {
                            Picked::Tile(action) => self.pick(action),
                            Picked::Side(action) => self.side(action),
                        }
                    }
                },
            }
        }

        // The viewer takes the whole window: the shell is not drawn under it, and the
        // grid keeps its place, which is a number of its own and not a widget's.
        if self.viewer.is_some() {
            self.show_viewer(ui, now);
            if self.drop.hovering() {
                drop_card::show(ui.ctx(), folder_drop::CARD);
            }
            // A search typed just before the viewer opened is still sent when its time
            // comes: left to the next frame something else asked for, it landed under
            // the viewer whenever the mouse next moved.
            self.ask_for_search(ui.ctx(), now);
            let _ = reported;
            return;
        }

        let data = GridData {
            layout_gen: self.layout_gen,
            index: &self.index,
            folders: self.held.folders(),
            size: self.size,
            zone: &self.zone,
            library: self.nav.settled().view == View::All,
            // The panel that lists what the search box understands lies over the
            // photos, and has no keyboard of its own.
            keys: !self.shell.search_help_open(),
        };
        // The entry marked as the view is where the user is going; the folders listed
        // and the line an empty view shows are of the grid that is on screen.
        self.list.follow(&Sources {
            counts: &self.counts,
            at: &self.nav.target(),
            today: self.today,
            open: self.layout.open,
            sort: self.nav.sort(),
            held: &self.held,
            tallies: self.index.folders(),
            layout_gen: self.layout_gen,
            zone: &self.zone,
            no_folders: self.scans.known() && self.watched.is_empty(),
        });
        // The folder the grid was in when it was last drawn. The grid is drawn after the
        // sidebar and takes its place then, so this is a frame behind, which the end of
        // this frame makes up for when it has to.
        self.marked = self.last.as_ref().and_then(|frame| frame.top_folder);
        let count = self.photo_count();
        let notice = self.notice();
        let lines = self.status();
        let panel = self.panel();
        let (toasts, next_toast) = self.toasts.at(now);
        // A still window draws no frame by itself: the one a message is gone in is asked
        // for.
        if let Some(at) = next_toast {
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ui.ctx().request_repaint_after(wait);
        }
        let searches = &self.held.collections().searches;
        let saved_as =
            saved_search_for(searches, &self.search.text).map(|saved| saved.name.clone());
        let shell = ShellData {
            layout: &self.layout,
            list: &self.list,
            here: self.marked,
            naming: self.rename.field().map(|(of, text)| Naming {
                of,
                text,
                busy: self.rename.busy(),
            }),
            search: SearchBarData {
                saved_as: saved_as.as_deref(),
                can_save: self.search.can_save(searches),
                in_search: self.nav.target().view == View::Search,
            },
            sort: self.nav.sort_target(),
            size: self.size,
            count: count.as_deref(),
            selected: self.selection.len(),
            lines: &lines,
            panel: panel.as_ref(),
            notice: notice.as_deref(),
            toasts,
        };
        let (view, thumbs, selection) = (&mut self.view, &mut self.thumbs, &mut self.selection);
        let mut output = None;
        let actions = self.shell.show(ui, &shell, &mut self.search.text, |ui| {
            output = Some(view.show(ui, &data, thumbs, selection));
        });
        // What lies under a dialog does nothing. egui keeps the pointer from it through
        // the dialog's own ground, which it finds under the pointer only from the frame
        // after the one the dialog is first drawn in: in that first frame a click that
        // was over in it - down and up together, as a long frame hands them over - was
        // still a click on the sidebar's row, and switched the view behind the dialog.
        // And a name left open in the sidebar by a refusal is not sent again by a press
        // on the dialog's own buttons.
        let actions = if under_dialog { Vec::new() } else { actions };
        for action in actions {
            // The other button on a row of the sidebar that has a menu.
            if let Action::RowMenu { what, at } = &action {
                let of = match what {
                    What::Album(id) => Some(MenuOf::Album(*id)),
                    What::Search { id, .. } => Some(MenuOf::Search(*id)),
                    What::Folder(id) => Some(MenuOf::Folder(*id)),
                    _ => None,
                };
                if let Some(of) = of {
                    self.open_menu(ui.ctx(), *at, of);
                }
            }
            self.act(action, now);
        }
        // What the grid's own input asked that is this side's to do.
        let (menu, header_menu, asked, open) =
            (output.as_ref()).map_or((None, None, Vec::new(), None), |output| {
                (
                    output.menu,
                    output.header_menu,
                    output.asked.clone(),
                    output.open,
                )
            });
        if let Some(at) = menu {
            self.open_menu(ui.ctx(), at, MenuOf::Selection);
        }
        // On a folder's header, real or pinned: the menu its row in the sidebar has.
        if let Some((folder, at)) = header_menu {
            self.open_menu(ui.ctx(), at, MenuOf::Folder(folder));
        }
        for ask in asked {
            self.ask(ask);
        }
        // Enter on the photo the keys are on, or a double click: the viewer.
        if let Some(id) = open {
            self.open_viewer(id);
        }
        self.ask_for_search(ui.ctx(), now);

        // The card that says what letting go will do, over everything.
        if self.drop.hovering() {
            let words = match self.dialog {
                Some(Dialog::Export(_)) => folder_drop::CARD_EXPORT,
                _ => folder_drop::CARD,
            };
            drop_card::show(ui.ctx(), words);
        }

        if let Some(output) = &output {
            if let Some(ids) = self.visible.update(&output.on_screen, now) {
                commands::set_visible(&self.engine, ids);
            }
            if let Some(at) = self.visible.due_at() {
                let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
                ui.ctx().request_repaint_after(wait);
            }
        }
        if let Some(output) = &output {
            let view = self.nav.settled().view;
            if let Some(folder) = self.last_folder.at_top(view, output.top_folder) {
                self.remembering.ask(folder);
            }
            // The sidebar was drawn before the grid took its place, marking the folder of
            // the frame before. Nearly always a frame follows by itself - the grid moves
            // on input, on a task's answer or in a scroll, each an immediate request, and
            // egui draws two frames for each of those - but not after a resize: eframe
            // draws one frame for it that nothing asked egui for, and a grid held to a
            // new end in it left the mark a folder behind until the grid's own report of
            // what is in view, a hundred and fifty milliseconds later.
            if output.top_folder != self.marked {
                ui.ctx().request_repaint();
            }
            self.run_probe(ui.ctx(), output, reported);
        }
        self.last = output;
    }

    /// What shows where nothing is painted, for the frame before the first one of ours.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        self.close();
    }
}

/// The viewer while it is open.
struct Viewing {
    viewer: Viewer,
    /// What the library last said of the photo on screen; none until it has answered
    /// for this photo.
    item: Option<ViewerItem>,
    /// The full picture, with what it was made for.
    full: Option<FullPicture>,
    /// Why a picture could not be made, and which.
    failed: Option<(Want, String)>,
    /// A picture the engine made from another state of the photo than the one named:
    /// it is not asked for again until the library has been read again.
    stale: Option<Want>,
    /// The box the photo is fitted into, in pixels, and since when it has been that.
    within: Option<((u32, u32), f64)>,
}

impl Viewing {
    /// The full picture of the photo the viewer is on, as it is now. One held for the
    /// state the photo was in before - it has been turned from elsewhere, and the new
    /// grid has said so before the new picture is made - is another picture, and is
    /// not drawn under what is now said of the photo.
    fn picture(&self) -> Option<&FullPicture> {
        (self.full.as_ref()).filter(|full| full.want == self.viewer.want())
    }
}

/// How long the window's size has to stand before the copy fitted to it is asked for
/// again, in milliseconds.
const FIT_REST_MS: f64 = 200.0;

/// A picture asked for: which, the largest side a texture may have, and the box the
/// photo is fitted into, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PictureAsk {
    want: Want,
    max_side: u32,
    fit: (u32, u32),
}

/// A photo's full picture as textures: its levels, largest first, each with its width
/// in pixels, and the copy scaled to the box it was asked for.
struct FullPicture {
    want: Want,
    /// The box its fitted copy was made for.
    fit: (u32, u32),
    levels: Vec<(egui::TextureHandle, u32)>,
    fitted: Option<egui::TextureHandle>,
}

/// What came of making a picture.
enum Made {
    /// Its levels, largest first, as egui takes them, and the copy fitted to the box.
    Levels {
        levels: Vec<egui::ColorImage>,
        fitted: Option<egui::ColorImage>,
    },
    /// It could not be made, and why.
    Failed(String),
    /// The engine rendered the photo as it is now, which is not the picture that was
    /// asked for: it was edited or rewritten between the asking and the render.
    Stale,
}

/// How many photos on either side of the one in the viewer have their previews warmed.
const PRELOAD: usize = 2;
const GONE: &str = "This photo is no longer available.";
const CANNOT_SHOW: &str = "This photo can't be shown.";
const NO_VIDEO: &str = "The native window does not play videos yet.";

/// Whether the library has said the photo cannot be shown: its thumbnail could not be
/// made. Not of a video, whose thumbnail is its poster frame's.
fn cannot_show(viewer: &Viewer) -> bool {
    (viewer.shown()).is_some_and(|shown| shown.failed && !shown.video)
}

/// What of `item` decides the picture on screen (`viewer::picture_changed`).
fn shown_of(item: &ViewerItem) -> Shown {
    Shown {
        // The key is written in hex for the web view's URLs; one that does not read is
        // no key any picture has.
        key: u64::from_str_radix(&item.thumb_key, 16).unwrap_or(0),
        width: item.width,
        height: item.height,
        orientation: item.orientation,
        failed: item.thumb_state == "failed",
        video: item.kind == photon_core::media::MediaKind::Video,
    }
}

/// Renders the picture `want` names and halves it into its levels, off the UI thread:
/// the decode, the halving and the making of egui's images are each tens of milliseconds
/// for a camera's photo.
fn make_picture(engine: &Engine, asked: PictureAsk) -> Made {
    let picture = match commands::viewer_picture(engine, asked.want.id, false, Some(asked.fit)) {
        Ok(picture) => picture,
        Err(err) => return Made::Failed(err.message),
    };
    if picture.thumb_key != asked.want.key {
        return Made::Stale;
    }
    let image = |width: u32, height: u32, rgba: &[u8]| {
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba)
    };
    let fitted =
        (picture.fitted.as_ref()).map(|fitted| image(fitted.width, fitted.height, &fitted.rgba));
    let full = crate::thumbs::textures::Pixels {
        width: picture.full.width,
        height: picture.full.height,
        rgba: picture.full.rgba,
    };
    let levels = viewer_levels::levels(full, asked.max_side)
        .into_iter()
        .map(|level| image(level.width, level.height, &level.rgba))
        .collect();
    Made::Levels { levels, fitted }
}

/// What a notch of the wheel moves on this system (`scroll::wheel_notch`).
fn system_wheel() -> Notch {
    crate::grid::scroll::wheel_notch(cfg!(target_os = "windows"), crate::wheel::lines())
}

/// Whether copies may be written to the folder whose path is `text`: the folder, with a
/// `~` written out, or what is said under the field. Off the UI thread: it asks the
/// file system, and the library for the watched folders.
fn check_dest(
    engine: &Engine,
    text: &str,
    home: Option<&std::path::Path>,
) -> Result<String, String> {
    // The text as it stands, where that is a folder: a folder's name can end in a
    // space, and a folder dropped on the dialog is put in the field as the system gave
    // it. Read without the spaces at its ends first, as a typed path is, "out " dropped
    // was the folder "out" beside it, and the copies were written there.
    let given = std::path::Path::new(text);
    let path = if given.is_absolute() && given.is_dir() {
        given.to_owned()
    } else {
        // (An empty field is never asked about: `ExportDialog::type_in`.)
        match export_dialog::typed_path(text, home).unwrap_or(Err(export_dialog::NOT_WHOLE)) {
            Ok(path) => path,
            Err(why) => return Err(why.to_owned()),
        }
    };
    // A file passes the engine's own check, which asks only where the path lies, and
    // then fails every photo of the export.
    let dest = path.to_str().filter(|_| path.is_dir());
    let dest = dest.ok_or_else(|| export_dialog::NO_FOLDER.to_owned())?;
    commands::check_export_dest(engine, dest)
        .map(|()| dest.to_owned())
        .map_err(|err| err.message)
}

/// Today in `zone`, as "On this day" means it.
fn today(zone: &TimeZone) -> Today {
    let now = jiff::Timestamp::now().to_zoned(zone.clone());
    Today {
        month: now.month().unsigned_abs().into(),
        day: now.day().unsigned_abs().into(),
    }
}

/// eframe calls `on_exit` when the window closes; a test, or a start that fails after the
/// engine opened, only drops. Either way the engine is shut down once.
impl Drop for App {
    fn drop(&mut self) {
        self.close();
    }
}
