use std::{
    io,
    sync::{mpsc, Arc, Mutex, RwLock},
    thread::{self, JoinHandle},
};

use super::{discover, Application, DiscoveryDiagnostics, DiscoveryError, DiscoverySnapshot};

pub(crate) struct AppCache {
    scan: Mutex<()>,
    applications: RwLock<Vec<Application>>,
}

impl AppCache {
    pub(crate) fn new() -> Self {
        Self {
            scan: Mutex::new(()),
            applications: RwLock::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_apps(applications: Vec<Application>) -> Self {
        Self {
            scan: Mutex::new(()),
            applications: RwLock::new(applications),
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<Application> {
        self.applications
            .read()
            .expect("application cache lock poisoned")
            .clone()
    }

    pub(crate) fn contains(&self, app_id: &str) -> bool {
        self.applications
            .read()
            .expect("application cache lock poisoned")
            .iter()
            .any(|application| application.app_id == app_id)
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn refresh(&self) -> Result<DiscoveryDiagnostics, DiscoveryError> {
        self.refresh_with(discover)
    }

    pub(crate) fn refresh_with<F>(
        &self,
        discover: F,
    ) -> Result<DiscoveryDiagnostics, DiscoveryError>
    where
        F: FnOnce() -> Result<DiscoverySnapshot, DiscoveryError>,
    {
        let _scan = self.scan.lock().expect("application scan lock poisoned");
        let snapshot = discover()?;
        let diagnostics = snapshot.diagnostics;
        *self
            .applications
            .write()
            .expect("application cache lock poisoned") = snapshot.applications;
        Ok(diagnostics)
    }
}

pub(crate) fn start_initial_refresh(cache: Arc<AppCache>) -> io::Result<JoinHandle<()>> {
    start_initial_refresh_with(cache, discover)
}

fn start_initial_refresh_with<F>(cache: Arc<AppCache>, discover: F) -> io::Result<JoinHandle<()>>
where
    F: FnOnce() -> Result<DiscoverySnapshot, DiscoveryError> + Send + 'static,
{
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let handle = thread::Builder::new()
        .name("app-discovery".into())
        .spawn(move || {
            let _ = cache.refresh_with(|| {
                let _ = entered_tx.send(());
                discover()
            });
        })?;
    entered_rx
        .recv()
        .map_err(|_| io::Error::other("app discovery worker did not start"))?;
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{mpsc, Arc},
        thread,
    };

    use super::{start_initial_refresh_with, AppCache};
    use crate::apps::{
        Application, ApplicationLaunchTarget, DiscoveryDiagnostics, DiscoveryError,
        DiscoverySnapshot,
    };

    fn application(name: &str) -> Application {
        Application {
            app_id: format!("app-{name}"),
            display_name: name.into(),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(format!(r"C:\Menu\{name}.lnk")),
                executable: None,
            },
            icon: None,
            aliases: Vec::new(),
            use_count: 0,
        }
    }

    fn snapshot(applications: Vec<Application>) -> DiscoverySnapshot {
        DiscoverySnapshot {
            applications,
            diagnostics: DiscoveryDiagnostics::default(),
        }
    }

    fn titles(applications: &[Application]) -> Vec<&str> {
        applications
            .iter()
            .map(|application| application.display_name.as_str())
            .collect()
    }

    #[test]
    fn failed_refresh_preserves_last_good_snapshot() {
        for error in [
            DiscoveryError::KnownFolderQuery,
            DiscoveryError::AppsFolderEnumeration,
        ] {
            let cache = AppCache::from_apps(vec![application("Existing")]);

            assert_eq!(cache.refresh_with(|| Err(error)), Err(error));
            assert_eq!(titles(&cache.snapshot()), ["Existing"]);
            assert!(cache.contains("app-Existing"));
        }
    }

    #[test]
    fn successful_refresh_replaces_snapshot_once() {
        let cache = AppCache::from_apps(vec![application("Old")]);

        cache
            .refresh_with(|| Ok(snapshot(vec![application("New")])))
            .unwrap();

        assert_eq!(titles(&cache.snapshot()), ["New"]);
        assert!(!cache.contains("app-Old"));
    }

    #[test]
    fn initial_background_refresh_populates_the_shared_instance() {
        let managed = Arc::new(AppCache::new());
        let command_state = Arc::clone(&managed);

        let handle = start_initial_refresh_with(Arc::clone(&managed), || {
            Ok(snapshot(vec![application("First")]))
        })
        .unwrap();
        handle.join().unwrap();

        assert!(Arc::ptr_eq(&managed, &command_state));
        assert_eq!(titles(&command_state.snapshot()), ["First"]);
    }

    #[test]
    fn failed_initial_refresh_leaves_shared_instance_empty() {
        let managed = Arc::new(AppCache::new());

        let handle = start_initial_refresh_with(Arc::clone(&managed), || {
            Err(DiscoveryError::KnownFolderQuery)
        })
        .unwrap();
        handle.join().unwrap();

        assert!(managed.snapshot().is_empty());
    }

    #[test]
    fn initial_refresh_and_later_rescan_are_serialized_and_later_result_wins() {
        let cache = Arc::new(AppCache::new());
        let (release_initial_tx, release_initial_rx) = mpsc::sync_channel(0);
        let initial = start_initial_refresh_with(Arc::clone(&cache), move || {
            release_initial_rx.recv().unwrap();
            Ok(snapshot(vec![application("Initial")]))
        })
        .unwrap();

        let later_cache = Arc::clone(&cache);
        let later = thread::spawn(move || {
            later_cache
                .refresh_with(|| Ok(snapshot(vec![application("Later")])))
                .unwrap();
        });
        release_initial_tx.send(()).unwrap();
        initial.join().unwrap();
        later.join().unwrap();

        assert_eq!(titles(&cache.snapshot()), ["Later"]);
    }
}
