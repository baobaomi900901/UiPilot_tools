//! Controller-gated handoff, imports and image event delivery.
use super::*;
use crate::{
    launcher_image::LauncherImage,
    panel_image::{PanelImageEvent, PanelImages},
    public_plugins::PublicPluginManager,
};

impl PluginPanelController {
    pub(crate) fn configure_images(
        &self,
        epoch: u64,
        image: Option<LauncherImage>,
    ) -> Result<(), &'static str> {
        let mut core = self.core.lock().map_err(|_| "imageUnavailable")?;
        let session = core
            .session
            .as_mut()
            .filter(|s| s.identity.session_epoch == epoch)
            .ok_or("expiredImage")?;
        if session.images.is_some() {
            return Err("expiredImage");
        }
        session.images = Some(PanelImages::new(epoch, image));
        Ok(())
    }

    pub(crate) fn begin_image_import(
        &self,
        manager: &PublicPluginManager,
        caller: &str,
        epoch: u64,
        sequence: u64,
    ) -> Result<(), &'static str> {
        if caller != "main" {
            return Err("invalidCaller");
        }
        let core = self.core.lock().map_err(|_| "imageUnavailable")?;
        let session = core
            .session
            .as_ref()
            .filter(|s| s.identity.session_epoch == epoch)
            .ok_or("expiredImage")?;
        if !manager.panel_images_enabled(&session.identity) {
            return Err("expiredImage");
        }
        session
            .images
            .as_ref()
            .ok_or("expiredImage")?
            .begin_import(sequence)
    }

    pub(crate) fn finish_image_import(
        &self,
        manager: &PublicPluginManager,
        caller: &str,
        epoch: u64,
        sequence: u64,
        image: LauncherImage,
    ) -> Result<LauncherImage, &'static str> {
        if caller != "main" {
            return Err("invalidCaller");
        }
        let mut core = self.core.lock().map_err(|_| "imageUnavailable")?;
        let session = core
            .session
            .as_mut()
            .filter(|s| s.identity.session_epoch == epoch)
            .ok_or("expiredImage")?;
        if !manager.panel_images_enabled(&session.identity) {
            return Err("expiredImage");
        }
        let image = session
            .images
            .as_ref()
            .ok_or("expiredImage")?
            .finish_import(sequence, image)?;
        if let Some(request) = session.runtime_request.as_ref() {
            request.cancel_image();
        }
        Ok(image)
    }

    pub(crate) fn clear_image(
        &self,
        manager: &PublicPluginManager,
        caller: &str,
        epoch: u64,
    ) -> Result<(), crate::public_plugins::panel_action::PanelActionError> {
        use crate::public_plugins::panel_action::PanelActionError as E;
        let mut core = self
            .core
            .lock()
            .map_err(|_| E::PanelRequestUnavailableError)?;
        let session = core
            .session
            .as_mut()
            .filter(|s| s.identity.session_epoch == epoch && s.identity.content_label == caller)
            .ok_or(E::ExpiredPanelSession)?;
        if !manager.panel_images_enabled(&session.identity) {
            return Err(E::ExpiredPanelSession);
        }
        session
            .images
            .as_ref()
            .ok_or(E::ExpiredPanelSession)?
            .replace(None)
            .map_err(|_| E::PanelRequestUnavailableError)?;
        if let Some(request) = session.runtime_request.as_ref() {
            request.cancel_image();
        }
        Ok(())
    }

    pub(crate) fn publish_image_change(
        &self,
        epoch: u64,
        publish: impl FnOnce(&PanelSessionIdentity, &PanelImageEvent) -> Result<(), ()>,
    ) -> Result<bool, PanelSettlementError> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| PanelSettlementError::Unavailable)?;
        let Some(session) = core
            .session
            .as_mut()
            .filter(|s| s.identity.session_epoch == epoch)
        else {
            return Ok(false);
        };
        // Ready must have actually been published; content's handshake alone is insufficient.
        if session.phase != PanelPhase::Ready
            || session.ready_notification.is_none()
            || session.pending.is_some()
        {
            return Ok(false);
        }
        let Some(images) = &session.images else {
            return Ok(false);
        };
        let event = images.event();
        if session.delivered_image_revision.as_ref() == Some(&event.revision) {
            return Ok(false);
        }
        publish(&session.identity, &event).map_err(|_| PanelSettlementError::Unavailable)?;
        session.delivered_image_revision = Some(event.revision);
        Ok(true)
    }
}

pub(crate) fn deliver_image_change<R: tauri::Runtime>(
    app: &AppHandle<R>,
    controller: &PluginPanelController,
    epoch: u64,
) -> Result<bool, PanelSettlementError> {
    controller.publish_image_change(epoch, |identity, event| {
        let content = app.get_webview(&identity.content_label).ok_or(())?;
        let payload = serde_json::to_string(event).map_err(|_| ())?;
        if payload.len() > 64 * 1024 { return Err(()); }
        content.eval(format!("window.dispatchEvent(new CustomEvent('uipilot-plugin-panel-image-changed',{{detail:{payload}}}));")).map_err(|_| ())
    })
}
