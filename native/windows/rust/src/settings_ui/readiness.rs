use super::*;
use crate::vocabulary::{Hooks, Worker, model_for};
use crate::vocabulary_contract::{Error, ModuleOwner, Ready, SourceReady, Status};

#[derive(Default)]
pub(super) struct Readiness {
    worker: Option<Worker>,
    saved: Option<Snapshot>,
    generation: u32,
    report: Option<Ready>,
    last_good: Option<Ready>,
    failed: bool,
    pub(super) previous_settings: bool,
}

impl Readiness {
    fn accept(&mut self, report: Ready) {
        if report.activation != self.generation
            || self
                .report
                .as_ref()
                .is_some_and(|old| report.revision <= old.revision)
        {
            return;
        }
        if report.model
            && report
                .sources
                .iter()
                .all(|source| source.dictionary.is_some() && source.error == Error::None)
        {
            self.last_good = Some(report.clone());
        }
        self.report = Some(report);
    }
}

impl Source {
    fn preflight(&self) -> bool {
        match self {
            Self::CurrentUser => true,
            #[cfg(test)]
            Self::Preflight(_) => true,
            _ => false,
        }
    }
}

impl State {
    pub(super) fn restart_readiness(&self, saved: &Snapshot) {
        if !self.source.preflight() {
            self.render_readiness();
            return;
        }
        let mut worker = self
            .readiness
            .borrow_mut()
            .worker
            .take()
            .unwrap_or_default();
        let generation = self.readiness.borrow().generation.checked_add(1);
        let started = generation.is_some_and(|generation| {
            worker.start(
                &model_for(&saved.value),
                generation,
                ModuleOwner::Process,
                Hooks::default(),
            )
        });
        {
            let mut readiness = self.readiness.borrow_mut();
            readiness.worker = Some(worker);
            readiness.saved = Some(saved.clone());
            readiness.generation = generation.unwrap_or(u32::MAX);
            readiness.report = None;
            readiness.last_good = None;
            readiness.failed = !started;
            readiness.previous_settings = false;
        }
        self.render_readiness();
    }

    pub(super) fn poll_readiness(&self) {
        let worker = self.readiness.borrow_mut().worker.take();
        let Some(mut worker) = worker else {
            return;
        };
        let report = worker.poll();
        {
            let mut readiness = self.readiness.borrow_mut();
            readiness.worker = Some(worker);
            if let Some(report) = report {
                readiness.accept(report);
            }
        }
        self.render_readiness();
    }

    pub(super) fn stop_readiness(&self) {
        let worker = self.readiness.borrow_mut().worker.take();
        drop(worker);
    }

    pub(super) fn render_readiness(&self) {
        let text = {
            let readiness = self.readiness.borrow();
            if !self.source.preflight() {
                "Local resource checks disabled for this isolated view.\r\nLive host adoption unavailable. No active host is bound.".to_owned()
            } else if readiness.saved.is_none() {
                "Local resource check unavailable until settings load.\r\nLive host adoption unavailable. No active host is bound.".to_owned()
            } else if readiness.failed {
                "Local resource check could not start for the verified settings.\r\nLive host adoption unavailable. No active host is bound.".to_owned()
            } else {
                let binding = if readiness.previous_settings {
                    "previous verified settings"
                } else {
                    "last verified settings"
                };
                let resources = match &readiness.report {
                    Some(report) if report.error != Error::None => {
                        "Local resource check failed.".to_owned()
                    }
                    Some(report) => format!(
                        "Model {}. English {}. Russian {}.",
                        if report.model { "ready" } else { "unavailable" },
                        source_status(&report.sources[0]),
                        source_status(&report.sources[1])
                    ),
                    None => "Local resource check pending.".to_owned(),
                };
                let retained = if readiness.last_good.is_some()
                    && readiness.report.as_ref().is_some_and(|report| {
                        report.error != Error::None
                            || report
                                .sources
                                .iter()
                                .any(|source| source.error != Error::None)
                    }) {
                    " Last good resources retained."
                } else {
                    ""
                };
                format!(
                    "For {binding}. {resources}{retained}\r\nLive host adoption unavailable. No active host is bound."
                )
            }
        };
        set_text(self.control(READINESS), &text);
    }
}

