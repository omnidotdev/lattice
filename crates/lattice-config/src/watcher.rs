use std::path::PathBuf;
use std::time::Duration;

use notify::{PollWatcher, RecursiveMode, Watcher};

/// Watch a config file for changes using polling.
///
/// Uses [`PollWatcher`] with a 2-second interval for cross-platform
/// compatibility.
pub struct ConfigWatcher {
    _watcher: PollWatcher,
}

impl ConfigWatcher {
    /// Start watching the config file at `path`.
    ///
    /// Calls `on_change` whenever the file is modified or recreated.
    ///
    /// # Errors
    ///
    /// Returns a notify error if the watcher cannot be created or the
    /// path cannot be watched.
    pub fn new(path: PathBuf, on_change: impl Fn() + Send + Sync + 'static) -> Result<Self, notify::Error> {
        let watch_path = path;

        let mut watcher = PollWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || matches!(event.kind, notify::EventKind::Create(_)) {
                        on_change();
                    }
                }
            },
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

        tracing::info!(path = %watch_path.display(), "config file watcher started");

        Ok(Self { _watcher: watcher })
    }
}
