//! The application: the engine, the shell around the grid, and the order of a frame.

use crate::{
    dirs::Dirs,
    empty::{self, GridState},
    events::{Event, UiEvents},
    grid::{
        view::{GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
    nav::{Nav, Place, Step},
    probe::{Facts, Move, Outside, Probe, Report},
    shell::{Action, Shell, ShellData},
    sidebar::rows::{Counts, Today, fixed_rows},
    tasks::{Latest, Queue},
    theme,
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
    toasts::{Toast, Toasts},
    window_layout::Layout,
};
use eframe::egui;
use jiff::tz::TimeZone;
use photon_core::{
    grid::GridIndex,
    library::{Folder, GridTile},
};
use photon_engine::{
    commands,
    engine::{Engine, EngineConfig},
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::mpsc::Receiver, time::Duration};

/// How long a thumbnail that has not been built is waited for, as `protocol.rs`'s
/// `THUMB_TIMEOUT` bounds the same wait.
const THUMB_TIMEOUT: Duration = Duration::from_secs(30);
/// Threads decoding cached thumbnails. A decode is a tenth of a millisecond; two keep up
/// with a scroll and leave the cores to the engine's renders.
const DECODERS: usize = 2;

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
    folders: HashMap<i64, Folder>,
    folder_list: Latest<(), Option<Vec<Folder>>>,
    /// Where the user is and is going, and the queue the steps there are made on.
    nav: Nav,
    steps: Queue<Step, Result<(), String>>,
    /// What the library holds of each kind, for the sidebar. Asked for by the layout
    /// generation held, so that the answer need not carry the layout.
    counts: Counts,
    counting: Latest<u64, Counts>,
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
                move |()| {
                    commands::list_folders(&engine)
                        .ok()
                        .map(|list| list.folders)
                }
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
                    match step {
                        Step::View(view) => commands::set_grid_view(&engine, view),
                        Step::Search(query) => commands::set_search_query(&engine, &query),
                    }
                    .map(|_| ())
                    .map_err(|err| err.message)
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
            folders: HashMap::new(),
            folder_list,
            nav: Nav::new(Place { view, arg }, engine.sort()),
            steps,
            counts: Counts::default(),
            counting,
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

    /// The folders the headers are named from.
    pub fn folders(&self) -> &HashMap<i64, Folder> {
        &self.folders
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

    /// How the window is laid out.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The messages showing.
    pub fn toasts(&self) -> &[Toast] {
        self.toasts.showing()
    }

    /// The version of the grid on screen. Every grid the engine publishes has another.
    pub fn version(&self) -> u64 {
        self.grid.version
    }

    /// The status bar's count of the photos shown, when there is one to give.
    pub fn photo_count(&self) -> Option<String> {
        empty::photo_count(&self.grid)
    }

    /// The line shown in place of photos. It is about the grid that is on screen, not
    /// about the view being gone to.
    pub fn notice(&self) -> Option<String> {
        empty::view_notice(&self.grid, self.nav.settled())
    }

    /// Asks for `step`, unless it leads to where the user already is.
    fn go(&mut self, step: Step) {
        if !self.nav.wants(&step) {
            return;
        }
        let number = self.steps.push(step.clone());
        self.nav.asked(number, step);
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::ToggleSidebar => {
                self.layout.sidebar_hidden = !self.layout.sidebar_hidden;
                self.layout_store.ask(self.layout);
            }
            Action::Go(row) => {
                // The clock itself, not the day the row was drawn with: a click is never a
                // day behind.
                if let Some(step) = row.step(today(&self.zone)) {
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
        }
    }

    /// Takes what the engine has reported since the last frame, and says whether it had
    /// reported anything. Only a changed library is acted on in this slice; the rest is
    /// taken so the channel stays empty.
    fn take_events(&mut self) -> bool {
        let mut reported = false;
        let mut changed = false;
        let mut data_changed = false;
        for event in self.events.try_iter() {
            reported = true;
            if let Event::Library(library) = event {
                changed = true;
                data_changed |= library.data_changed;
            }
        }
        if changed {
            let (version, index, build_error, layout_gen) = self.engine.published();
            self.grid = GridState {
                version,
                len: index.len(),
                build_error,
            };
            self.index = index;
            self.layout_gen = layout_gen;
            // Other results are another list: a place in the old one names nothing here.
            let (view, arg) = self.engine.view_and_arg();
            if self.nav.settle(Place { view, arg }, self.engine.sort()) {
                self.view.to_top();
            }
            // The counts read SQLite when they are not cached, so they are asked for.
            self.counting.ask(layout_gen);
            // And today is read again, so that a window left open past midnight catches
            // up the next time anything in the library moves.
            self.today = today(&self.zone);
        }
        // A folder renamed, added or given an alias: the headers are named from the list.
        if data_changed {
            self.folder_list.ask(());
        }
        reported
    }

    fn take_answers(&mut self, now_ms: f64) {
        if let Some(Ok(Some(folders))) = self.folder_list.answer() {
            self.folders = folders
                .into_iter()
                .map(|folder| (folder.id, folder))
                .collect();
        }
        if let Some(Ok(counts)) = self.counting.answer() {
            self.counts = counts;
        }
        let _ = self.layout_store.answer();
        for (number, answer) in self.steps.answers() {
            let refused = match answer {
                Ok(Ok(())) => None,
                Ok(Err(why)) => Some(why),
                Err(_) => Some("photon could not change the view.".to_owned()),
            };
            if let Some(said) = self.nav.answered(number, refused) {
                self.toasts.error(said, now_ms);
            }
        }
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
        self.take_answers(now);

        let data = GridData {
            layout_gen: self.layout_gen,
            index: &self.index,
            folders: &self.folders,
            size: self.size,
            zone: &self.zone,
        };
        // The row marked is where the user is going; the line an empty view shows is
        // about the grid that is on screen.
        let rows = fixed_rows(&self.counts, &self.nav.target(), self.today);
        let count = self.photo_count();
        let notice = self.notice();
        let shell = ShellData {
            layout: &self.layout,
            rows: &rows,
            count: count.as_deref(),
            notice: notice.as_deref(),
            toasts: self.toasts.showing(),
        };
        let (view, thumbs) = (&mut self.view, &mut self.thumbs);
        let mut output = None;
        let actions = self.shell.show(ui, &shell, |ui| {
            output = Some(view.show(ui, &data, thumbs));
        });
        for action in actions {
            self.act(action);
        }
        // A still window draws no frame by itself, and a message would stay.
        if let Some(at) = self.toasts.tick(now) {
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
