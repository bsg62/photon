//! The application: the engine, the shell around the grid, and the order of a frame.

use crate::{
    compare::{self, Compare},
    compare_view::{CompareAction, CompareData, CompareView, PaneData},
    copies::{self, CopiesOf},
    crop_tool::CropTool,
    dialog_view::{DialogView, Done},
    dialogs::{Confirm, Dialog, Subject},
    dirs::Dirs,
    drop_card,
    empty::{self, GridState, LibraryFacts, Panel, PanelButton},
    events::{Event, UiEvents},
    export_dialog::{self, ExportDialog, Job},
    face_crops::Crops,
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
    nav::{self, Landed, LastFolder, Nav, Place, Step, Target},
    people_page::{self, PeoplePage},
    people_view::{PeopleData, PeopleView},
    person_picker::{self, PersonPicker},
    playback::Playing,
    posters::PosterWorker,
    probe::{Facts, Move, Outside, Probe, Report},
    rename::{Commit, Of, Rename},
    scans::Scans,
    search_bar::{SearchAction, SearchBarData},
    search_box::{SearchBox, saved_search_for},
    selection::Selection,
    settings::{self, Section},
    shell::{Action, Shell, ShellData},
    side_menu::{self, FolderFacts, SideAction},
    sidebar::{
        list::{Collections, Group, Held, List, Sources, What},
        rows::{Counts, Fixed, Today},
        view::{Named, Naming},
    },
    slideshow::Slideshow,
    slideshow_order::{next_still, shuffle_stride},
    status::Line,
    tag_picker::{Mode, Submit, TagPicker},
    tasks::{Latest, Queue},
    theme,
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
    tile_menu::{self, Deciding, TileAction},
    toasts::{Toast, Toasts},
    video::{Failure, Videos},
    viewer::{Shown, Viewer, Want},
    viewer_caption::{self, Captioned},
    viewer_faces::{self, On},
    viewer_info_view::{InfoAction, InfoData, InfoView},
    viewer_layout, viewer_levels,
    viewer_menu::{self, ViewerPick},
    viewer_view::{Cropped, Level, Message, Showing, ViewerAction, ViewerData, ViewerView},
    viewer_zoom::Zoom,
    window_layout::{Layout, STATUS_BAR, TOP_BAR},
};
use eframe::egui;
use jiff::tz::TimeZone;
use photon_core::{
    grid::{GridEntry, GridIndex, GridView as View},
    library::{Folder, GridTile, NamedItems, NamedPerson, ThemeChoice, WatchedFolder},
    media::MediaKind,
    sort::Sort,
};
use photon_engine::{
    commands::{self, FolderList, ViewerItem},
    engine::{Engine, EngineConfig, NOT_BUILT},
    error::AppError,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::mpsc::Receiver, time::Duration};

mod people;
mod settings_dialog;
use settings_dialog::{SettingWrite, SettingWrote};

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
/// How long a closing window waits for the poster worker to answer the job it has: it
/// hears it is to stop within a quarter of a second, whatever it is doing.
const POSTERS_PATIENCE: Duration = Duration::from_secs(1);

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
    /// The people the user has named, for the person dialog: asked for as it opens, and
    /// none until they are read - what the sidebar lists leaves out a person with no
    /// photo in sight, whom a typed name still joins.
    people: Latest<(), Option<Vec<NamedPerson>>>,
    named: Option<Vec<NamedPerson>>,
    /// How many copies the library has of the one photo a tile's menu was opened on:
    /// asked as it opens, and its item there once the answer is in, for that photo alone.
    copy_counts: Latest<i64, (i64, Option<usize>)>,
    menu_copies: Option<(i64, usize)>,
    /// When a dialog last closed or an item of a menu was last picked, on the frame's
    /// clock: what the click that did it may be the first of.
    overlay_closed: Option<f64>,
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
    /// How many crop tools have been opened: each has its number.
    crops: u64,
    /// The window, to be told to go fullscreen and back from wherever that is decided.
    ctx: egui::Context,
    /// The slideshow's interval, in seconds, and whether it shuffles: read once at each
    /// show's start, off this thread.
    show_settings: Latest<(), (Option<i64>, bool)>,
    /// The theme the user chose, as put on the window.
    theme: ThemeChoice,
    /// The theme the system last said the window has (`keep_title_bar`).
    system_theme: Option<egui::Theme>,
    /// What Settings shows of the stored settings, read as it opens.
    settings_read: Latest<(), Result<settings::Stored, String>>,
    /// photon's memory, read every two seconds while Settings shows About, and when it
    /// was last asked for.
    memory_read: Latest<(), Option<settings::Memory>>,
    memory_asked: Option<f64>,
    /// How many people the user has named, read before face detection is switched off.
    face_count: Latest<u64, (u64, Result<i64, String>)>,
    /// Settings' counts of photos for each watched folder and the changes made to
    /// keywords, read as it opens and at every change of the library's data.
    settings_lists: Latest<(), Result<settings_dialog::Lists, String>>,
    /// The library counted for Statistics, with the opening of Settings that asked.
    stats_read: Latest<u64, (u64, Result<photon_core::library::LibraryStats, String>)>,
    /// The path in Settings' field for a folder to add, asked about off this thread:
    /// with the text that was asked about.
    adding_check: Latest<String, (String, Result<String, String>)>,
    /// How many times Settings has been opened: each opening's number.
    settings_opened: u64,
    /// The watched folders whose removal has been asked for, and whether it has been
    /// answered: until the list is read after that answer their rows offer nothing
    /// (`FolderRow::going`).
    removing: Vec<(i64, bool)>,
    /// Compare, while it is open.
    compare: Option<Comparing>,
    /// Compare, its photos read, to be put up at the end of the frame.
    compare_landed: Option<Comparing>,
    compare_view: CompareView,
    /// How many times compare has been asked to open: what a test of "not asked" counts.
    compares_asked: u64,
    /// The photos compare is being opened on, read off this thread: what each is.
    comparing: Latest<Vec<i64>, Result<Vec<compare::Pane>, String>>,
    /// A pane's full picture, one at a time, made once the zoom has left the fit.
    compare_pictures: Latest<PictureAsk, (PictureAsk, Made)>,
    /// What the window was last asked of its fullscreen, by a show or by F11.
    fullscreen_asked: Option<bool>,
    /// The same, until the window has done it.
    fullscreen_pending: Option<bool>,
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
    /// The system's video, which makes the posters and plays: its framework, or a test's
    /// stand-in.
    videos: Arc<dyn Videos>,
    /// The thread that makes them, from the first frame until the window closes - started
    /// there and not here so that a test can say what makes them (`with_videos`).
    poster_worker: Option<PosterWorker>,
    /// The path in the export's field, asked about off this thread: with the text that
    /// was asked about, since the field has been typed in since.
    checking: Latest<String, (String, Result<String, String>)>,
    /// Whether an export applies edits, as stored: read when the export's dialog opens.
    edits: Latest<(), Option<bool>>,
    /// The sound and the loop, carried from one video to the next for the session.
    video_prefs: crate::playback::Prefs,
    /// Whether the system can play a video at all, once the poster worker has asked it.
    video_plays: Arc<std::sync::OnceLock<bool>>,
    /// The file of the video on screen, read off this thread: what its player opens.
    video_paths: Latest<i64, (i64, Option<PathBuf>)>,
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
    /// The grid the counts were read with: what the Copies view says of its photo is
    /// said only beside that grid.
    counted_at: u64,
    /// Whether the band under the photos was drawn with its line the last time the line
    /// was known (`foot_shown`), and whether the last frame drew the band at all.
    footed: bool,
    foot_drawn: bool,
    counting: Latest<u64, (Counts, u64)>,
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
    /// A photo to select and bring into sight in the next frame drawn: a copy located
    /// from the viewer's info panel.
    locating: Option<i64>,
    /// A photo to open in the viewer, looked up in the grid the next frame draws: a
    /// face's, from the People page, in a grid switched to All photos for it.
    opening: Option<i64>,
    /// The People page, while it is on screen (`app/people.rs`).
    page: Option<PeoplePage>,
    people_view: PeopleView,
    people_read: Latest<u64, people::Read>,
    /// The library's data changed since the page last heard of it.
    people_changed: bool,
    face_crops: Crops,
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
    /// The photo in the viewer, and what of it the menu was opened on: a face, by what
    /// it is, or the photo itself. As the tile's menu is of whatever is selected, this
    /// one is of whatever photo the viewer shows and the library has said something
    /// of: its items are built from that in every frame, and its picks act on it.
    Viewer(On),
}

/// What an item of whichever menu is open does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Picked {
    Tile(TileAction),
    Side(SideAction),
    Viewer(ViewerPick),
}

