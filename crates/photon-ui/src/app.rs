//! The application: the engine, the grid, and the order of a frame.

use crate::{
    dirs::Dirs,
    events::{Event, UiEvents},
    grid::{
        view::{GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
    probe::{Facts, Move, Outside, Probe, Report},
    tasks::Latest,
    theme::{
        self,
        apply::{color, palette},
    },
    thumbs::{loader::Loader, shown::Thumbs, source::EngineThumbs},
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
    folders: HashMap<i64, Folder>,
    folder_list: Latest<(), Option<Vec<Folder>>>,
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

        let (_, index, _, layout_gen) = engine.published();
        Ok(Self {
            engine,
            events: receiver,
            view: GridView::default(),
            thumbs: Thumbs::new(loader),
            index,
            layout_gen,
            folders: HashMap::new(),
            folder_list,
            size,
            zone: TimeZone::system(),
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
            let (_, index, _, layout_gen) = self.engine.published();
            self.index = index;
            self.layout_gen = layout_gen;
        }
        // A folder renamed, added or given an alias: the headers are named from the list.
        if data_changed {
            self.folder_list.ask(());
        }
        reported
    }

    fn take_answers(&mut self) {
        if let Some(Ok(Some(folders))) = self.folder_list.answer() {
            self.folders = folders
                .into_iter()
                .map(|folder| (folder.id, folder))
                .collect();
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
        let facts = Facts {
            now_ms: ctx.input(|input| input.time) * 1000.0,
            max: self.view.max_position(),
            settled: output.settled,
            photos: self.index.len(),
            outside: run.outside.frame(reported),
        };
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
        match run.probe.frame(facts, epoch_ms) {
            Move::To(position) => {
                self.view.scroll_to(position);
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
        let reported = self.take_events();
        self.take_answers();

        let data = GridData {
            layout_gen: self.layout_gen,
            index: &self.index,
            folders: &self.folders,
            size: self.size,
            zone: &self.zone,
        };
        let surface = color(palette(ui.ctx()).surface);
        let (view, thumbs) = (&mut self.view, &mut self.thumbs);
        let mut output = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(surface))
            .show(ui, |ui| output = Some(view.show(ui, &data, thumbs)));

        let now = ui.input(|input| input.time) * 1000.0;
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

/// eframe calls `on_exit` when the window closes; a test, or a start that fails after the
/// engine opened, only drops. Either way the engine is shut down once.
impl Drop for App {
    fn drop(&mut self) {
        self.close();
    }
}
