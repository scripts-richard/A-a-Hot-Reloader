extern crate inotify;

use std::collections::HashMap;
use std::io;
use walkdir::{DirEntry, WalkDir};

use inotify::{
    EventMask,
    Inotify,
    WatchDescriptor,
    WatchMask,
};

// Macro alias for slog info to first check for a logger.
macro_rules! watcher_info(
    ($w:expr, #$tag:expr, $($args:tt)+) => {
        if let Some(logger) = &$w.logger {
            info!(logger, $tag, $($args)+)
        }
    };
    ($w:expr, $($args:tt)+) => {
        if let Some(logger) = &$w.logger {
            info!(logger, $($args)+)
        }
    };
);

pub enum Traversal {
    RECURSIVE,
    HEURISTIC,
}

pub enum WatcherType {
    FILE,
    DIRECTORY,
}

pub struct Watcher {
    watcher_type: WatcherType,
    notify: Inotify,
    watch_mask: WatchMask,
    logger: Option<slog::Logger>,
    paths: Option<HashMap<WatchDescriptor, String>>
}

impl Watcher {
    pub fn file_watcher(file: &str) -> Result<Watcher, io::Error> {
        let mut inotify = Inotify::init()?;
        let watch_mask = WatchMask::MODIFY | WatchMask::DELETE;

        inotify.add_watch(file, watch_mask)?;

        Ok(Watcher {
            watcher_type: WatcherType::FILE,
            notify: inotify,
            watch_mask: watch_mask,
            logger: None,
            paths: None,
        })
    }

    pub fn dir_watcher(path: &str, trav: Traversal) -> Result<Watcher, io::Error> {
        let mut inotify = Inotify::init()?;
        let watch_mask = WatchMask::MODIFY |
                         WatchMask::CREATE |
                         WatchMask::DELETE;

        let paths = match trav {
            Traversal::RECURSIVE => {
                let mut paths = HashMap::new();

                for entry in WalkDir::new(path)
                    .follow_links(true)
                    .into_iter()
                    .filter_entry(|e| !is_hidden(e) && e.file_type().is_dir()) {
                        let entry = entry?;
                        let path = entry.path();
                        let wd = inotify.add_watch(path, watch_mask)?;

                        if let Some(path) = path.to_str() {
                            paths.insert(wd, String::from(path));
                        }
                }

                Some(paths)
            },
            Traversal::HEURISTIC => {
                inotify.add_watch(path, watch_mask)?;

                None
            }
        };

        Ok(Watcher {
            watcher_type: WatcherType::DIRECTORY,
            notify: inotify,
            watch_mask: watch_mask,
            logger: None,
            paths: paths,
        })
    }

    pub fn watch(&mut self) -> Result<(bool), io::Error> {
        match &self.watcher_type {
            WatcherType::FILE => self.file_event_loop(),
            WatcherType::DIRECTORY => self.dir_event_loop(),
        }
    }

    pub fn register_logger(&mut self, logger: slog::Logger) { self.logger = Some(logger); }

    fn dir_event_loop(&mut self) -> Result<(bool), io::Error> {
        let mut buffer = [0u8; 4096];

        loop {
            let events = self.notify.read_events_blocking(&mut buffer)?;

            for event in events {
                if event.mask.contains(EventMask::CREATE) {
                    if event.mask.contains(EventMask::ISDIR) {
                        watcher_info!(self, "Directory created: {:?}", event.name);

                        if let (Some(paths), Some(name)) = (&mut self.paths, event.name) {
                            if let Some(name) = name.to_str() {
                                if !name.starts_with(".") {
                                    let wd = event.wd;

                                    if let Some(path) = paths.get(&wd) {
                                        let new_path = path.to_owned() + "/" + name;
                                        watcher_info!(self, "Watching new directory: {}", new_path);

                                        let wd = self.notify.add_watch(&new_path, self.watch_mask)?;
                                        paths.insert(wd, new_path);
                                    }
                                }
                            }
                        }
                    } else {
                        watcher_info!(self, "File created: {:?}", event.name);
                    }
                } else if event.mask.contains(EventMask::DELETE) {
                    if event.mask.contains(EventMask::ISDIR) {
                        watcher_info!(self, "Directory deleted: {:?}", event.name);
                    } else {
                        watcher_info!(self, "File deleted: {:?}", event.name);
                    }
                } else if event.mask.contains(EventMask::MODIFY) {
                    if event.mask.contains(EventMask::ISDIR) {
                        watcher_info!(self, "Directory modified: {:?}", event.name);
                    } else {
                        watcher_info!(self, "File modified: {:?}", event.name);
                    }
                }
                return Ok(true);
            }
        }
    }

    fn file_event_loop(&mut self) -> Result<(bool), io::Error> {
        let mut buffer = [0u8; 4096];

        loop {
            let events = self.notify.read_events_blocking(&mut buffer)?;

            for event in events {
                if event.mask.contains(EventMask::MODIFY) {
                    watcher_info!(self, "File modified");
                } else {
                    watcher_info!(self, "Unexpected event: {:?}", event.name);
                }

                return Ok(true);
            }
        }
    }
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name()
         .to_str()
         .map(|s| s.starts_with("."))
         .unwrap_or(false)
}