/// Something to write to the library.
enum Write {
    /// A group of the People page named (`commands::name_person`).
    NameGroup {
        group: i64,
        name: String,
    },
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
    /// A keyword put on the photo in the viewer, or taken off it.
    TagAdd {
        id: i64,
        tag: String,
    },
    TagRemove {
        id: i64,
        tag: String,
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
    /// A name from the person dialog, for the face or the photos it was opened for.
    Name(person_picker::Submit),
    /// A quarter turn of the photo in the viewer (`commands::rotate_item`): a number
    /// in `library.db`, added to the edit the photo has. No photo is written; the
    /// picture that follows is the engine's render.
    Turn {
        id: i64,
        clockwise: bool,
    },
    /// The photo's edit, replaced (`commands::set_item_edit`): no turns and no crop is
    /// "Original"; the crop tool's rectangle, with the number its tool was opened
    /// under.
    Edit {
        id: i64,
        turns: u8,
        crop: Option<[u16; 4]>,
        tool: Option<u64>,
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
    /// A setting changed in Settings.
    Setting(SettingWrite),
}

/// A write that was made.
enum Wrote {
    /// A group of the People page named.
    GroupNamed {
        group: i64,
    },
    Search,
    TileSize,
    /// How many stars were written, of how many asked, and of which photos.
    Stars {
        ids: Vec<i64>,
        asked: usize,
        done: usize,
        starred: bool,
    },
    /// A write whose rebuild says everything there is to say: a hide.
    Grid,
    /// An edit stored: the crop tool that sent it, if one did, closes.
    Edited {
        tool: Option<u64>,
    },
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
    /// A face named: under the name the library has for the person, which is an
    /// existing person's own spelling whatever was typed.
    NamedFace {
        name: String,
    },
    /// The face a name was typed for was no longer there: nothing was written, and no
    /// person made.
    FaceGone,
    /// Photos named by their one unnamed face, and why the rest were not.
    NamedItems(NamedItems),
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
    /// A setting of Settings stored, as the library made it.
    Setting(SettingWrote),
}

/// A write the library refused, and what it said.
enum Unwritten {
    /// A group's name refused: in words the user can act on when the group is gone.
    GroupName {
        group: i64,
        said: String,
    },
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
    /// An edit the library would not store: the crop tool stays open, to be put right.
    Edit {
        said: String,
    },
    /// A setting of Settings the library would not store, and what of the dialog it
    /// was sent from waits for the answer.
    Setting {
        refused: settings_dialog::Refused,
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
        // Ctrl with `+`, `-` and `0` scales nothing: egui would zoom the whole window by
        // them unless told, as a web view zooms its page - which the Tauri photon has
        // switched off, and which stands a slip of the hand from the viewer's own keys.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
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
        // And the title bar the system draws, which is the window's own.
        let theme = commands::theme(&engine).unwrap_or_default();
        theme::apply::put(&ctx, theme);
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
                        Step::Copies(id) => commands::set_copies_view(&engine, id),
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
                            ids,
                            done,
                            starred,
                        })
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::Hidden { ids, hidden } => {
                        commands::set_items_hidden(&engine, &ids, hidden)
                            .map(|_| Wrote::Grid)
                            .map_err(|err| Unwritten::Said(err.message))
                    }
                    // The engine rebuilds the grid for a keyword, and the grid's
                    // arrival is what has the viewer read its photo again.
                    Write::TagAdd { id, tag } => commands::add_item_tag(&engine, id, &tag)
                        .map(|_| Wrote::Grid)
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::TagRemove { id, tag } => commands::remove_item_tag(&engine, id, &tag)
                        .map(|()| Wrote::Grid)
                        .map_err(|err| Unwritten::Said(err.message)),
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
                    // The name said afterwards is the library's: an existing person's
                    // own spelling, whatever case was typed, and for a new one the name
                    // as the library cleaned it. Read here, after the write and off the
                    // UI thread; what was typed stands in only if that read fails.
                    Write::Name(person_picker::Submit {
                        target: person_picker::Target::Face(face),
                        name,
                    }) => commands::name_faces(&engine, &[face], &name)
                        .map(|person| match person {
                            Some(person) => Wrote::NamedFace {
                                name: commands::named_people(&engine)
                                    .ok()
                                    .and_then(|people| {
                                        people.into_iter().find(|named| named.id == person)
                                    })
                                    .map_or(name, |named| named.name),
                            },
                            None => Wrote::FaceGone,
                        })
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::Name(person_picker::Submit {
                        target: person_picker::Target::Items(ids),
                        name,
                    }) => commands::name_items(&engine, &ids, &name)
                        .map(Wrote::NamedItems)
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::NameGroup { group, name } => {
                        commands::name_person(&engine, group, &name)
                            .map(|_| Wrote::GroupNamed { group })
                            .map_err(|err| Unwritten::GroupName {
                                group,
                                // The group is gone - a grouping run emptied it, another write
                                // merged it away - and the library's "that is not a named
                                // person" reads as the user's mistake.
                                said: if err.kind == "notAPerson" {
                                    people_page::STALE_GROUP.to_owned()
                                } else {
                                    err.message
                                },
                            })
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
                    // The engine rebuilds the grid for an edit, the photo's key is
                    // another, and the viewer asks for the picture again: nothing here
                    // draws a turn or a crop.
                    Write::Turn { id, clockwise } => commands::rotate_item(&engine, id, clockwise)
                        .map(|()| Wrote::Grid)
                        .map_err(|err| Unwritten::Said(err.message)),
                    Write::Edit {
                        id,
                        turns,
                        crop,
                        tool,
                    } => commands::set_item_edit(&engine, id, turns, crop)
                        .map(|()| Wrote::Edited { tool })
                        .map_err(|err| Unwritten::Edit { said: err.message }),
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
                    Write::Setting(write) => settings_dialog::store(&engine, write),
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
                    let counts = Counts {
                        starred: info.starred_count,
                        duplicates: info.duplicate_count,
                        hidden: info.hidden_count,
                        videos: info.video_count,
                        copies_of: info.copies_of.map(|photo| CopiesOf {
                            id: photo.id,
                            file_name: photo.file_name,
                            gone: photo.gone,
                            hidden: photo.hidden,
                        }),
                    };
                    (counts, info.version)
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
        let comparing = Latest::spawn(
            "compare",
            {
                let engine = engine.clone();
                move |ids: Vec<i64>| {
                    // Every pane before any is shown: half a comparison is worse than none.
                    (ids.iter())
                        .map(|id| {
                            let item =
                                commands::viewer_item(&engine, *id).map_err(|err| err.message)?;
                            Ok(compare::Pane {
                                id: item.id,
                                file_name: item.file_name,
                                width: item.width,
                                height: item.height,
                                orientation: item.orientation,
                                taken_at: item.taken_at,
                                key: u64::from_str_radix(&item.thumb_key, 16).unwrap_or(0),
                                starred: item.starred,
                            })
                        })
                        .collect()
                }
            },
            repaint(&ctx),
        );
        let compare_pictures = Latest::spawn(
            "compare picture",
            {
                let engine = engine.clone();
                move |asked: PictureAsk| (asked, make_picture(&engine, asked))
            },
            repaint(&ctx),
        );
        let show_settings = Latest::spawn(
            "slideshow settings",
            {
                let engine = engine.clone();
                move |()| {
                    let interval = commands::slideshow_interval(&engine).ok();
                    // A failed read plays the view in its own order.
                    let shuffle = commands::slideshow_shuffle(&engine).unwrap_or(false);
                    (interval, shuffle)
                }
            },
            repaint(&ctx),
        );
        let settings_read = Latest::spawn(
            "settings",
            {
                let engine = engine.clone();
                move |()| settings_dialog::read(&engine)
            },
            repaint(&ctx),
        );
        let memory_read = Latest::spawn("memory", |()| settings_dialog::memory(), repaint(&ctx));
        let face_count = Latest::spawn(
            "named count",
            {
                let engine = engine.clone();
                move |opening| settings_dialog::named(&engine, opening)
            },
            repaint(&ctx),
        );
        let settings_lists = Latest::spawn(
            "settings lists",
            {
                let engine = engine.clone();
                move |()| settings_dialog::lists(&engine)
            },
            repaint(&ctx),
        );
        let people_read = Latest::spawn(
            "people page",
            {
                let engine = engine.clone();
                move |number: u64| people::read(&engine, number)
            },
            repaint(&ctx),
        );
        let face_crops = Crops::of(engine.clone(), repaint(&ctx));
        let stats_read = Latest::spawn(
            "statistics",
            {
                let engine = engine.clone();
                move |opening| {
                    let counted = commands::library_stats(&engine).map_err(|err| err.message);
                    (opening, counted)
                }
            },
            repaint(&ctx),
        );
        let adding_check = Latest::spawn(
            "folder to add",
            |text: String| {
                let home = ::dirs::home_dir();
                let answer = settings_dialog::check_folder(&text, home.as_deref());
                (text, answer)
            },
            repaint(&ctx),
        );
        let people = Latest::spawn(
            "named people",
            {
                let engine = engine.clone();
                move |()| commands::named_people(&engine).ok()
            },
            repaint(&ctx),
        );
        let copy_counts = Latest::spawn(
            "copy count",
            {
                let engine = engine.clone();
                move |id: i64| (id, commands::copy_count(&engine, id).ok())
            },
            repaint(&ctx),
        );
        let video_paths = Latest::spawn(
            "video path",
            {
                let engine = engine.clone();
                move |id: i64| (id, commands::item_path(&engine, id).ok())
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
            people,
            named: None,
            copy_counts,
            menu_copies: None,
            overlay_closed: None,
            deleting: Vec::new(),
            viewer: None,
            viewer_view: ViewerView::default(),
            previews: Thumbs::previews(preview_loader),
            viewer_items,
            pictures,
            asking: None,
            asked_pictures: 0,
            crops: 0,
            ctx: ctx.clone(),
            show_settings,
            theme,
            system_theme: None,
            settings_read,
            memory_read,
            memory_asked: None,
            face_count,
            settings_opened: 0,
            removing: Vec::new(),
            settings_lists,
            adding_check,
            stats_read,
            fullscreen_asked: None,
            fullscreen_pending: None,
            compare: None,
            compare_landed: None,
            compare_view: CompareView::default(),
            compares_asked: 0,
            comparing,
            compare_pictures,
            drops,
            drop: FolderDrop::default(),
            hovered: 0,
            fell: Vec::new(),
            wheel: system_wheel,
            notch,
            videos: Arc::new(crate::video::System),
            poster_worker: None,
            checking,
            edits,
            video_plays: Arc::default(),
            video_prefs: Default::default(),
            video_paths,
            exports,
            exporting: false,
            steps,
            search: SearchBox::default(),
            writes,
            stale: false,
            counts: Counts::default(),
            counted: false,
            counted_at: NOT_BUILT,
            footed: false,
            foot_drawn: false,
            counting,
            collecting,
            list: List::default(),
            marked: None,
            going: None,
            showing: None,
            locating: None,
            opening: None,
            page: None,
            people_view: PeopleView::default(),
            people_read,
            people_changed: false,
            face_crops,
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

    /// Videos made posters of and played by `videos` in place of the system's framework:
    /// for a test, which has none of its own to count on.
    pub fn with_videos(mut self, videos: Arc<dyn Videos>) -> Self {
        self.videos = videos;
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
            // Of the photo the viewer is on, as the library last said it is. With
            // nothing said of it - a step has just been made, the photo is gone, the
            // viewer is shut - there is nothing for the items to be of. (No step is made
            // under an open menu: the keys are the menu's, and the viewer is deaf to the
            // wheel and the buttons while one is open.)
            MenuOf::Viewer(on) => (self.viewer.as_ref())
                .and_then(|open| open.item.as_ref())
                .map(|item| {
                    let faces = viewer_faces::actions(&viewer_faces::drawn(item), on);
                    (viewer_menu::viewer_menu(&faces, item.hidden).into_iter())
                        .map(|item| item.map(Picked::Viewer))
                        .collect()
                }),
        }
    }

    /// The viewer, while it is open.
    pub fn viewer(&self) -> Option<&Viewer> {
        self.viewer.as_ref().map(|open| &open.viewer)
    }

    /// The time in the video of the frame the viewer drew in its picture's place in the
    /// last frame; none while it draws its poster, or no video.
    pub fn video_frame(&self) -> Option<f64> {
        let (texture, _, at) = self.viewer.as_ref()?.frame.as_ref()?;
        let (drawn, _) = self.viewer_view.drawn()?;
        (drawn.id() == texture.id()).then_some(*at)
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

    /// The slideshow of the viewer, while the viewer is open.
    pub fn slideshow(&self) -> Option<&Slideshow> {
        self.viewer.as_ref().map(|open| &open.show)
    }

    /// Whether the photo a show has left is still drawn over the one arriving.
    pub fn fading(&self) -> bool {
        (self.viewer.as_ref()).is_some_and(|open| open.outgoing.is_some())
    }

    /// What the window was last asked of its fullscreen.
    pub fn fullscreen_asked(&self) -> Option<bool> {
        self.fullscreen_asked
    }

    /// Compare, while it is open.
    pub fn compare(&self) -> Option<&Compare> {
        self.compare.as_ref().map(|open| &open.compare)
    }

    /// How many times compare has been asked to open since the window was made.
    pub fn compares_asked(&self) -> u64 {
        self.compares_asked
    }

    /// How wide each pane's full picture is, where it has been made.
    pub fn compare_pictures(&self) -> Option<Vec<Option<u32>>> {
        let open = self.compare.as_ref()?;
        Some(
            (open.full.iter())
                .map(|full| {
                    full.as_ref()
                        .and_then(|full| full.levels.first())
                        .map(|(_, width)| *width)
                })
                .collect(),
        )
    }

    /// The crop tool, while it is open on the photo in the viewer.
    pub fn cropping(&self) -> Option<&CropTool> {
        (self.viewer.as_ref()?.crop.as_ref()).map(|crop| &crop.tool)
    }

    /// Where the picture drawn under the crop tool lies in the tool's box, in fractions
    /// of it, while the tool is open.
    pub fn crop_part(&self) -> Option<[f32; 4]> {
        Some(self.viewer.as_ref()?.crop.as_ref()?.part())
    }

    /// The widths of the levels of the picture the crop tool has had made for itself -
    /// the photo turned and not cropped - largest first: none until it is made, and
    /// for a photo that has no crop, whose own picture is that one.
    pub fn crop_picture(&self) -> Option<Vec<u32>> {
        let full = self.viewer.as_ref()?.crop.as_ref()?.picture()?;
        Some(full.levels.iter().map(|(_, width)| *width).collect())
    }

    /// Whether a keyword is being typed into the info panel's field.
    pub fn viewer_typing(&self) -> bool {
        self.viewer.is_some() && self.viewer_view.typing()
    }

    /// Whether the viewer's info panel is open.
    pub fn viewer_info(&self) -> bool {
        self.viewer.as_ref().is_some_and(|open| open.info)
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

    /// Whether the people the person dialog lists have been read since it opened.
    pub fn people_listed(&self) -> bool {
        self.named.is_some()
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

    /// Opens the dialog that names `target`: a face of the photo in the viewer, or the
    /// photos selected in the grid. The people it lists are read as it opens, and none
    /// are listed until then.
    fn open_person(&mut self, target: person_picker::Target) {
        self.named = None;
        self.people.ask(());
        self.open_dialog(Dialog::Person(PersonPicker::new(target)));
    }

    /// Does what an item of the viewer's menu does, to the photo the viewer shows: the
    /// one the items were built from, a moment ago in this frame.
    fn viewer_pick(&mut self, pick: ViewerPick) {
        let shown = (self.viewer.as_ref()).and_then(|open| open.item.as_ref());
        let Some((photo, hidden)) = shown.map(|item| (item.id, item.hidden)) else {
            return;
        };
        match pick {
            ViewerPick::Name(face) => self.open_person(person_picker::Target::Face(face)),
            // The grid's Remove, for the one photo: every face of theirs photon has on
            // it, and the message says when Picasa keeps them there.
            ViewerPick::Not { person, name } => {
                self.writes.push(Write::PersonRemove {
                    person,
                    name,
                    ids: vec![photo],
                });
            }
            ViewerPick::Hide(hidden) => self.write(TileAction::Hide(hidden), vec![photo]),
            // The viewer shuts and the grid lands on the photo: in Hidden when the
            // library says it is hidden, which is the one view that holds it then. Not
            // handed back to the grid under the viewer first, which is being left - or
            // is the one the photo is found in, by the same lookup.
            ViewerPick::Locate => {
                self.shut_viewer();
                self.locate(photo, hidden);
            }
            // What does not work yet takes no press, and so answers nothing.
            ViewerPick::Reveal | ViewerPick::OpenInApp => {}
        }
    }

    /// Opens the menu of `of` at `at`, in the place of any that was open.
    fn open_menu(&mut self, ctx: &egui::Context, at: egui::Pos2, of: MenuOf) {
        self.menu = Some(Open { at, of });
        self.menu_view.reset();
        // The one photo's copies are counted for its menu, a round trip that the menu
        // does not wait for: its item is put in when the answer is in.
        self.menu_copies = None;
        if of == MenuOf::Selection
            && self.selection.len() == 1
            && let Some(id) = self.selection.deciding(&self.index)
        {
            self.copy_counts.ask(id);
        }
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
            Dialog::Confirm(_)
            | Dialog::Export(_)
            | Dialog::Person(_)
            | Dialog::Settings(_)
            | Dialog::Shortcuts => Vec::new(),
        };
        let tags = &self.held.collections().tags;
        let done = (self.dialog_view).show(ui, dialog, &keywords, tags, self.named.as_deref());
        match (done, &mut self.dialog) {
            (Done::Open, _) => {}
            (Done::Closed, _) => {
                if let Some(Dialog::Settings(settings)) = self.dialog.take() {
                    self.settings_closed(*settings);
                }
                self.dialog = None;
            }
            (Done::Settings(asked), Some(Dialog::Settings(_))) => {
                self.settings_asked(ui.ctx(), asked);
            }
            (Done::Typed(text), Some(Dialog::Tags(picker))) => picker.type_in(text),
            (Done::Typed(text), Some(Dialog::Person(picker))) => picker.type_in(text),
            // Closed before its write, as the keyword picker is. The name is written as
            // it is typed: the library is what knows whose it is, whatever its case.
            (Done::Submit { tag, typed }, Some(Dialog::Person(picker))) => {
                if let Some(text) = typed {
                    picker.type_in(text);
                }
                if let Some(submit) = picker.submit(tag.as_deref()) {
                    self.dialog = None;
                    self.writes.push(Write::Name(submit));
                }
            }
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
                | Done::Export
                | Done::Settings(_),
                _,
            ) => {}
        }
    }

    /// Whether Settings' field for a folder to add is on screen and takes a path: shown -
    /// it stays open when another section is shown, and a drop then went into a field
    /// nobody could see - and not adding one already, when it takes nothing.
    fn add_field_shown(&self) -> bool {
        matches!(&self.dialog, Some(Dialog::Settings(settings))
            if settings.section == Section::Folders
                && settings.adding.as_ref().is_some_and(|adding| !adding.busy()))
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
        // Under Settings' field for a folder to add, the first path is put in the field,
        // to be added as a typed one is: the field is asking for a folder, and that is
        // what the drop answers.
        if self.add_field_shown()
            && let Some(Dialog::Settings(settings)) = &mut self.dialog
            && let Some(adding) = &mut settings.adding
        {
            if let Some(ask) = adding.type_in(first.clone()) {
                self.adding_check.ask(ask);
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
            info: false,
            crop: None,
            show: Slideshow::default(),
            shuffle: None,
            outgoing: None,
            playing: None,
            frame: None,
            path: None,
            seen: None,
            path_asked: None,
            open_failed: None,
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

    /// The video on screen, played: its player opened once its file is known, the system
    /// is known to play and the viewer has rested on it, let go of when the viewer leaves
    /// that picture of it, and its newest frame written into its texture. A frame is asked
    /// for while it plays.
    fn play_video(
        &mut self,
        ctx: &egui::Context,
        open: &mut Viewing,
        (id, key): (i64, u64),
        rested: bool,
    ) {
        match self.video_paths.answer() {
            Some(Ok((of, path))) => {
                let why = || "the library has no file for it".to_owned();
                open.path = Some((of, path.ok_or_else(why)));
            }
            // A read that panicked is an answer for the video it was asked of.
            Some(Err(_)) => {
                if let Some(of) = open.path_asked {
                    open.path = Some((of, Err("the library could not be read".into())));
                }
            }
            None => {}
        }
        if !open.viewer.is_video() || open.viewer.gone() {
            open.playing = None;
            open.frame = None;
            return;
        }
        // Another video, or another picture of it: its player is another. And another
        // picture of the same video is another file - the scanner has followed it to a
        // new name, or it was rewritten - whose path is read again.
        if (open.playing.as_ref()).is_some_and(|playing| !playing.is_of(id, key)) {
            open.playing = None;
            open.frame = None;
        }
        if open.seen.is_some_and(|(of, at)| of == id && at != key) {
            open.path = None;
            open.path_asked = None;
        }
        open.seen = Some((id, key));
        let crashed = (open.item.as_ref()).is_some_and(|item| item.video_crashed);
        let refused =
            (open.open_failed.as_ref()).is_some_and(|(of, at, _)| (*of, *at) == (id, key));
        // Not before the viewer has rested on it, as its full picture is not: a held
        // arrow passes videos faster than a player opens, and each one opened and let go
        // of is work on this thread, and a moment of its sound.
        if open.playing.is_none() && !crashed && !refused && rested {
            match (self.video_plays.get(), open.path.as_ref()) {
                // The system says whether it plays once the poster worker has asked it,
                // a moment after the launch: looked at again until it has.
                (None, _) => ctx.request_repaint_after(Duration::from_millis(100)),
                (Some(false), _) => {}
                (Some(true), Some((of, file))) if *of == id => {
                    let opened = match file {
                        Ok(path) => self.videos.open(path),
                        Err(why) => Err(Failure::Broken(why.clone())),
                    };
                    match opened {
                        Ok(stream) => {
                            open.playing = Some(Playing::start(id, key, stream, self.video_prefs))
                        }
                        Err(failure) => open.open_failed = Some((id, key, failure)),
                    }
                }
                (Some(true), _) if open.path_asked != Some(id) => {
                    open.path_asked = Some(id);
                    self.video_paths.ask(id);
                }
                (Some(true), _) => {}
            }
        }
        let Some(playing) = open.playing.as_mut() else {
            return;
        };
        if let Some(picture) = playing.frame() {
            let size = [picture.width as usize, picture.height as usize];
            // Premultiplied: every frame is opaque (`Picture::from_rows`), so it is the
            // same picture, and the unmultiplying pass over every pixel of every frame is
            // spared - a millisecond of this thread a frame at 1080p.
            let image = egui::ColorImage::from_rgba_premultiplied(size, &picture.rgba);
            match (open.frame.as_mut()).filter(|(texture, ..)| texture.size() == size) {
                Some((texture, _, at)) => {
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *at = picture.at;
                }
                None => {
                    let texture =
                        ctx.load_texture("video frame", image, egui::TextureOptions::LINEAR);
                    open.frame = Some((texture, picture.width, picture.at));
                }
            }
        }
        // A video that has failed is shown by its poster, under what is said of it - not
        // by the last frame it drew before it failed.
        if playing.failure().is_some() {
            open.frame = None;
        }
        // The next frame is the video's to say when: asked for at twice sixty a second,
        // so that one is drawn within a refresh of its time, whatever its rate.
        if playing.running() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    /// The viewer is over: what it held goes with it. Its previews too, which nothing
    /// else lets go of but their store's own limit - a hundred megabytes of texture for
    /// the rest of the session, after any stretch of looking at photos.
    fn shut_viewer(&mut self) {
        // A show the viewer is closed under is stopped, and gives the window back.
        let now = self.ctx.input(|input| input.time) * 1000.0;
        if (self.viewer.as_mut()).is_some_and(|open| open.show.stop(now)) {
            self.fullscreen(false);
        }
        self.viewer = None;
        self.previews.clear();
        self.viewer_view.let_go();
        // And the dialog that stands over it, which is of its photo - "Who is this?",
        // of a face on it: shut from under the dialog by a grid of other results that
        // does not hold the photo, the viewer left it standing over the grid, asking
        // about a face on a photo nobody sees. (No other dialog opens over the viewer:
        // the grid's are opened from the grid, and take the keys that would open it.)
        self.dialog = None;
    }

    /// Asks the window to go fullscreen, or to leave it.
    fn fullscreen(&mut self, on: bool) {
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        self.fullscreen_asked = Some(on);
        self.fullscreen_pending = Some(on);
    }

    /// Whether the window is fullscreen, as far as is known: what it was last asked,
    /// until it has done it, and then what it says. egui reports what the window last
    /// said, and a compositor says it a configure later: a show stopped and started
    /// again before the window had left fullscreen took the window for one the user had
    /// made fullscreen, played windowed, and gave nothing back.
    fn window_fullscreen(&self, ctx: &egui::Context) -> Option<bool> {
        self.fullscreen_pending
            .or_else(|| ctx.input(|input| input.viewport().fullscreen))
    }

    /// Stops the slideshow that is running, and gives the window back its own
    /// fullscreen - or none.
    fn stop_show(&mut self, open: &mut Viewing, now: f64) {
        if open.show.stop(now) {
            self.fullscreen(false);
        }
        open.outgoing = None;
        open.shuffle = None;
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
        // A photo located is selected as well, alone: it is what was asked for.
        if let Some(id) = self.locating.take()
            && let Some(offset) = self.index.position_of(id)
        {
            self.selection.select(&self.index, offset);
            self.view.go_to(offset, Align::Nearest);
        }
    }

    /// What the tasks have answered about the photo in the viewer. Each answer counts
    /// for the photo it was asked of, and for the picture it was asked for: the user
    /// may have stepped on, or the photo been edited, since.
    fn viewer_answers(&mut self, ctx: &egui::Context, now_ms: f64) -> bool {
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
                            // Another picture: what was made is of the one before,
                            // and so is a rectangle being drawn on it.
                            open.full = None;
                            open.crop = None;
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
        // The show's settings, for the show that is running: a show stopped since asks
        // nothing of them, and one started since asked again, which `Latest` answers
        // alone.
        if let Some(answer) = self.show_settings.answer() {
            answered = true;
            if let (Ok((interval, shuffle)), Some(open)) = (answer, &mut self.viewer) {
                open.show.interval(interval, now_ms);
                // No stride drawn yet: none for a view of no length, which a show never
                // walks, and one `shuffle_stride` never keeps.
                if shuffle {
                    open.shuffle = Some((0, 1));
                }
            }
        }
        // Compare opens when the library has said what each of its photos is; refused,
        // the library's words say why.
        if let Some(answer) = self.comparing.answer() {
            answered = true;
            match answer {
                Ok(Ok(panes)) => {
                    let count = panes.len();
                    // Put up at the end of the frame (`ui`), over the grid alone.
                    self.compare_landed = Compare::open(panes).map(|compare| Comparing {
                        compare,
                        full: (0..count).map(|_| None).collect(),
                        failed: vec![false; count],
                        asking: None,
                    });
                }
                Ok(Err(said)) => self.toasts.error(said, now_ms),
                Err(_) => self
                    .toasts
                    .error("photon could not compare those photos.", now_ms),
            }
        }
        // A pane's full picture, kept for the pane it was made for while compare stands.
        if let Some(answer) = self.compare_pictures.answer() {
            answered = true;
            if let (Ok((asked, made)), Some(open)) = (answer, &mut self.compare) {
                open.asking = None;
                let pane = (open.compare.panes().iter())
                    .position(|pane| pane.id == asked.want.id && pane.key == asked.want.key);
                if let Some(n) = pane {
                    match made {
                        Made::Levels { levels, .. } => {
                            let options = egui::TextureOptions::LINEAR;
                            let levels = (levels.into_iter().enumerate())
                                .map(|(level, image)| {
                                    let width = image.size[0] as u32;
                                    let name = format!("compare-{}-{level}", asked.want.id);
                                    (ctx.load_texture(name, image, options), width)
                                })
                                .collect();
                            open.full[n] = Some(FullPicture {
                                want: asked.want,
                                fit: asked.fit,
                                levels,
                                fitted: None,
                            });
                        }
                        Made::Failed(_) | Made::Stale => open.failed[n] = true,
                    }
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
                        let of = if asked.uncropped {
                            "uncropped"
                        } else {
                            "viewer"
                        };
                        let levels = (levels.into_iter().enumerate())
                            .map(|(n, image)| {
                                let width = image.size[0] as u32;
                                let name = format!("{of}-{}-{n}", want.id);
                                (ctx.load_texture(name, image, options), width)
                            })
                            .collect();
                        let fitted = fitted.map(|image| {
                            ctx.load_texture(format!("{of}-{}-fitted", want.id), image, options)
                        });
                        let picture = FullPicture {
                            want,
                            fit: asked.fit,
                            levels,
                            fitted,
                        };
                        // The picture without its crop is the crop tool's, and nobody's
                        // once the tool has closed.
                        if !asked.uncropped {
                            open.full = Some(picture);
                        } else if let Some(crop) = &mut open.crop {
                            crop.uncropped = Some(picture);
                        }
                    }
                    // The tool stands on the picture it has, in the place its crop
                    // keeps, and why there is no more of it is said.
                    Made::Failed(said) if asked.uncropped => {
                        if let Some(crop) = &mut open.crop {
                            crop.failed = true;
                            self.toasts.error(said, now_ms);
                        }
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
    ///
    /// `under_dialog`: a dialog stood over the window as the frame began. It may have
    /// closed in this very frame, and the frame is its all the same - as it is to the
    /// shell, which goes by the same word.
    fn show_viewer(&mut self, ui: &mut egui::Ui, now: f64, under_dialog: bool) {
        let Some(mut open) = self.viewer.take() else {
            return;
        };
        let ctx = ui.ctx().clone();
        // Tab is nobody's here. No control of the viewer takes the keyboard, but a
        // message drawn over it has a button that does, and with the keyboard on that
        // the arrows walk it on instead of stepping, and the grid's keys are dead when
        // the viewer has closed. egui has read Tab already, from the frame's raw input:
        // its walk is called off, as under a menu. Not under a dialog, whose keys walk
        // it: Tab egui has settled by now, among the dialog's own widgets, but an arrow
        // pressed on one of its buttons - a person's row, reached by Tab - walks to the
        // next at the end of the frame, and called off here that walk went nowhere:
        // over the viewer the arrows did nothing in a dialog where over the grid they
        // step through its rows.
        let under_dialog = under_dialog || self.dialog.is_some();
        if !under_dialog {
            ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        // Held while a dialog or the viewer's menu is over it, and while a keyword is
        // being typed in the info panel - whose field keeps the keyboard across a step,
        // and would write the rest of a word to a photo nobody typed it for. Asked
        // before the countdown is: a menu asked for in the frame before its end holds it.
        let paused = under_dialog || self.menu.is_some();
        open.show.hold(paused || self.viewer_view.typing(), now);
        // A show's countdown has ended: it goes on to the next photo of its order. Here,
        // before anything of the photo is asked or drawn, as a step by hand is acted on
        // after its frame.
        if open.show.due(now) && self.show_step(&mut open, 1, now) {
            self.left(&mut open);
        }
        let (id, key, want) = (open.viewer.id(), open.viewer.key(), open.viewer.want());
        // A show does not wait for the rest a step by hand waits for: nothing is being
        // stepped past, and the countdown waits for the picture.
        let rested = open.viewer.rested(now) || open.show.active();

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
        // (The box is the photo's area, which the open info panel takes its room from:
        // the copy is of the size the photo is drawn at beside it.)
        let window = ctx.content_rect();
        let area = viewer_layout::photo_area(
            viewer_layout::Frame {
                left: window.left(),
                top: window.top(),
                width: window.width(),
                height: window.height(),
            },
            open.info,
        );
        let scale = ctx.pixels_per_point();
        let within = (
            (area.width * scale).round() as u32,
            (area.height * scale).round() as u32,
        );
        if open.within.is_none_or(|(seen, _)| seen != within) {
            open.within = Some((within, now));
        }
        let stood = open.within.map_or(now, |(_, since)| since);
        // Whether the photo can be turned and cropped: the library has said what it
        // is (nothing is known of one that is gone), it is in the view still, it is not
        // a video, and it is a picture that can be shown and was made - the Svelte
        // viewer's `editable`. And the crop tool is of such a photo alone: it closes
        // over one that has gone, has left the view or was stepped from. And over one
        // that is another picture: by the grid's key at once, which is a round trip
        // before the library says so (`took`, for a picture changed under the key it
        // had) - meanwhile the tool would ask for a picture of the new state that only
        // it draws, and that its own closing then throws away.
        let editable = open.item.is_some()
            && !open.viewer.is_video()
            && !open.viewer.orphaned()
            && !failed
            && !(open.failed.as_ref()).is_some_and(|(of, _)| *of == want);
        if !editable || open.crop.as_ref().is_some_and(|crop| crop.key != key) {
            open.crop = None;
        }
        // Under the crop tool of a photo that has a crop, the picture wanted is the
        // one turned and not cropped, which the rectangle is drawn on: the picture on
        // hand is of what the crop keeps, and stands in its place meanwhile. One
        // question at a time, as ever, and it is this one while the tool is open. (A
        // photo with no crop has that picture already.)
        let uncropped = (open.crop.as_ref()).is_some_and(|crop| crop.had.is_some());
        let ask = PictureAsk {
            want,
            uncropped,
            max_side: ctx.input(|input| input.max_texture_side) as u32,
            fit: within,
        };
        // Whether to ask is decided from what is held and what is on its way, never
        // from what was once asked: a picture let go of is asked for again - one photo
        // on and straight back inside the rest, a photo that was gone and is here
        // again - where "asked for once" left the photo at its preview for good.
        let coming =
            (self.asking).filter(|asked| asked.want == want && asked.uncropped == uncropped);
        let (held, refused) = match open.crop.as_ref().filter(|_| uncropped) {
            Some(crop) => (crop.picture().map(|full| full.fit), crop.failed),
            None => (
                open.picture().map(|full| full.fit),
                (open.failed.as_ref()).is_some_and(|(of, _)| *of == want),
            ),
        };
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

        // The show counts the photo from when its full picture is shown, or cannot be:
        // a slow decode is not counted against it. The photo it left fades from then.
        let shown = open.picture().is_some()
            || (open.failed.as_ref()).is_some_and(|(of, _)| *of == want)
            || failed
            || open.viewer.gone();
        if open.show.active() && shown {
            open.show.shown(now);
            if let Some(outgoing) = open.outgoing.as_mut().filter(|out| out.since.is_none()) {
                outgoing.since = Some(now);
            }
        }
        // The pointer moving brings the controls back, and nothing else does, as in the
        // Svelte viewer: a press or a key that woke them would be answered by the
        // controls it woke - the second click of a double click on the photo where the
        // cross stood, at rest, closed the viewer.
        let poked = ctx.input(|input| {
            (input.events.iter()).any(|event| matches!(event, egui::Event::PointerMoved(_)))
        });
        if poked {
            open.show.poke(now);
        }
        if let Some(at) = open.show.next(now) {
            ctx.request_repaint_after(Duration::from_secs_f64(((at - now) / 1000.0).max(0.0)));
        }
        let seen = (open.outgoing.as_ref()).map(|out| {
            out.since
                .map_or(1.0, |since| 1.0 - (now - since) / CROSSFADE_MS)
        });
        if seen.is_some_and(|seen| seen <= 0.0) {
            open.outgoing = None;
        } else if seen.is_some_and(|seen| seen < 1.0) {
            ctx.request_repaint();
        }
        self.play_video(&ctx, &mut open, (id, key), rested);
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
            // Over its poster, which stays.
            let item = open.item.as_ref();
            let failure = (open.playing.as_ref().and_then(Playing::failure))
                .or(open.open_failed.as_ref().map(|(_, _, failure)| failure));
            crate::playback::said(&crate::playback::Why {
                crashed: item
                    .filter(|item| item.video_crashed)
                    .map(|item| item.thumb_error.as_deref()),
                plays: self.video_plays.get().copied(),
                failure,
                linux: cfg!(target_os = "linux"),
            })
            .map(|said| (said, true))
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
        // Under the crop tool, the picture turned and not cropped once it is made;
        // until then the photo's own, in the part of the tool's box its crop keeps.
        let tools = open.crop.as_ref().and_then(|crop| crop.picture());
        let under = tools.or(open.picture());
        let part = (open.crop.as_ref()).map_or([0.0, 0.0, 1.0, 1.0], Cropping::part);
        let part =
            egui::Rect::from_min_max(egui::pos2(part[0], part[1]), egui::pos2(part[2], part[3]));
        let fitted = under.and_then(|full| full.fitted.clone());
        // A video's frame is its picture, drawn as a photo's is, fitted; until its first,
        // its poster - the preview - stands in its place.
        let levels: Vec<Level<'_>> = match open.frame.as_ref() {
            Some((texture, width, _)) => vec![Level {
                texture,
                width: *width,
            }],
            None => (under.iter())
                .flat_map(|full| full.levels.iter())
                .map(|(texture, width)| Level {
                    texture,
                    width: *width,
                })
                .collect(),
        };
        let len = self.index.len();
        let collections = self.held.collections();
        let faces = (open.item.as_ref()).map_or(Vec::new(), viewer_faces::drawn);
        let data = ViewerData {
            levels: &levels,
            fitted: fitted.as_ref(),
            preview: preview.as_ref(),
            message: (said.as_ref()).map(|(text, over)| Message { text, over: *over }),
            caption: &caption,
            photo_caption: photo_caption.as_deref(),
            // (Nothing is known of a photo that is gone: its facts went with it.)
            starred: open.item.as_ref().map(|item| item.starred),
            // As it is seen: turned by its orientation, and by its edit.
            width: (open.item.as_ref()).map_or(0, |item| {
                viewer_caption::oriented(item.width, item.height, item.orientation).0
            }),
            video: open.viewer.is_video(),
            // A show wraps round the view's ends.
            can_prev: open.viewer.can_step(-1, len) || (open.show.active() && len > 1),
            can_next: open.viewer.can_step(1, len) || (open.show.active() && len > 1),
            editable,
            edited: (open.item.as_ref()).is_some_and(|item| item.edit.is_some()),
            info_open: open.info,
            info: (open.item.as_ref()).map(|item| InfoData {
                item,
                albums: &collections.albums,
                finds_faces: self.engine.face_detection(),
                looking: self.scans.looking_for_faces(),
                zone: &self.zone,
            }),
            faces: &faces,
            // A dialog, or the viewer's own menu: both are shown before the viewer in a
            // frame, and neither takes the wheel out of it.
            paused,
            can_show: open.item.is_some(),
            show: open.show.active().then(|| Showing {
                playing: open.show.playing(),
                quiet: open.show.idle(now),
            }),
            outgoing: (open.outgoing.as_ref())
                .map(|out| (&out.texture, out.at, seen.unwrap_or(1.0) as f32)),
            strip: open.playing.as_ref().and_then(Playing::strip),
        };
        // The zoom is the view's to change in this frame, and the viewer's to keep:
        // before what was done in the frame is acted on, since a step puts it back to
        // the fit.
        let mut zoom = *open.viewer.zoom();
        // And so is the crop tool's rectangle, while the tool is open.
        let mut tool = open
            .crop
            .as_ref()
            .map(|crop| (crop.tool, crop.size, crop.sent));
        let cropped = tool.as_mut().map(|(tool, size, sent)| Cropped {
            tool,
            size: *size,
            part,
            busy: *sent,
        });
        let mut actions = self.viewer_view.show(ui, &data, &mut zoom, cropped);
        drop(levels);
        drop(faces);
        *open.viewer.zoom_mut() = zoom;
        if let (Some(crop), Some((tool, ..))) = (&mut open.crop, tool) {
            crop.tool = tool;
        }
        // What lies under a dialog does nothing, here as in the shell: egui keeps the
        // pointer from it only from the frame after the dialog is first drawn, and a
        // click that was over within that first frame was still a button's.
        if under_dialog {
            actions.clear();
        }

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

        // What the info panel writes is written first, and to the photo the panel was
        // drawn for: a keyword and an album's tick, on the queue of writes. A step in
        // the same frame is acted on below, and by the viewer's photo these went to the
        // one stepped to, whose panel nobody had seen - an arrow key repeats thirty
        // times a second - and behind the Escape that closes they were not written at
        // all. What the panel shows of them is what the library says once they are
        // made: nothing is shown ahead.
        if let Some(id) = open.item.as_ref().map(|item| item.id) {
            for action in &actions {
                match action {
                    ViewerAction::Panel(InfoAction::AddTag(tag)) => {
                        let tag = tag.clone();
                        self.writes.push(Write::TagAdd { id, tag });
                    }
                    ViewerAction::Panel(InfoAction::RemoveTag(tag)) => {
                        let tag = tag.clone();
                        self.writes.push(Write::TagRemove { id, tag });
                    }
                    ViewerAction::Panel(InfoAction::Album { id: album, on }) => {
                        let (album, ids) = (*album, vec![id]);
                        self.writes.push(if *on {
                            Write::AlbumAdd { album, ids }
                        } else {
                            Write::AlbumRemove { album, ids }
                        });
                    }
                    _ => {}
                }
            }
        }
        // The menu that was asked for, and the face that was pressed to be named: each
        // opened once the frame's steps are acted on.
        let mut menu = None;
        let mut naming = None;
        for action in &actions {
            match action {
                ViewerAction::Menu { at, on } => menu = Some((*at, MenuOf::Viewer(*on))),
                ViewerAction::NameFace(face) => naming = Some(*face),
                _ => {}
            }
        }

        let mut closed = false;
        // A link of the info panel that leaves the viewer, to follow once it is shut.
        let mut leaving = None;
        for action in actions {
            let moved = match action {
                ViewerAction::Close => {
                    closed = true;
                    break;
                }
                // In a show, a step by hand follows the show's order, skipping videos
                // and wrapping, and the photo left fades as at the countdown's end.
                ViewerAction::Step(dir) if open.show.active() => {
                    self.show_step(&mut open, dir, now)
                }
                ViewerAction::Step(dir) => open.viewer.step(dir, &self.index, now),
                ViewerAction::Slideshow if open.show.active() => {
                    self.stop_show(&mut open, now);
                    false
                }
                ViewerAction::Slideshow => self.start_show(&mut open, &ctx, shown, now),
                ViewerAction::Pause => {
                    open.show.toggle(now);
                    false
                }
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
                // The second click of a double click on a menu's item or a dialog's
                // row that lay over the picture a moment ago is no click on it: the
                // first did what it was for, and this one paused the video.
                ViewerAction::VideoClicked => {
                    let double =
                        ctx.options(|options| options.input_options.max_double_click_delay);
                    let just_closed =
                        (self.overlay_closed).is_some_and(|at| now - at < double * 1000.0);
                    if !just_closed && let Some(playing) = open.playing.as_mut() {
                        playing.act(crate::playback::Act::Toggle);
                        ctx.request_repaint();
                    }
                    false
                }
                // Done to the player now, in the frame's order: a key that seeks and
                // the arrow after it act on the video they were pressed on.
                ViewerAction::Video(act) => {
                    if let Some(playing) = open.playing.as_mut() {
                        playing.act(act);
                        self.video_prefs = playing.prefs();
                        ctx.request_repaint();
                    }
                    false
                }
                ViewerAction::Info => {
                    open.info = !open.info;
                    false
                }
                // A turn, and every turn and the crop taken off: of the photo on
                // screen, like the star - and like the star not of one stepped to in
                // this frame, of which nothing is known yet.
                ViewerAction::Turn(clockwise) => {
                    if let Some(item) = open.item.as_ref() {
                        let id = item.id;
                        self.writes.push(Write::Turn { id, clockwise });
                    }
                    false
                }
                ViewerAction::Original => {
                    if let Some(item) = open.item.as_ref() {
                        self.writes.push(Write::Edit {
                            id: item.id,
                            turns: 0,
                            crop: None,
                            tool: None,
                        });
                    }
                    false
                }
                // The crop tool opens on the whole picture, fitted to the whole
                // window: the info panel shuts and the zoom is put back.
                ViewerAction::Crop => {
                    // The crop tool stops a show, which would step from under it.
                    self.stop_show(&mut open, now);
                    if let Some(item) = open.item.as_ref() {
                        self.crops += 1;
                        let size = (item.uncropped_width, item.uncropped_height);
                        let had = item.edit.as_ref().and_then(|edit| edit.crop);
                        open.crop = Some(Cropping {
                            tool: CropTool::begin(had, size.0, size.1),
                            key: open.viewer.key(),
                            number: self.crops,
                            sent: false,
                            had,
                            size,
                            uncropped: None,
                            failed: false,
                        });
                        open.info = false;
                        *open.viewer.zoom_mut() = Zoom::default();
                    }
                    false
                }
                // The rectangle, written with the turns the photo has: the tool stays
                // until the library has answered, and takes nothing more meanwhile.
                ViewerAction::CropApply => {
                    // (A second one, while the first is on its way, the view does not
                    // answer: the tool is busy.)
                    if let (Some(item), Some(crop)) = (open.item.as_ref(), open.crop.as_mut()) {
                        crop.sent = true;
                        self.writes.push(Write::Edit {
                            id: item.id,
                            turns: item.edit.as_ref().map_or(0, |edit| edit.turns),
                            crop: crop.tool.wire(),
                            tool: Some(crop.number),
                        });
                    }
                    false
                }
                ViewerAction::CropCancel => {
                    open.crop = None;
                    false
                }
                // (Written above.)
                ViewerAction::Panel(
                    InfoAction::AddTag(_) | InfoAction::RemoveTag(_) | InfoAction::Album { .. },
                ) => false,
                // (Opened below.)
                ViewerAction::Menu { .. } | ViewerAction::NameFace(_) => false,
                ViewerAction::Panel(link) => {
                    leaving = Some(link);
                    break;
                }
            };
            if moved {
                self.left(&mut open);
            }
        }
        self.viewer = Some(open);
        if closed {
            self.close_viewer();
        }
        // A link that leaves the viewer shuts it and goes where a row of the sidebar
        // would: through `go`, which tells the search box. The photo is not handed back
        // to the grid first, where the grid that was under the viewer is being left
        // too. Where it is not - the camera's link in that camera's own search, a
        // person's in their view: `go` asks for no step to where the user is - the
        // viewer closes as it does by its cross, onto the photo it was on.
        if let Some(link) = leaving {
            let step = match &link {
                InfoAction::Search(query) => Some(Step::Search(query.clone())),
                InfoAction::Person(key) => Some(Step::Person(key.clone())),
                InfoAction::ShowCopies(id) => Some(Step::Copies(*id)),
                _ => None,
            };
            if step.as_ref().is_some_and(|step| !self.nav.wants(step)) {
                self.close_viewer();
            } else {
                self.shut_viewer();
            }
            match link {
                // A copy the panel lists is never a hidden photo.
                InfoAction::Locate(id) => self.locate(id, false),
                InfoAction::ShowCopies(id) => self.show_copies(id),
                _ => step.into_iter().for_each(|step| self.go(step)),
            }
        }
        // The menu that was asked for and the face that was pressed are of the photo
        // that was on screen then, and are opened only while it still is: not after a
        // step in the same frame - a held arrow key repeats thirty times a second -
        // which would stand the menu over the next photo as that photo's, and ask "Who
        // is this?" of a face on the photo that was left, over another face in the same
        // place; and not over a viewer the frame has closed. Nothing is known of the
        // photo stepped to until the library has answered, which is what is asked.
        if !(self.viewer.as_ref()).is_some_and(|open| open.item.is_some()) {
            return;
        }
        if let Some((at, of)) = menu {
            self.open_menu(&ctx, at, of);
        }
        // An outline's click a moment after a dialog closed or a menu's item was picked
        // is the second click of a double click on what did it - a person's row, which
        // names at its first click, or "Hide photo" - with a face's outline where the
        // row or the item had been, and asks nothing.
        let double = ctx.options(|options| options.input_options.max_double_click_delay);
        let just_closed = (self.overlay_closed).is_some_and(|at| now - at < double * 1000.0);
        if let Some(face) = naming.filter(|_| !just_closed) {
            self.open_person(person_picker::Target::Face(face));
        }
    }

    /// The viewer has gone on to another photo: what was said and made of the one left
    /// is not its, and a show counts nothing until the new one is shown. And a picture
    /// that could not be made is asked for again when the photo is come back to: its
    /// drive may be there by then.
    fn left(&mut self, open: &mut Viewing) {
        open.item = None;
        open.full = None;
        open.failed = None;
        open.show.changed();
        self.viewer_items.ask(open.viewer.id());
    }

    /// One step of a show, by its countdown or by hand: to the next photo of its order
    /// that is not a video, `dir` along, wrapping. Answers whether the viewer moved: a
    /// view of one photo has nowhere to go. The picture on screen is kept, to fade out
    /// over the one arriving - at fit only, as in the Svelte viewer.
    fn show_step(&mut self, open: &mut Viewing, dir: i64, now: f64) -> bool {
        let len = self.index.len();
        let stride = open.stride(len);
        let index = &self.index;
        let is_photo = |at: usize| {
            (index.rows(at, 1).first()).is_some_and(|entry| entry.kind == MediaKind::Image)
        };
        if len == 0 {
            return false;
        }
        // Whatever stands at the offset of a photo the view has lost - left the view, or
        // the library - is the photo that came after it (`Viewer::step`): going on, the
        // walk begins a place back, so that its first step is that photo and not the one
        // after it.
        let offset = open.viewer.offset();
        let lost = open.viewer.orphaned() || open.viewer.gone();
        let from = if lost && dir > 0 {
            (offset + len - 1) % len
        } else {
            offset.min(len - 1)
        };
        let Some(next) = next_still(from, len, is_photo, dir, stride) else {
            return false;
        };
        // (The view keeps nothing of a zoomed photo. A photo still in its place that is
        // the only one to go to is no step: `go_to` refuses it, and nothing fades.)
        let drawn = self.viewer_view.drawn();
        let moved = open.viewer.go_to(next, &self.index, now);
        if moved {
            open.outgoing = drawn.map(|(texture, at)| Outgoing {
                texture,
                at,
                since: None,
            });
        }
        moved
    }

    /// Starts a show on the photo on screen - or, on a video, on the next photo; with
    /// none in the view, says so and starts nothing. Answers whether the viewer moved.
    fn start_show(
        &mut self,
        open: &mut Viewing,
        ctx: &egui::Context,
        shown: bool,
        now: f64,
    ) -> bool {
        let mut moved = false;
        if open.viewer.is_video() {
            let index = &self.index;
            let is_photo = |at: usize| {
                (index.rows(at, 1).first()).is_some_and(|entry| entry.kind == MediaKind::Image)
            };
            match next_still(open.viewer.offset(), index.len(), is_photo, 1, 1) {
                Some(next) => moved = open.viewer.go_to(next, &self.index, now),
                None => {
                    self.toasts.done(NO_PHOTOS_TO_SHOW, now);
                    return false;
                }
            }
        }
        open.info = false;
        open.shuffle = None;
        self.show_settings.ask(());
        // Shown: the frame's word for the photo it drew, which is the photo the show
        // starts on only if no step of this frame has left it - an arrow before the
        // `S`, after which `left` has forgotten what was said of it. (The step off a
        // video is made here, and `left`, after, makes the show count nothing.)
        let shown = shown && open.item.is_some();
        if open.show.start(now, shown, self.window_fullscreen(ctx)) {
            self.fullscreen(true);
        }
        moved
    }

    /// Asks what each of `ids` is, to open compare on them once the library has said:
    /// two to four photos, and nothing for any other count.
    /// (Asked of the count first: `C` with the whole library selected would otherwise
    /// read every photo of it, to open nothing.)
    fn open_compare(&mut self, ids: Vec<i64>) {
        if compare::can_compare(ids.len()) {
            self.comparing.ask(ids);
            self.compares_asked += 1;
        }
    }

    /// One frame of compare: what it asks the tasks for, what it draws, and what the
    /// user did in it.
    fn show_compare(&mut self, ui: &mut egui::Ui, now: f64, under_dialog: bool) {
        let Some(mut open) = self.compare.take() else {
            return;
        };
        let ctx = ui.ctx().clone();
        // Under a dialog - the shortcut sheet is the one that opens over compare - compare
        // is deaf, as the viewer is: the dialog takes the keys and a button going down out
        // of the frame, not the wheel, and the frames it opens and closes in are its own.
        let under_dialog = under_dialog || self.dialog.is_some();
        // Tab is compare's, between its panes: egui's walk of the keyboard is called off -
        // a message's button over the panes would take it. Not under a dialog, whose own
        // walk it is then.
        if !under_dialog {
            ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        // What the grid on screen says of a pane's photo, where it holds it: its star -
        // written from compare and come back with the rebuild, or changed elsewhere - and
        // its key, another once it is turned or rewritten elsewhere, whose full picture
        // is then made again under the new one.
        let fresh: Vec<(i64, bool, u64)> = (open.compare.panes().iter())
            .filter_map(|pane| {
                (self.entry(pane.id)).map(|entry| (pane.id, entry.starred, entry.thumb_key))
            })
            .collect();
        for (id, starred, key) in fresh {
            open.compare.starred(id, starred);
            if let Some(n) = open.compare.rekey(id, key) {
                open.full[n] = None;
                open.failed[n] = false;
            }
        }
        let wants: Vec<Want> = (open.compare.panes().iter())
            .map(|pane| Want {
                id: pane.id,
                key: pane.key,
                again: 0,
            })
            .collect();
        let previews: Vec<crate::thumbs::loader::Want> = (wants.iter())
            .map(|want| crate::thumbs::loader::Want {
                id: want.id,
                key: want.key,
            })
            .collect();
        self.previews.frame(&ctx, &previews, false);
        // Once zoomed, the full pictures, one after another, the pane with the keys first:
        // fitted, a pane is drawn from its preview, which a zoom soon outgrows.
        if open.compare.wants_full() && open.asking.is_none() {
            let focus = open.compare.focus();
            let next = (std::iter::once(focus).chain(0..wants.len()))
                .find(|n| open.full[*n].is_none() && !open.failed[*n]);
            if let Some(n) = next {
                let ask = PictureAsk {
                    want: wants[n],
                    uncropped: false,
                    max_side: ctx.input(|input| input.max_texture_side) as u32,
                    // A box of nought: no copy is fitted to it (`fitted_copy`), a pane
                    // wanting its full picture only once it is zoomed.
                    fit: (0, 0),
                };
                self.compare_pictures.ask(ask);
                open.asking = Some(ask);
            }
        }
        let shown: Vec<Option<egui::TextureHandle>> = (wants.iter())
            .map(|want| self.previews.texture(want.key).cloned())
            .collect();
        let levels: Vec<Vec<Level<'_>>> = (open.full.iter())
            .map(|full| {
                (full.iter())
                    .flat_map(|full| full.levels.iter())
                    .map(|(texture, width)| Level {
                        texture,
                        width: *width,
                    })
                    .collect()
            })
            .collect();
        let facts = compare::differing_facts(open.compare.panes());
        let data = CompareData {
            panes: (open.compare.panes().iter().enumerate())
                .map(|(n, pane)| PaneData {
                    pane,
                    facts: &facts[n],
                    preview: shown[n].as_ref(),
                    levels: &levels[n],
                })
                .collect(),
            focus: open.compare.focus(),
        };
        let mut zoom = *open.compare.zoom();
        let actions = self.compare_view.show(ui, &data, &mut zoom, under_dialog);
        let actions = if under_dialog { Vec::new() } else { actions };
        drop(data);
        drop(levels);
        *open.compare.zoom_mut() = zoom;

        let (toasts, next_toast) = self.toasts.at(now);
        if let Some(at) = next_toast {
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ctx.request_repaint_after(wait);
        }
        let dismissed = crate::shell::toasts(ui, ctx.content_rect(), toasts, &theme::tokens::DARK);
        if let Some(id) = dismissed {
            self.toasts.dismiss(id);
        }

        let mut opening = None;
        let mut closed = false;
        for action in actions {
            match action {
                CompareAction::Close => closed = true,
                CompareAction::Focus(n) => open.compare.focus_pane(n),
                CompareAction::Next => open.compare.next_pane(),
                CompareAction::Prev => open.compare.prev_pane(),
                // The pane with the keys, as the library has it: not ahead of the write.
                CompareAction::Star => {
                    if let Some(pane) = open.compare.panes().get(open.compare.focus()) {
                        let (ids, starred) = (vec![pane.id], !pane.starred);
                        self.writes.push(Write::Stars { ids, starred });
                    }
                }
                // Opened in the viewer when the grid on screen holds it - the viewer goes
                // through the grid's photos - and otherwise nothing: in Starred, a pane
                // unstarred from compare is no longer there to open.
                CompareAction::Open => {
                    let focused = open.compare.panes().get(open.compare.focus());
                    opening = focused
                        .map(|pane| pane.id)
                        .filter(|id| self.index.position_of(*id).is_some());
                    closed = opening.is_some();
                }
            }
            if closed {
                break;
            }
        }
        if closed {
            self.previews.clear();
            self.compare_view = CompareView::default();
            if let Some(id) = opening {
                self.open_viewer(id);
            }
            return;
        }
        self.compare = Some(open);
    }

    /// Finds a photo in the grid: in All photos, where every photo that is not hidden
    /// is, selected and brought into sight. From anywhere else that is a step to All
    /// photos first, and the photo is looked up in that step's grid (`Nav::jump`);
    /// where the user is there already or on their way, `go` asks for no step, and
    /// drops what was half typed into the search box, which would land after the jump.
    ///
    /// From Hidden too, where a folder asked for is looked up in the grid on screen
    /// (`Nav::home_of_folders`): a copy the panel lists is never a hidden photo, and a
    /// photo unhidden in the viewer stays on screen in Hidden with its copies listed.
    ///
    /// A photo the library says is `hidden` is looked for in Hidden instead, the one
    /// view that holds it: the viewer's own "Locate in photon", on a photo opened from
    /// there or hidden a moment ago.
    fn locate(&mut self, id: i64, hidden: bool) {
        let view = if hidden { View::Hidden } else { View::All };
        self.go(Step::View(view));
        if let Some(to) = self.nav.jump(Target::Photo(id)) {
            self.arrive(to);
        }
    }

    /// The Copies view of a photo, with the photo selected in it and brought into sight,
    /// as the Svelte `showCopies` lands it: the view first, since an offset means
    /// something only in the grid it is looked up in.
    fn show_copies(&mut self, id: i64) {
        self.go(Step::Copies(id));
        if let Some(to) = self.nav.jump(Target::Photo(id)) {
            self.arrive(to);
        }
    }

    /// Goes where a jump leads, in the grid the next frame draws.
    fn arrive(&mut self, to: Target) {
        match to {
            Target::Folder(folder) => self.go_to_folder(folder),
            Target::Photo(id) => self.locating = Some(id),
            Target::Open(id) => self.opening = Some(id),
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
            // Offered for the photo it was counted for, while that is the one photo the
            // menu is of: a menu of several becomes a menu of one when a grid takes the
            // others away, and a count asked for an earlier menu can land in a later one.
            copies: (self.menu_copies)
                .filter(|(id, _)| self.selection.deciding(&self.index) == Some(*id)),
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
            // Compare takes its photos as it opens, and opens when the library has said
            // what each is.
            TileAction::Compare => {
                self.open_compare(ids);
                return;
            }
            TileAction::ShowCopies(id) => {
                self.show_copies(id);
                return;
            }
            // And so does the person dialog, whose title says how many photos.
            TileAction::AddToPerson => {
                self.open_person(person_picker::Target::Items(ids));
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
            // (Opened by `pick`, as dialogs.)
            | TileAction::Compare
            | TileAction::AddKeyword
            | TileAction::RemoveKeyword
            | TileAction::Export
            | TileAction::AddToPerson
            | TileAction::ShowCopies(_) => return,
        };
        self.writes.push(write);
    }

    /// What a key asked of the selection: the menu's item, without the menu - for the
    /// photos that were selected at the key, which the ask carries. The frame's later
    /// keys and clicks have been answered by now, and the selection is theirs.
    fn ask(&mut self, asked: Asked) {
        if asked.ask == Ask::Compare {
            self.open_compare(asked.ids);
            return;
        }
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
            Ask::Compare => TileAction::Compare,
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
            .or_else(|| (self.copies_notice()).filter(|_| empty::show_empty_notice(&self.grid)))
    }

    /// The line under the photos of a Copies view that holds some: that its photo is
    /// hidden, or is the only one left.
    pub fn foot(&self) -> Option<String> {
        (self.copies_notice()).filter(|_| self.grid.len > 0 && empty::grid_built(&self.grid))
    }

    /// The band under the photos as this frame draws it: the line, or - while the line
    /// for the grid on screen is not known yet, a round trip after the grid - the band's
    /// room and no words, if the frame before had it. Every rebuild of the view is a new
    /// grid, a star or a thumbnail as much as a purge: gone and back at each, the band gave
    /// the photos its room and took it again, and a grid scrolled to its end was left a
    /// band short of it. The words are not kept, being of the grid before.
    fn foot_shown(&mut self) -> Option<String> {
        let copies = nav::copies_of(self.nav.settled()).is_some();
        let shown = if copies && self.grid.len > 0 && self.counted_at != self.grid.version {
            self.footed.then(String::new)
        } else {
            let line = self.foot();
            self.footed = line.is_some();
            line
        };
        self.foot_drawn = shown.is_some();
        shown
    }

    /// Whether the last frame drew the band under the photos, with or without its words.
    pub fn foot_drawn(&self) -> bool {
        self.foot_drawn
    }

    /// What the Copies view on screen says of its photo (`copies::notice`), from what
    /// the library said of the photo with this very grid, or nothing: read with the grid
    /// before, a photo just purged was "no other copies any more" for a round trip, of a
    /// photo whose copies are all still there.
    fn copies_notice(&self) -> Option<String> {
        let shown = nav::copies_of(self.nav.settled())?;
        let of = (self.counts.copies_of.as_ref())
            .filter(|of| of.id == shown && self.counted_at == self.grid.version);
        let first = self.index.rows(0, 1).first().map(|entry| entry.id);
        of.and_then(|of| copies::notice(Some(of), self.grid.len, first))
    }

    /// Asks for `step`, unless it leads to where the user already is. What a click on a
    /// row of the sidebar does, and the one way the engine's view is moved.
    ///
    /// The search box goes with it. A search - a saved one's row, On this day - is what
    /// the box then holds. Any other step is a switch away, which clears the engine's
    /// query: a search half typed is dropped, or it would land after the switch and
    /// replace the grid that was asked for, and the box is emptied as the switch is made.
    pub fn go(&mut self, step: Step) {
        // A view asked for is the grid asked for: the People page is left. A sort moves no
        // view (and its controls are not drawn over the page).
        if !matches!(step, Step::Sort(_)) {
            self.leave_people();
        }
        self.ask_step(step);
    }

    /// Asks for `step` as `go` does, and leaves the People page where it is: the step to
    /// All photos that opens a face's photo under it.
    fn ask_step(&mut self, step: Step) {
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
        self.leave_people();
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
        self.leave_people();
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
        if let Some(to) = self.nav.jump(Target::Folder(folder)) {
            self.arrive(to);
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
            // Typing in the box is asking for the grid: it leaves the People page.
            Action::Search(SearchAction::Typed) => {
                self.leave_people();
                self.search.typed(now_ms);
            }
            // Escape in the box empties it and leaves the page where it is, as the Svelte
            // box does: only typing is asking for the grid.
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
            Action::Row(What::PeoplePage) => self.open_people(now_ms),
            // The button under "No folders yet.": Settings' list of folders.
            Action::Row(What::AddFolder) => self.open_settings(Section::Folders),
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
            Action::Panel(PanelButton::WatchedFolders) => self.open_settings(Section::Folders),
            Action::Settings => self.open_settings(Section::Folders),
            // Settings' list of folders, with the field a folder is typed in open.
            Action::Panel(PanelButton::AddFolder) => {
                self.open_settings(Section::Folders);
                if let Some(Dialog::Settings(settings)) = &mut self.dialog {
                    settings.adding = Some(Default::default());
                }
            }
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
            // The People page reads every visible face, at most once a second
            // (`PeoplePage::changed`).
            self.people_changed = true;
            // Settings' counts of photos and its changes to keywords are read again with
            // the sidebar's lists, while it is open.
            if matches!(self.dialog, Some(Dialog::Settings(_))) {
                self.settings_lists.ask(());
            }
        }
        if data_changed || folders_changed {
            self.folder_list.ask(());
        }
        reported
    }

    /// Takes what the tasks have answered since the last frame, and says whether any had.
    fn take_answers(&mut self, now_ms: f64) -> bool {
        let mut answered = self.settings_answers(now_ms);
        if let Some(read) = self.folder_list.answer() {
            answered = true;
            if let Ok(Some(list)) = read {
                self.watched = list.watched;
                self.held.set_folders(list.folders);
                // A removal answered before this list was asked for is in it: its row
                // has gone, and its number is free to be another folder's.
                self.removing.retain(|(_, answered)| !answered);
            }
            // Read or not: the list on hand is what there is to go by. Left as "not
            // read", a read that failed at launch kept an empty library saying nothing.
            self.scans.listed(&self.watched);
        }
        if let Some(counts) = self.counting.answer() {
            answered = true;
            if let Ok((mut counts, version)) = counts {
                self.counted_at = version;
                // The name the view was opened with stays once the library has none.
                counts.copies_of =
                    copies::keep_name(self.counts.copies_of.as_ref(), counts.copies_of);
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
        // The people, for the person dialog: `Latest` answers the last question alone,
        // and each opening asks, having forgotten the list before. A read that failed
        // leaves them not known - not "No one is named yet.", which would be said of a
        // library full of people: a name can be typed all the same, and the library is
        // what decides whose it is.
        if let Some(answer) = self.people.answer() {
            answered = true;
            if let Ok(Some(people)) = answer {
                self.named = Some(people);
            }
        }
        // A failed count is no item, as in the Svelte menu: the other items still work.
        if let Some(answer) = self.copy_counts.answer() {
            answered = true;
            if let Ok((id, Some(count))) = answer {
                self.menu_copies = Some((id, count));
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
                Ok(Ok(Wrote::GroupNamed { group })) => self.group_named(group, true, now_ms),
                Ok(Err(Unwritten::GroupName { group, said })) => {
                    self.toasts.error(said, now_ms);
                    self.group_named(group, false, now_ms);
                }
                Ok(Ok(Wrote::TileSize)) => {}
                Ok(Err(Unwritten::Search { query, said })) => {
                    self.search.save_refused(&query);
                    self.toasts.error(said, now_ms);
                }
                Ok(Err(Unwritten::TileSize { said })) => self.toasts.error(said, now_ms),
                // A star the engine could not write - a folder whose INI is read-only -
                // costs a message, and not the photos beside it.
                Ok(Ok(Wrote::Stars {
                    ids,
                    asked,
                    done,
                    starred,
                })) => {
                    // Compare's panes say what was written - whether or not the grid on
                    // screen still holds the photo, which in Starred it does not once
                    // its star is off. (Compare writes one photo at a time, and a write
                    // of one that fails is a refusal, not fewer done.)
                    if let Some(open) = self.compare.as_mut() {
                        for id in ids {
                            open.compare.starred(id, starred);
                        }
                    }
                    if let Some(said) = tile_menu::stars_missed(asked, done, starred) {
                        self.toasts.error(said, now_ms);
                    }
                }
                Ok(Ok(Wrote::Grid)) => {}
                // The crop tool closes when its rectangle is stored, and only the tool
                // that sent it: one closed and opened again meanwhile is another.
                Ok(Ok(Wrote::Edited { tool })) => {
                    if let Some(open) = &mut self.viewer
                        && open.crop.as_ref().map(|crop| crop.number) == tool
                    {
                        open.crop = None;
                    }
                }
                // Refused, it stands where the rectangle can be put right, and takes
                // an "Apply" again.
                Ok(Err(Unwritten::Edit { said })) => {
                    if let Some(crop) = (self.viewer.as_mut()).and_then(|open| open.crop.as_mut()) {
                        crop.sent = false;
                    }
                    self.toasts.error(said, now_ms);
                }
                Ok(Ok(Wrote::Lists)) => {
                    self.collecting.ask(());
                    // And the photo in the viewer, whose albums its info panel ticks:
                    // the engine rebuilds no grid for an album unless one is on screen,
                    // so nothing else would read it again.
                    if let Some(open) = &self.viewer {
                        self.viewer_items.ask(open.viewer.id());
                    }
                }
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
                // (The lists, and the photo in the viewer, are read again by the rebuild
                // a write of the people announces.)
                Ok(Ok(Wrote::NamedFace { name })) => {
                    (self.toasts).done(person_picker::named_face_message(&name), now_ms);
                }
                Ok(Ok(Wrote::FaceGone)) => self.toasts.done(person_picker::FACE_GONE, now_ms),
                Ok(Ok(Wrote::NamedItems(came))) => {
                    (self.toasts).done(person_picker::named_items_message(&came), now_ms);
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
                        // Under the page, if it is open: the user is not asking for the grid,
                        // and the Svelte delete switches no page.
                        if self.nav.target() == shown {
                            self.ask_step(Step::View(View::All));
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
                Ok(Ok(Wrote::Setting(wrote))) => self.setting_written(wrote),
                Ok(Err(Unwritten::Setting { refused, said })) => {
                    // Said under the field that waits for it, or as a message.
                    if let Some(said) = self.setting_refused(refused, said) {
                        self.toasts.error(said, now_ms);
                    }
                }
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
                // A write that panicked says which it was no more: what the People page
                // hid for writes on their way comes back, as a refusal puts it back, and
                // the page is read again.
                Err(_) => {
                    if let Some(model) = &mut self.page {
                        model.forget_writes();
                    }
                    self.read_people(now_ms);
                    self.toasts
                        .error("photon could not store that change.", now_ms);
                }
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
        // A folder clicked from an excursion, or a photo located from one: this is the
        // grid it was to be looked up in.
        if let Some(to) = shown.jump {
            self.arrive(to);
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
            // Before the engine goes, so that a poster being made is answered as not made
            // this session: left unanswered, its claim is counted at the next launch as
            // a window that died opening the video, and two such deaths fail it for good.
            if let Some(worker) = self.poster_worker.as_mut()
                && !worker.finish(POSTERS_PATIENCE)
            {
                tracing::warn!("the window closed while a video's poster was being made");
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
        if self.poster_worker.is_none() && !self.closed {
            self.poster_worker = Some(PosterWorker::start(
                self.engine.clone(),
                self.videos.clone(),
                self.video_plays.clone(),
            ));
        }
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
        self.keep_title_bar(ui.ctx());
        let in_view = ui.ctx().content_rect().height() - TOP_BAR - STATUS_BAR;
        crate::grid::view::set_wheel_notch(ui.ctx(), self.notch.points(f64::from(in_view)));
        // What the window has asked for and the window has now done is no longer on
        // its way.
        let reported = ui.ctx().input(|input| input.viewport().fullscreen);
        if self.fullscreen_pending.is_some() && self.fullscreen_pending == reported {
            self.fullscreen_pending = None;
        }
        // F11 is the window's fullscreen, everywhere: in the grid, in the viewer, under a
        // menu, a dialog or a band - read before anything else takes the keys out of the
        // frame. At its press: held, it would flicker the window at every repeat.
        let f11 = ui.input(|input| {
            (input.events.iter()).any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::F11,
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })
        });
        if f11 {
            let on = self.window_fullscreen(ui.ctx()) == Some(true);
            self.fullscreen(!on);
        }
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
        let reported = self.people_answers(ui.ctx(), now) || reported;
        self.adopt();
        let reported = self.viewer_answers(ui.ctx(), now) || reported;
        // The launch's way back, in the frame its first photos are on screen and before
        // they are drawn.
        let (photos, view, sort) = (self.grid.len, self.nav.settled().view, self.nav.sort());
        if let Some(folder) = self.last_folder.restore(photos, view, sort) {
            self.go_to_folder(folder);
        }
        self.take_folder();
        self.take_photo();
        self.take_open(now);
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

        // `?`: the list of shortcuts, over whatever it was pressed in - the grid, the
        // viewer, compare - as long as nothing there has a use for the character: a text
        // field, the viewer's keyword, and an open dropdown, whose keys are its own. Not
        // what merely holds the keyboard, which the splitter does after every drag. Not
        // over another dialog nor under a menu, and not under a held band, which has
        // taken every key out of the frame by now - nor over a press on the photos, which
        // would become a band under the list (as Enter and `C` open nothing over it). At
        // its press, and a chord is not this key.
        let pressing = (self.last.as_ref()).is_some_and(|frame| frame.pressing);
        let asks_sheet = self.dialog.is_none()
            && self.menu.is_none()
            && !pressing
            && !ui.ctx().text_edit_focused()
            && !self.shell.lists_open()
            && !(self.cropping().is_some() && self.viewer_view.ratio_open())
            && !self.viewer_typing()
            && ui.input_mut(|input| {
                let mut asked = false;
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::Questionmark,
                        pressed: true,
                        repeat,
                        modifiers,
                        ..
                    } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                        asked |= !repeat;
                        false
                    }
                    _ => true,
                });
                asked
            });
        if asks_sheet {
            self.open_dialog(Dialog::Shortcuts);
        }

        // A dialog, before anything else is drawn: nothing behind it reads a key, and it
        // takes them out of the frame when it has read its own (`dialog_view.rs`).
        let under_dialog = self.dialog.is_some();
        self.settings_frame(ui.ctx(), now);
        self.show_dialog(ui);
        if under_dialog && self.dialog.is_none() {
            self.overlay_closed = Some(now);
        }

        // The menu, before anything else is drawn: while it is open the keys are its
        // own, and it takes them out of the frame before the search box, the sidebar
        // or the grid can read them (`menu_view.rs`).
        if let Some(open) = self.menu {
            // The viewer's menu is dark in both themes, as the viewer is.
            let palette = match open.of {
                MenuOf::Viewer(_) => &theme::tokens::DARK,
                _ => theme::apply::palette(ui.ctx()),
            };
            match self.menu_items(open.of) {
                // What it was the menu of is gone: nothing is selected any more - a
                // grid of other results, or one that has lost the menu's photos and has
                // none to land on - or its album, search or folder is no longer listed,
                // or the viewer shows nothing the library has said anything of.
                None => self.menu = None,
                Some(items) => match (self.menu_view).show_in(ui, open.at, &items, palette) {
                    Outcome::Open => {}
                    Outcome::Closed => self.menu = None,
                    Outcome::Picked(picked) => {
                        self.menu = None;
                        self.overlay_closed = Some(now);
                        match picked {
                            Picked::Tile(action) => self.pick(action),
                            Picked::Side(action) => self.side(action),
                            Picked::Viewer(pick) => self.viewer_pick(pick),
                        }
                    }
                },
            }
        }

        // The keyword's field of the viewer's info panel goes with the viewer, however
        // it was shut - by its own keys at the end of the last frame, or by a grid of
        // other results a moment ago in this one. egui lets go of a field that is no
        // longer drawn only at the end of the frame that did not draw it, so the grid
        // would be deaf to this frame's keys and, having seen the keyboard elsewhere,
        // to the next frame's.
        if self.viewer.is_none() {
            InfoView::let_go(ui.ctx());
        }

        // Compare takes the whole window as the viewer does.
        if self.compare.is_some() {
            self.show_compare(ui, now, under_dialog);
            if self.drop.hovering() {
                drop_card::show(ui.ctx(), folder_drop::CARD);
            }
            return;
        }
        // The viewer takes the whole window: the shell is not drawn under it, and the
        // grid keeps its place, which is a number of its own and not a widget's.
        if self.viewer.is_some() {
            self.show_viewer(ui, now, under_dialog);
            if self.drop.hovering() {
                drop_card::show(ui.ctx(), folder_drop::CARD);
            }
            // A search typed just before the viewer opened is still sent when its time
            // comes: left to the next frame something else asked for, it landed under
            // the viewer whenever the mouse next moved.
            self.ask_for_search(ui.ctx(), now);
            // Compare does not open over the viewer (below).
            self.compare_landed = None;
            let _ = reported;
            return;
        }

        // Before the grid's data borrows the index: it remembers whether it drew the band.
        let foot = self.foot_shown();
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
            copies_marked: copies::marked(self.nav.settled().view),
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
            people: self.page.is_some(),
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
            // The page is drawn in the grid's place, and nothing the grid would say.
            panel: panel.as_ref().filter(|_| self.page.is_none()),
            notice: notice.as_deref().filter(|_| self.page.is_none()),
            grid_controls: self.page.is_none(),
            foot: foot.as_deref().filter(|_| self.page.is_none()),
            toasts,
        };
        let (view, thumbs, selection) = (&mut self.view, &mut self.thumbs, &mut self.selection);
        let progress = self.scans.faces_line();
        let people = self.page.as_ref().map(|model| PeopleData {
            model,
            finds_faces: self.engine.face_detection(),
            progress: progress.as_deref(),
        });
        let (people_view, crops) = (&mut self.people_view, &mut self.face_crops);
        let mut output = None;
        let mut pressed = Vec::new();
        let actions = self
            .shell
            .show(ui, &shell, &mut self.search.text, |ui| match &people {
                Some(people) => pressed = people_view.show(ui, ui.max_rect(), people, crops),
                None => output = Some(view.show(ui, &data, thumbs, selection)),
            });
        let pressed = if under_dialog { Vec::new() } else { pressed };
        for action in pressed {
            self.people_act(action);
        }
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
                Some(Dialog::Settings(_)) if self.add_field_shown() => folder_drop::CARD_ADD,
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
        // Compare, once the library has said what its photos are, goes up after this
        // frame's grid has answered the frame's keys: a key pressed before compare was
        // ever drawn is the grid's, as it was meant. And over the grid alone: a viewer, a
        // menu, a dialog or a held band come since it was asked for - its photos are
        // four reads of the library, which on a slow share take a while - keep the
        // window, and the answer is dropped.
        let free = self.viewer.is_none()
            && self.menu.is_none()
            && self.dialog.is_none()
            && self.page.is_none();
        let banding = self.last.as_ref().is_some_and(|frame| frame.banding);
        if let Some(landed) = self.compare_landed.take()
            && free
            && !banding
        {
            self.compare = Some(landed);
        }
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
    /// The player of the video on screen, from when its file is known until the viewer
    /// leaves that picture of it.
    playing: Option<Playing>,
    /// Its newest frame as a texture, written in place frame by frame, with its width
    /// and the time in the video it shows.
    frame: Option<(egui::TextureHandle, u32, f64)>,
    /// The video's file, or why there is none, once read: by id, as the viewer may have
    /// stepped since it was asked for.
    path: Option<(i64, Result<PathBuf, String>)>,
    /// The video and picture of the last frame: a new picture of the same video is a new
    /// file, whose path is read again.
    seen: Option<(i64, u64)>,
    /// The video whose file was last asked for: asked once, not in every frame.
    path_asked: Option<i64>,
    /// A player that could not be opened, and of what; it is not opened again for that.
    open_failed: Option<(i64, u64, Failure)>,
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
    /// Whether the info panel is open: the viewer's while it is, closed at every
    /// opening, and kept across a step.
    info: bool,
    /// The crop tool, while it is open.
    crop: Option<Cropping>,
    /// The slideshow, running or not.
    show: Slideshow,
    /// A shuffled show's stride, and the view's length it was drawn for; none for a
    /// show in the view's own order.
    shuffle: Option<(usize, usize)>,
    /// The photo the show has left, drawn over the one arriving until it has faded.
    outgoing: Option<Outgoing>,
}

/// Compare while it is open: the panes, their full pictures as they are made, which
/// could not be, and the one on its way.
struct Comparing {
    compare: Compare,
    full: Vec<Option<FullPicture>>,
    failed: Vec<bool>,
    asking: Option<PictureAsk>,
}

/// The picture a show has just left, as it was drawn, and since when it fades: from
/// when the photo arriving is shown.
struct Outgoing {
    texture: egui::TextureHandle,
    at: egui::Rect,
    since: Option<f64>,
}

/// How long the photo a show leaves takes to fade, in milliseconds.
const CROSSFADE_MS: f64 = 600.0;
const NO_PHOTOS_TO_SHOW: &str = "There are no photos here to show.";

/// A number from nought up to one, a different one each time: what a shuffled show's
/// stride is drawn by. From the hasher the standard library seeds afresh, which is all
/// the chance this needs and no dependency.
fn random() -> f64 {
    use std::hash::BuildHasher;
    let bits = std::collections::hash_map::RandomState::new().hash_one(std::time::Instant::now());
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

/// The crop tool, open on the photo in the viewer.
struct Cropping {
    tool: CropTool,
    /// The key of the picture it was opened on.
    key: u64,
    /// The number it was opened under: the answer to its write counts for it alone.
    number: u64,
    /// Its rectangle has been sent to be stored, and the answer is not here yet.
    sent: bool,
    /// The crop the photo had as the tool opened, where it had one: its own picture is
    /// then of that part only.
    had: Option<[u16; 4]>,
    /// The size in pixels of the picture the rectangle is drawn on: turned, and not
    /// cropped.
    size: (u32, u32),
    /// That picture, where the photo's own is not it.
    uncropped: Option<FullPicture>,
    /// It could not be made.
    failed: bool,
}

impl Cropping {
    /// The picture turned and not cropped. (Of the photo as it is now: the tool closes
    /// when the photo is another picture.)
    fn picture(&self) -> Option<&FullPicture> {
        self.uncropped.as_ref()
    }

    /// Where the picture drawn under the tool lies in the tool's box, in fractions of it:
    /// all of it, or - while the photo's own picture stands in for the one without its
    /// crop - the part the crop keeps.
    fn part(&self) -> [f32; 4] {
        match (self.picture(), self.had) {
            (None, Some(crop)) => {
                crop.map(|edge| f32::from(edge) / photon_core::edit::CROP_UNIT as f32)
            }
            _ => [0.0, 0.0, 1.0, 1.0],
        }
    }
}

impl Viewing {
    /// How far one step of the show moves through a view of `len` photos. A view that
    /// grew or shrank under a shuffled show is asked again, and keeps the stride in use
    /// wherever it still can (`shuffle_stride`).
    fn stride(&mut self, len: usize) -> usize {
        match self.shuffle {
            None => 1,
            Some((drawn_for, stride)) if drawn_for == len => stride,
            Some((_, stride)) => {
                let stride = shuffle_stride(len, random, Some(stride));
                self.shuffle = Some((len, stride));
                stride
            }
        }
    }

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
    /// Turned and not cropped: the picture the crop tool draws its rectangle on.
    uncropped: bool,
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
    let made = commands::viewer_picture(engine, asked.want.id, asked.uncropped, Some(asked.fit));
    let picture = match made {
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