fn source_status(source: &SourceReady) -> &'static str {
    match (source.status, source.error) {
        (Status::Retained, _) => "last good retained after error",
        (_, Error::None) => match source.status {
            Status::Ready => "ready",
            Status::Pending => "pending",
            Status::Missing => "default file absent",
            Status::Retained => unreachable!(),
        },
        (_, Error::Missing) => "file missing",
        (_, Error::Unstable) => "changing, retry pending",
        (_, Error::Parse) => "invalid file",
        _ => "unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Files(std::path::PathBuf);
    impl Files {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "Keyrill-settings-resource-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            for name in ["en.add", "ru.add"] {
                std::fs::write(path.join(name), b"fixture\n").unwrap();
            }
            Self(path)
        }

        fn preferences(&self) -> Preferences {
            Preferences {
                dictionaries: ["en.add", "ru.add"].map(|name| {
                    DictionaryPath::Explicit(
                        LocalPath::new(self.0.join(name).to_str().unwrap()).unwrap(),
                    )
                }),
                ..Preferences::default()
            }
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let path = self.0.canonicalize().unwrap();
            assert_eq!(
                path.parent(),
                std::env::temp_dir()
                    .canonicalize()
                    .unwrap()
                    .as_path()
                    .into()
            );
            assert!(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("Keyrill-settings-resource-tests-")
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => break,
                    Err(error) if std::time::Instant::now() >= deadline => {
                        panic!("fixture cleanup after worker retirement failed: {error}")
                    }
                    Err(_) => std::thread::yield_now(),
                }
            }
        }
    }

    fn wait_for(state: &State, check: impl Fn(&Readiness) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            send(state.window.get(), WM_TIMER, 1, 0);
            if check(&state.readiness.borrow()) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "native preflight report arrived"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn native_preflight_uses_verified_saved_snapshot_and_restarts_only_after_save_or_reload() {
        let files = Files::new();
        let store = Rc::new(crate::settings::testing::Isolated::new());
        let opened = store.0.load().unwrap();
        let SaveOutcome::Saved(initial) = store.0.save(opened.revision, files.preferences()) else {
            panic!("isolated settings saved");
        };
        let window = crate::settings_ui::tests::Window::new(Source::Preflight(store.clone()));
        wait_for(&window.0, |readiness| readiness.last_good.is_some());
        assert_eq!(window.0.readiness.borrow().saved.as_ref(), Some(&initial));
        let generation = window.0.readiness.borrow().generation;
        std::fs::write(files.0.join("en.add"), b"invalid\0dictionary").unwrap();
        wait_for(&window.0, |readiness| {
            readiness
                .report
                .as_ref()
                .is_some_and(|report| report.sources[0].error == Error::Parse)
        });
        assert!(window.0.readiness.borrow().last_good.is_some());
        assert!(
            get_text(window.0.control(READINESS))
                .unwrap()
                .contains("last good retained")
        );
        std::fs::write(files.0.join("en.add"), b"fixture\n").unwrap();
        window.click(AUTOMATIC);
        assert_eq!(window.0.readiness.borrow().generation, generation);
        window.click(SAVE);
        let saved = store.0.load().unwrap();
        assert_ne!(saved.revision, initial.revision);
        assert_eq!(window.0.readiness.borrow().saved.as_ref(), Some(&saved));
        assert_eq!(window.0.readiness.borrow().generation, generation + 1);
        wait_for(&window.0, |readiness| readiness.last_good.is_some());
        window.click(RELOAD);
        assert_eq!(window.0.readiness.borrow().generation, generation + 2);
        window.click(CLOSE);
        assert!(window.0.readiness.borrow().worker.is_none());
    }

    #[test]
    fn native_preflight_conflict_keeps_prior_revision_report_and_preview_is_isolated() {
        let files = Files::new();
        let store = Rc::new(crate::settings::testing::Isolated::new());
        let opened = store.0.load().unwrap();
        let SaveOutcome::Saved(initial) = store.0.save(opened.revision, files.preferences()) else {
            panic!("isolated settings saved");
        };
        let window = crate::settings_ui::tests::Window::new(Source::Preflight(store.clone()));
        wait_for(&window.0, |readiness| readiness.last_good.is_some());
        let mut changed = initial.value.as_ref().clone();
        changed.switch_source = !changed.switch_source;
        assert!(matches!(
            store.0.save(initial.revision, changed),
            SaveOutcome::Saved(_)
        ));
        window.click(SAVE);
        assert!(window.status().contains("another window"));
        assert_eq!(window.0.readiness.borrow().saved.as_ref(), Some(&initial));
        assert!(window.0.readiness.borrow().last_good.is_some());
        assert!(
            get_text(window.0.control(READINESS))
                .unwrap()
                .contains("previous verified settings")
        );
        let preview =
            crate::settings_ui::tests::Window::new(Source::Preview(RefCell::new(Snapshot {
                value: Arc::new(files.preferences()),
                ..Snapshot::default()
            })));
        preview.click(SAVE);
        send(preview.0.window.get(), WM_TIMER, 1, 0);
        assert!(preview.0.readiness.borrow().worker.is_none());
        assert!(preview.0.readiness.borrow().saved.is_none());
        assert!(
            get_text(preview.0.control(READINESS))
                .unwrap()
                .contains("checks disabled")
        );
        let ordinary_test = crate::settings_ui::tests::Window::new(Source::Test(store));
        assert!(ordinary_test.0.readiness.borrow().worker.is_none());
    }

    #[test]
    fn preflight_mailbox_rejects_stale_generation_and_revision() {
        let mut readiness = Readiness {
            generation: 3,
            ..Default::default()
        };
        readiness.accept(Ready {
            activation: 2,
            revision: 100,
            ..Default::default()
        });
        assert!(readiness.report.is_none());
        readiness.accept(Ready {
            activation: 3,
            revision: 8,
            model: true,
            ..Default::default()
        });
        for revision in [7, 8] {
            readiness.accept(Ready {
                activation: 3,
                revision,
                model: false,
                ..Default::default()
            });
            assert!(readiness.report.as_ref().unwrap().model);
        }
        readiness.accept(Ready {
            activation: 3,
            revision: 9,
            model: false,
            ..Default::default()
        });
        assert!(!readiness.report.as_ref().unwrap().model);
    }
}
