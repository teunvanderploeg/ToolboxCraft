use crate::{
    catalog::{App, CATALOG, by_id},
    model::*,
    releases::{Releases, select_asset},
    self_update::{self, SelfUpdate},
    store::Store,
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub trait Platform: Send + Sync + 'static {
    fn recover(&self) -> Result<BTreeMap<String, Installation>>;
    fn detect(&self, app: App) -> Option<Installation>;
    fn running(&self, installation: &Installation) -> bool;
    fn install(&self, app: App, version: &str, package: &Path) -> Result<Installation>;
    fn remove(&self, app: App, installation: &Installation) -> Result<()>;
    fn launch(&self, app: App, installation: &Installation) -> Result<()>;
    fn launch_at_login(&self, enabled: bool) -> Result<()>;
    fn notify(&self, title: &str, message: &str);
    fn stage_self_update(&self, package: &Path) -> Result<()>;
}
#[derive(Debug)]
pub enum Command {
    Check,
    Install(String),
    Cancel(String),
    Launch(String),
    Remove(String),
    Settings(Settings),
    CheckSelfUpdate,
    ApplySelfUpdate,
    Shutdown,
}
#[derive(Debug, Clone)]
pub enum Event {
    Changed,
    Notice(String),
    SelfUpdate(Option<SelfUpdate>),
    RestartReady,
}
#[derive(Clone)]
pub struct EngineHandle {
    pub commands: tokio::sync::mpsc::UnboundedSender<Command>,
    pub state: Arc<Mutex<State>>,
}
pub struct Engine {
    store: Store,
    state: Arc<Mutex<State>>,
    platform: Arc<dyn Platform>,
    client: Releases,
    events: mpsc::Sender<Event>,
    target: Target,
    downloads: Arc<Semaphore>,
    installs: Arc<Semaphore>,
    cancellations: Arc<Mutex<BTreeMap<String, CancellationToken>>>,
    update: Arc<Mutex<Option<SelfUpdate>>>,
    stop: CancellationToken,
}
impl Engine {
    pub fn start(
        store: Store,
        platform: Arc<dyn Platform>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(EngineHandle, mpsc::Receiver<Event>)> {
        let mut state = store.load()?;
        store.recover(&mut state)?;
        let managed = platform.recover()?;
        state.installations.retain(|_, i| !i.managed);
        state.installations.extend(managed);
        for app in CATALOG {
            if !state.installations.contains_key(app.id)
                && let Some(i) = platform.detect(app)
            {
                state.installations.insert(app.id.into(), i);
            }
        }
        state.installations.retain(|_, i| i.path.exists());
        store.save(&state)?;
        let state = Arc::new(Mutex::new(state));
        let (commands, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (events, event_rx) = mpsc::channel();
        let engine = Arc::new(Self {
            store,
            state: state.clone(),
            platform,
            client: Releases::new()?,
            events,
            target: Target::current()?,
            downloads: Arc::new(Semaphore::new(2)),
            installs: Arc::new(Semaphore::new(1)),
            cancellations: Arc::new(Mutex::new(BTreeMap::new())),
            update: Arc::new(Mutex::new(None)),
            stop: CancellationToken::new(),
        });
        runtime.spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut rescan = tokio::time::interval(Duration::from_secs(30));
            let mut checking = false;
            let mut check_task: Option<tokio::task::JoinHandle<()>> = None;
            loop {
                if check_task.as_ref().is_some_and(|t| t.is_finished()) { checking = false; check_task = None; }
                tokio::select! {
                    _ = timer.tick() => { if !checking { checking = true; let e = engine.clone(); check_task = Some(tokio::spawn(async move { e.check().await; e.check_self().await; })); } }
                    _ = rescan.tick() => { engine.rescan(); engine.auto_updates(); }
                    command = rx.recv() => match command {
                        Some(Command::Check) => { if !checking { checking = true; let e = engine.clone(); check_task = Some(tokio::spawn(async move { e.check().await; })); } }
                        Some(Command::Install(id)) => engine.queue(id),
                        Some(Command::Cancel(id)) => { if let Some(c) = engine.cancellations.lock().unwrap().get(&id) { c.cancel(); } }
                        Some(Command::Launch(id)) => {
                            let e = engine.clone(); tokio::task::spawn_blocking(move || { if let (Some(app), Some(i)) = (by_id(&id), e.snapshot().installations.get(&id).cloned()) && let Err(err) = e.platform.launch(app, &i) { e.notice(err); } });
                        }
                        Some(Command::Remove(id)) => { let e = engine.clone(); tokio::spawn(async move { if let Err(err) = e.remove(&id).await { e.notice(err); } }); }
                        Some(Command::Settings(settings)) => {
                            let previous = engine.snapshot().settings.launch_at_login;
                            let desired = settings.launch_at_login;
                            { let mut state = engine.state.lock().unwrap(); state.settings = settings; engine.persist(&state); }
                            if previous != desired {
                                let e = engine.clone(); tokio::task::spawn_blocking(move || {
                                    if let Err(err) = e.platform.launch_at_login(desired) {
                                        e.notice(err);
                                        let mut state = e.state.lock().unwrap();
                                        if state.settings.launch_at_login == desired { state.settings.launch_at_login = previous; e.persist(&state); }
                                    }
                                });
                            }
                        }
                        Some(Command::CheckSelfUpdate) => { let e = engine.clone(); tokio::spawn(async move { e.check_self().await; }); }
                        Some(Command::ApplySelfUpdate) => { let e = engine.clone(); tokio::spawn(async move { if let Err(err) = e.apply_self().await { e.notice(err); } }); }
                        Some(Command::Shutdown) | None => { engine.stop.cancel(); for token in engine.cancellations.lock().unwrap().values() { token.cancel(); } break; }
                    }
                }
            }
        });
        Ok((EngineHandle { commands, state }, event_rx))
    }
    fn snapshot(&self) -> State {
        self.state.lock().unwrap().clone()
    }
    fn persist(&self, state: &State) {
        if let Err(err) = self.store.save(state) {
            self.notice(format!("Cannot save application state: {err}"));
        }
        let _ = self.events.send(Event::Changed);
    }
    fn notice(&self, message: impl std::fmt::Display) {
        let text = message.to_string();
        tracing::warn!("{text}");
        let _ = self.events.send(Event::Notice(text));
    }
    fn operation(&self, id: &str, op: Operation) {
        let mut state = self.state.lock().unwrap();
        state.operations.insert(id.into(), op);
        self.persist(&state);
    }
    async fn check(self: &Arc<Self>) {
        self.notice("Checking for updates…");
        let mut failures = 0;
        for app in CATALOG {
            if self.stop.is_cancelled() {
                return;
            }
            let cached = self.snapshot().releases.get(app.id).cloned();
            match self.client.latest(app, cached.as_ref()).await {
                Ok(release) => {
                    let mut state = self.state.lock().unwrap();
                    state.releases.insert(app.id.into(), release);
                    self.persist(&state);
                }
                Err(err) => {
                    failures += 1;
                    self.notice(format!("{}: {err}", app.name));
                    if err.to_string().contains("request limit") {
                        break;
                    }
                }
            }
        }
        if failures == 0 {
            let _ = self
                .events
                .send(Event::Notice("Release information is up to date".into()));
        }
        self.auto_updates();
        let _ = self.events.send(Event::Changed);
    }
    fn rescan(&self) {
        let before = self.snapshot().installations;
        let mut installations = before.clone();
        installations.retain(|_, i| i.path.exists());
        for app in CATALOG {
            if !installations.contains_key(app.id)
                && let Some(i) = self.platform.detect(app)
            {
                installations.insert(app.id.into(), i);
            }
        }
        // Avoid overwriting a concurrent installation commit.
        let mut state = self.state.lock().unwrap();
        for (id, old) in before {
            if !old.path.exists()
                && state
                    .installations
                    .get(&id)
                    .is_some_and(|i| i.path == old.path)
            {
                state.installations.remove(&id);
            }
        }
        for (id, i) in installations {
            state.installations.entry(id).or_insert(i);
        }
        self.persist(&state);
    }
    fn auto_updates(self: &Arc<Self>) {
        let snapshot = self.snapshot();
        for (id, i) in &snapshot.installations {
            if i.managed
                && snapshot.settings.auto_update_app(id)
                && snapshot
                    .releases
                    .get(id)
                    .is_some_and(|r| newer(&r.release.tag_name, &i.version))
                && !matches!(
                    snapshot.operations.get(id),
                    Some(Operation::Failed(_) | Operation::Cancelled)
                )
            {
                self.queue(id.clone());
            }
        }
    }
    fn queue(self: &Arc<Self>, id: String) {
        if by_id(&id).is_none() {
            return;
        }
        // A token also guards the interval before the persisted operation appears.
        let cancel = {
            let mut jobs = self.cancellations.lock().unwrap();
            if jobs.contains_key(&id) {
                return;
            }
            let token = self.stop.child_token();
            jobs.insert(id.clone(), token.clone());
            token
        };
        self.operation(&id, Operation::Queued);
        let e = self.clone();
        tokio::spawn(async move {
            let result = e.install(&id, &cancel).await;
            if let Err(err) = result {
                e.operation(
                    &id,
                    if cancel.is_cancelled() {
                        Operation::Cancelled
                    } else {
                        Operation::Failed(format!("{err:#}"))
                    },
                );
            }
            e.cancellations.lock().unwrap().remove(&id);
        });
    }
    async fn install(&self, id: &str, cancel: &CancellationToken) -> Result<()> {
        let app = by_id(id).context("Unknown application")?;
        let cached = self
            .snapshot()
            .releases
            .get(id)
            .cloned()
            .context("Release information is not available. Check for updates first.")?;
        let release = cached.release;
        let asset = select_asset(app, &release, self.target)?.clone();
        let permit = tokio::select! { _ = cancel.cancelled() => anyhow::bail!("Cancelled"), p = self.downloads.acquire() => p? };
        let sums = tokio::select! { _ = cancel.cancelled() => anyhow::bail!("Cancelled"), r = self.client.checksum(app, &release, &asset) => r? };
        let directory = self.store.root.join("downloads");
        tokio::fs::create_dir_all(&directory).await?;
        let package = directory.join(format!("{id}.package"));
        let mut last = std::time::Instant::now() - Duration::from_secs(1);
        // Download progress updates are throttled and transient; recovery uses the persisted initial phase.
        self.operation(
            id,
            Operation::Downloading {
                received: 0,
                total: asset.size,
            },
        );
        let progress_state = self.state.clone();
        let events = self.events.clone();
        let throttle = Mutex::new(&mut last);
        self.client
            .download(
                (app, &release),
                &asset,
                &package,
                sums,
                cancel,
                |received, total| {
                    let mut last = throttle.lock().unwrap();
                    if last.elapsed() > Duration::from_millis(100) || received == total {
                        **last = std::time::Instant::now();
                        progress_state
                            .lock()
                            .unwrap()
                            .operations
                            .insert(id.into(), Operation::Downloading { received, total });
                        let _ = events.send(Event::Changed);
                    }
                },
            )
            .await?;
        self.operation(id, Operation::Verifying);
        drop(permit);
        if let Err(err) = self.wait_for_exit(id, cancel).await {
            let _ = tokio::fs::remove_file(&package).await;
            return Err(err);
        }
        let _permit = tokio::select! { _ = cancel.cancelled() => anyhow::bail!("Cancelled"), p = self.installs.acquire() => p? };
        anyhow::ensure!(!cancel.is_cancelled(), "Cancelled");
        if let Some(i) = self.snapshot().installations.get(id).cloned() {
            anyhow::ensure!(
                !self.platform.running(&i),
                "The app opened while its update was queued. Close it and retry."
            );
        }
        self.operation(id, Operation::Installing);
        let platform = self.platform.clone();
        let version = release.tag_name.clone();
        let downloaded = package.clone();
        let installed =
            tokio::task::spawn_blocking(move || platform.install(app, &version, &downloaded))
                .await??;
        let _ = tokio::fs::remove_file(package).await;
        {
            let mut state = self.state.lock().unwrap();
            state.installations.insert(id.into(), installed);
            state.history.insert(
                0,
                HistoryEntry {
                    app: id.into(),
                    version: release.tag_name,
                    timestamp: now(),
                    action: "Installed".into(),
                },
            );
            state.history.truncate(200);
            state.operations.insert(id.into(), Operation::Complete);
            self.persist(&state);
        }
        self.platform
            .notify("ToolboxCraft", &format!("{} is ready to open", app.name));
        Ok(())
    }
    async fn wait_for_exit(&self, id: &str, cancel: &CancellationToken) -> Result<()> {
        loop {
            anyhow::ensure!(!cancel.is_cancelled(), "Cancelled");
            if let Some(i) = self.snapshot().installations.get(id).cloned()
                && self.platform.running(&i)
            {
                self.operation(id, Operation::WaitingForExit);
                tokio::select! { _ = cancel.cancelled() => {}, _ = tokio::time::sleep(Duration::from_millis(500)) => {} }
            } else {
                return Ok(());
            }
        }
    }
    async fn remove(&self, id: &str) -> Result<()> {
        let app = by_id(id).context("Unknown app")?;
        {
            let mut jobs = self.cancellations.lock().unwrap();
            anyhow::ensure!(
                !jobs.contains_key(id),
                "Wait for the current operation to finish"
            );
            jobs.insert(id.into(), self.stop.child_token());
        }
        let result = async {
            let _permit = self.installs.acquire().await?;
            let i = self
                .snapshot()
                .installations
                .get(id)
                .cloned()
                .context("App is not installed")?;
            anyhow::ensure!(i.managed, "This app was installed outside ToolboxCraft");
            anyhow::ensure!(
                !self.platform.running(&i),
                "Close the app before uninstalling"
            );
            self.operation(id, Operation::Installing);
            let platform = self.platform.clone();
            let version = i.version.clone();
            tokio::task::spawn_blocking(move || platform.remove(app, &i)).await??;
            let mut state = self.state.lock().unwrap();
            state.installations.remove(id);
            state.operations.remove(id);
            state.history.insert(
                0,
                HistoryEntry {
                    app: id.into(),
                    version,
                    timestamp: now(),
                    action: "Uninstalled".into(),
                },
            );
            state.history.truncate(200);
            self.persist(&state);
            Ok(())
        }
        .await;
        self.cancellations.lock().unwrap().remove(id);
        if result.is_err() {
            let mut state = self.state.lock().unwrap();
            state.operations.remove(id);
            self.persist(&state);
        }
        result
    }
    async fn check_self(&self) {
        match self_update::check(&self.client, self.target).await {
            Ok(update) => {
                *self.update.lock().unwrap() = update.clone();
                let _ = self.events.send(Event::SelfUpdate(update));
            }
            Err(err) => self.notice(format!("ToolboxCraft update check: {err}")),
        }
    }
    async fn apply_self(&self) -> Result<()> {
        let _permit = self.installs.acquire().await?;
        anyhow::ensure!(
            self.cancellations.lock().unwrap().is_empty(),
            "Wait for application operations to finish before updating ToolboxCraft"
        );
        let update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .context("No ToolboxCraft update is available")?;
        let path = self.store.root.join("self-update.zip");
        self_update::download(&self.client, &update, &path, &self.stop).await?;
        let platform = self.platform.clone();
        tokio::task::spawn_blocking(move || platform.stage_self_update(&path)).await??;
        let _ = self.events.send(Event::RestartReady);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct TestPlatform {
        running: AtomicBool,
    }
    impl Platform for TestPlatform {
        fn recover(&self) -> Result<BTreeMap<String, Installation>> {
            Ok(BTreeMap::new())
        }
        fn detect(&self, _: App) -> Option<Installation> {
            None
        }
        fn running(&self, _: &Installation) -> bool {
            self.running.load(Ordering::SeqCst)
        }
        fn install(&self, _: App, _: &str, _: &Path) -> Result<Installation> {
            anyhow::bail!("must not install during these tests")
        }
        fn remove(&self, _: App, _: &Installation) -> Result<()> {
            anyhow::bail!("must not remove during these tests")
        }
        fn launch(&self, _: App, _: &Installation) -> Result<()> {
            Ok(())
        }
        fn launch_at_login(&self, _: bool) -> Result<()> {
            Ok(())
        }
        fn notify(&self, _: &str, _: &str) {}
        fn stage_self_update(&self, _: &Path) -> Result<()> {
            Ok(())
        }
    }
    fn engine(temp: &tempfile::TempDir, platform: Arc<TestPlatform>) -> Arc<Engine> {
        let (events, _) = mpsc::channel();
        Arc::new(Engine {
            store: Store::new(temp.path()).unwrap(),
            state: Arc::new(Mutex::new(State::default())),
            platform,
            client: Releases::new().unwrap(),
            events,
            target: Target {
                os: Os::Linux,
                arch: Arch::X64,
            },
            downloads: Arc::new(Semaphore::new(2)),
            installs: Arc::new(Semaphore::new(1)),
            cancellations: Arc::new(Mutex::new(BTreeMap::new())),
            update: Arc::new(Mutex::new(None)),
            stop: CancellationToken::new(),
        })
    }
    #[tokio::test]
    async fn waits_for_an_external_process_and_cancellation_preserves_installed_state() {
        let temp = tempfile::tempdir().unwrap();
        let platform = Arc::new(TestPlatform {
            running: AtomicBool::new(true),
        });
        let e = engine(&temp, platform.clone());
        e.state.lock().unwrap().installations.insert(
            "photocraft".into(),
            Installation {
                path: temp.path().into(),
                version: "1.0.0".into(),
                managed: true,
            },
        );
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        let worker = e.clone();
        let task = tokio::spawn(async move { worker.wait_for_exit("photocraft", &cancel2).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(matches!(
            e.snapshot().operations["photocraft"],
            Operation::WaitingForExit
        ));
        assert!(!task.is_finished());
        cancel.cancel();
        assert!(task.await.unwrap().is_err());
        assert_eq!(e.snapshot().installations["photocraft"].version, "1.0.0");
        platform.running.store(false, Ordering::SeqCst);
        assert!(
            e.wait_for_exit("photocraft", &CancellationToken::new())
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn duplicate_queued_jobs_and_failed_updates_do_not_replace_apps() {
        let temp = tempfile::tempdir().unwrap();
        let e = engine(
            &temp,
            Arc::new(TestPlatform {
                running: AtomicBool::new(false),
            }),
        );
        e.state.lock().unwrap().installations.insert(
            "photocraft".into(),
            Installation {
                path: temp.path().into(),
                version: "1.0.0".into(),
                managed: true,
            },
        );
        e.queue("photocraft".into());
        e.queue("photocraft".into());
        assert_eq!(e.cancellations.lock().unwrap().len(), 1);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(matches!(
            e.snapshot().operations["photocraft"],
            Operation::Failed(_)
        ));
        assert_eq!(e.snapshot().installations["photocraft"].version, "1.0.0");
    }
}
