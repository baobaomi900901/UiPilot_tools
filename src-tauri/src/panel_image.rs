//! Session-owned image snapshots. Runtime bindings are weak, revision-scoped capabilities.
use crate::launcher_image::LauncherImage;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PanelImageEvent {
    pub(crate) session_epoch: String,
    pub(crate) revision: String,
    pub(crate) image: Option<LauncherImage>,
}

struct State {
    revision: u64,
    sequence: u64,
    pending: Option<u64>,
    image: Option<LauncherImage>,
}

pub(crate) struct PanelImages {
    epoch: u64,
    state: Mutex<State>,
}

impl PanelImages {
    pub(crate) fn new(epoch: u64, image: Option<LauncherImage>) -> Arc<Self> {
        let images = Arc::new(Self {
            epoch,
            state: Mutex::new(State {
                revision: 0,
                sequence: 0,
                pending: None,
                image: None,
            }),
        });
        images.replace(image).expect("first image revision");
        images
    }
    fn replace_locked(
        &self,
        state: &mut State,
        mut image: Option<LauncherImage>,
    ) -> Result<(), &'static str> {
        let revision = state.revision.checked_add(1).ok_or("imageUnavailable")?;
        if let Some(image) = &mut image {
            image.descriptor.id = format!("panel-image-{}-{revision}", self.epoch);
        }
        state.revision = revision;
        state.pending = None;
        state.image = image;
        Ok(())
    }
    pub(crate) fn replace(&self, image: Option<LauncherImage>) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        self.replace_locked(&mut state, image)
    }
    pub(crate) fn begin_import(&self, sequence: u64) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if sequence == 0 || sequence <= state.sequence {
            return Err("expiredImage");
        }
        state.sequence = sequence;
        state.pending = Some(sequence);
        Ok(())
    }
    pub(crate) fn finish_import(
        &self,
        sequence: u64,
        image: LauncherImage,
    ) -> Result<LauncherImage, &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if state.pending != Some(sequence) {
            return Err("expiredImage");
        }
        self.replace_locked(&mut state, Some(image))?;
        Ok(state.image.as_ref().expect("just replaced").clone())
    }
    pub(crate) fn event(&self) -> PanelImageEvent {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        PanelImageEvent {
            session_epoch: self.epoch.to_string(),
            revision: state.revision.to_string(),
            image: state.image.clone(),
        }
    }
    pub(crate) fn bind(
        self: &Arc<Self>,
        id: &str,
        request: &str,
    ) -> Result<Arc<PanelImageBinding>, &'static str> {
        let state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if !state.image.as_ref().is_some_and(|image| image.id == id) {
            return Err("expiredImage");
        }
        Ok(Arc::new(PanelImageBinding {
            session: Arc::downgrade(self),
            revision: state.revision,
            id: format!("{id}-{request}"),
            claimed: AtomicBool::new(false),
        }))
    }
}

pub(crate) struct PanelImageBinding {
    session: Weak<PanelImages>,
    revision: u64,
    id: String,
    claimed: AtomicBool,
}
impl PanelImageBinding {
    pub(crate) fn image(&self, id: Option<&str>) -> Result<LauncherImage, &'static str> {
        if id.is_some_and(|id| id != self.id) {
            return Err("expiredRequest");
        }
        let session = self.session.upgrade().ok_or("expiredRequest")?;
        let state = session.state.lock().map_err(|_| "expiredRequest")?;
        if state.revision != self.revision {
            return Err("expiredRequest");
        }
        let mut image = state.image.clone().ok_or("expiredRequest")?;
        image.descriptor.id = self.id.clone();
        Ok(image)
    }
    pub(crate) fn claim(&self, id: &str) -> Result<LauncherImage, &'static str> {
        let image = self.image(Some(id))?;
        if self.claimed.swap(true, Ordering::AcqRel) {
            return Err("serviceAlreadyUsed");
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher_image::{normalize_rgba, LauncherImageStore};

    #[test]
    fn handoff_survives_launcher_clear_but_binding_dies_on_replace_clear_and_drop() {
        let launcher = LauncherImageStore::default();
        launcher.begin_import(1).unwrap();
        let original = launcher
            .finish_import(1, normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap())
            .unwrap();
        let session = PanelImages::new(7, Some(original.clone()));
        let event = session.event();
        let id = event.image.unwrap().id.clone();
        assert_ne!(id, original.id);
        let binding = session.bind(&id, "request-1").unwrap();
        launcher.clear(2).unwrap();
        assert_eq!(binding.image(None).unwrap().png, original.png);
        assert_ne!(binding.image(None).unwrap().id, id);
        assert!(binding.image(Some(&id)).is_err());
        session.replace(Some(original.clone())).unwrap();
        assert!(binding.image(None).is_err());
        let binding = session
            .bind(&session.event().image.unwrap().id, "request-2")
            .unwrap();
        session.replace(None).unwrap();
        assert!(binding.image(None).is_err());
        assert!(session.event().image.is_none());
        session.replace(Some(original)).unwrap();
        let binding = session
            .bind(&session.event().image.unwrap().id, "request-3")
            .unwrap();
        drop(session);
        assert!(binding.image(None).is_err());
    }

    #[test]
    fn imports_are_ordered_and_bad_decode_does_not_replace_current_snapshot() {
        let session = PanelImages::new(9, Some(normalize_rgba(1, 1, &[1, 2, 3, 255]).unwrap()));
        let before = session.event();
        session.begin_import(1).unwrap();
        session.begin_import(2).unwrap();
        assert!(session
            .finish_import(1, normalize_rgba(1, 1, &[4, 5, 6, 255]).unwrap())
            .is_err());
        assert_eq!(session.event().revision, before.revision);
        session
            .finish_import(2, normalize_rgba(1, 1, &[4, 5, 6, 255]).unwrap())
            .unwrap();
        assert_ne!(session.event().revision, before.revision);
        session.begin_import(3).unwrap();
        session.replace(None).unwrap();
        assert!(session
            .finish_import(3, normalize_rgba(1, 1, &[4, 5, 6, 255]).unwrap())
            .is_err());
    }
}
