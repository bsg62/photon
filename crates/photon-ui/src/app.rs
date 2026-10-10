//! The application: the engine, the shell around the grid, and the order of a frame.

use crate::{
    dirs::Dirs,
    empty::{self, GridState, LibraryFacts, Panel, PanelButton},
    events::{Event, UiEvents},
    grid::{
        view::{Align, GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
    nav::{Landed, LastFolder, Nav, Place, Step},
    probe::{Facts, Move, Outside, Probe, Report},
    scans::Scans,
    search_bar::{SearchAction, SearchBarData},
    search_box::{SearchBox, saved_search_for},
    shell::{Action, Shell, ShellData},
    sidebar::{
        list::{Collections, Group, Held, List, Sources, What},
        rows::{Counts, Fixed, Today},
    },
    status::Line,
    tasks::{Latest, Queue},
    theme,
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
    toasts::{Toast, Toasts},
    window_layout::Layout,
};
use eframe::egui;
use jiff::tz::TimeZone;
use photon_core::{
    grid::{GridIndex, GridView as View},
    library::{Folder, GridTile, WatchedFolder},
    sort::Sort,
};
use photon_engine::{
    commands::{self, FolderList},
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

/// Something to write to the library.
enum Write {
    /// The search in the box, under a name: the bookmark.
    SaveSearch { name: String, query: String },
    /// The size of the tiles, as the size control was last pressed.
    TileSize(GridTile),
}

/// A write that was made.
enum Wrote {
    Search,
    TileSize,
}

/// A write the library refused, and what it said.
enum Unwritten {
    Search { query: String, said: String },
    TileSize { said: String },
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
        theme::apply::install(&ctx);
        // A notch of the wheel as far as the web view of this system moved it.
        crate::grid::view::set_wheel_notch(&ctx, crate::grid::scroll::WHEEL_NOTCH);
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
            Arc::new(EngineThumbs(engine.clone())),
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
                self.layout.sidebar_hidden = !self.layout.sidebar_hidden;
                self.layout_store.ask(self.layout);
            }
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
            }
        }
        let _ = self.layout_store.answer();
        let _ = self.remembering.answer();
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
        self.index = index;
        self.layout_gen = layout_gen;
        // Other results are another list: a place in the old one names nothing here.
        if shown.other_results {
            self.view.to_top();
            // A folder asked for in the results that were on screen is not a place in
            // these.
            self.going = None;
        }
        // A folder clicked from an excursion: this is the grid it was to be looked up in.
        if let Some(folder) = shown.jump {
            self.go_to_folder(folder);
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
            self.engine.shutdown();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Whether this frame was asked for from outside the grid: by the engine, which
        // reported, or by the person, whose mouse or keys are in the frame's input.
        let touched = ui.input(|input| !input.events.is_empty());
        let reported = self.take_events() || touched;
        let now = ui.input(|input| input.time) * 1000.0;
        // A task's answer asked for this frame as an engine's report does: the frame is
        // not one the grid drew by itself.
        let reported = self.take_answers(now) || reported;
        self.adopt();
        // The launch's way back, in the frame its first photos are on screen and before
        // they are drawn.
        let (photos, view, sort) = (self.grid.len, self.nav.settled().view, self.nav.sort());
        if let Some(folder) = self.last_folder.restore(photos, view, sort) {
            self.go_to_folder(folder);
        }
        self.take_folder();
        // Nothing reports the moment the engine has started its first scans: it is asked
        // for, a few times a second, for as long as the launch takes and no longer.
        if self.scans.starting() {
            ui.ctx().request_repaint_after(LAUNCH_POLL);
        }
        // A search typed, whose time has come.
        if let Some(query) = self.search.take_due(now) {
            self.search_for(query);
        }

        let data = GridData {
            layout_gen: self.layout_gen,
            index: &self.index,
            folders: self.held.folders(),
            size: self.size,
            zone: &self.zone,
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
            search: SearchBarData {
                saved_as: saved_as.as_deref(),
                can_save: self.search.can_save(searches),
                in_search: self.nav.target().view == View::Search,
            },
            sort: self.nav.sort_target(),
            size: self.size,
            count: count.as_deref(),
            lines: &lines,
            panel: panel.as_ref(),
            notice: notice.as_deref(),
            toasts,
        };
        let (view, thumbs) = (&mut self.view, &mut self.thumbs);
        let mut output = None;
        let actions = self.shell.show(ui, &shell, &mut self.search.text, |ui| {
            output = Some(view.show(ui, &data, thumbs));
        });
        for action in actions {
            self.act(action, now);
        }
        // A typed search is sent in a frame of its own, which a still window would not
        // draw: asked for here, at every frame until it is sent, since a frame asked for
        // after a delay comes a frame early.
        if let Some(at) = self.search.due_at() {
            let wait = Duration::from_secs_f64(((at - now) / 1000.0).max(0.0));
            ui.ctx().request_repaint_after(wait);
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
