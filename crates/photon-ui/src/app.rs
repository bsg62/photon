//! The application: the engine, the grid, and the order of a frame.

use crate::{
    dirs::Dirs,
    events::{Event, UiEvents},
    grid::{
        view::{GridData, GridOutput, GridView},
        visible::VisibleReport,
    },
    icons,
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
    closed: bool,
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
            closed: false,
        })
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

    /// Takes what the engine has reported since the last frame. Only a changed library
    /// is acted on in this slice; the rest is taken so the channel stays empty.
    fn take_events(&mut self) {
        let mut changed = false;
        let mut data_changed = false;
        for event in self.events.try_iter() {
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
    }

    fn take_answers(&mut self) {
        if let Some(Ok(Some(folders))) = self.folder_list.answer() {
            self.folders = folders
                .into_iter()
                .map(|folder| (folder.id, folder))
                .collect();
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
        self.take_events();
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
