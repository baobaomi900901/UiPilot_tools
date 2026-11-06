use super::*;
use crate::plugin_share::{Delivery, Payload, Phase, Receiver, Session, View};

impl PluginPanelController {
    pub(crate) fn share_begin(
        &self,
        caller: &str,
        epoch: u64,
        payload: Payload,
        rows: Vec<Receiver>,
    ) -> Result<(), ()> {
        if !payload.valid() || !crate::plugin_share::envelope(&(epoch.to_string(), &payload)) {
            return Err(());
        }
        let mut core = self.core.lock().map_err(|_| ())?;
        let source = core
            .session
            .as_ref()
            .filter(|s| {
                s.identity.content_label == caller
                    && s.identity.session_epoch == epoch
                    && s.phase == PanelPhase::Ready
            })
            .ok_or(())?
            .identity
            .clone();
        if core.share.as_ref().is_some_and(|s| s.locked()) {
            return Err(());
        }
        core.next_share = core.next_share.checked_add(1).ok_or(())?;
        let share = Session::new(core.next_share, source, payload, rows);
        if !crate::plugin_share::envelope(&share.view) {
            return Err(());
        }
        core.share = Some(share);
        // Internal child -> main focus transfer, rather than an application blur.
        core.main_content_focused = true;
        core.internal_blur_until = Instant::now().checked_add(INTERNAL_BLUR_GRACE);
        if let Some(s) = core.session.as_mut() {
            s.content_visible = false;
            s.content_focused = false;
        }
        Ok(())
    }
    pub(crate) fn share_view(&self) -> Option<View> {
        let mut core = self.core.lock().ok()?;
        let share = core.share.as_mut()?;
        if Instant::now() >= share.deadline
            && matches!(share.view.phase, Phase::Opening | Phase::Delivering)
        {
            share.fail();
        }
        Some(share.view.clone())
    }
    pub(crate) fn share_select(
        &self,
        id: &str,
        plugin: &str,
        receiver: &str,
    ) -> Result<Receiver, ()> {
        let mut core = self.core.lock().map_err(|_| ())?;
        let source = core.session.as_ref().ok_or(())?.identity.clone();
        let s = core
            .share
            .as_mut()
            .filter(|s| {
                s.view.transfer_id == id && s.source == source && s.view.phase == Phase::Choosing
            })
            .ok_or(())?;
        let r = s
            .view
            .receivers
            .iter()
            .find(|r| {
                r.plugin_id == plugin && r.receiver_id == receiver && r.disabled_reason.is_none()
            })
            .cloned()
            .ok_or(())?;
        s.view.phase = Phase::Opening;
        s.view.target_label = Some(r.plugin_name.clone());
        s.view.receivers.clear();
        s.deadline = Instant::now() + Duration::from_secs(30);
        s.selected = Some(r.clone());
        Ok(r)
    }
    pub(crate) fn share_target(
        &self,
        id: &str,
        target: crate::commands::PluginPanelCommandResult,
    ) -> Result<(), ()> {
        let mut core = self.core.lock().map_err(|_| ())?;
        let identity = core.session.as_ref().ok_or(())?.identity.clone();
        let s = core
            .share
            .as_mut()
            .filter(|s| {
                s.view.transfer_id == id
                    && s.view.phase == Phase::Opening
                    && s.target.as_ref() == Some(&identity)
            })
            .ok_or(())?;
        s.view.target = Some(target);
        s.view.phase = Phase::Delivering;
        Ok(())
    }
    pub(crate) fn share_claim(
        &self,
        caller: &str,
        epoch: u64,
        request: &str,
    ) -> Result<Option<Delivery>, ()> {
        let mut core = self.core.lock().map_err(|_| ())?;
        let live = core
            .session
            .as_ref()
            .filter(|s| {
                s.identity.content_label == caller
                    && s.identity.session_epoch == epoch
                    && s.phase == PanelPhase::Ready
                    && s.current_request_id == request
                    && s.ready_notification.is_some()
            })
            .ok_or(())?
            .identity
            .clone();
        let Some(s) = core.share.as_mut() else {
            return Ok(None);
        };
        if s.target.as_ref() != Some(&live) {
            return Ok(None);
        }
        if s.view.phase != Phase::Delivering || s.claimed || Instant::now() >= s.deadline {
            return Err(());
        }
        s.claimed = true;
        s.request_id = Some(request.into());
        let delivery = Delivery {
            version: 1,
            transfer_id: s.view.transfer_id.clone(),
            receiver_id: s.selected.as_ref().ok_or(())?.receiver_id.clone(),
            session_epoch: epoch.to_string(),
            payload: s.payload.take().ok_or(())?,
        };
        if !crate::plugin_share::envelope(&delivery) {
            s.fail();
            return Err(());
        }
        Ok(Some(delivery))
    }
    pub(crate) fn share_received(
        &self,
        caller: &str,
        epoch: u64,
        request: &str,
        id: &str,
        accepted: bool,
    ) -> Result<(), ()> {
        let mut core = self.core.lock().map_err(|_| ())?;
        let live = core
            .session
            .as_ref()
            .filter(|s| {
                s.identity.content_label == caller
                    && s.identity.session_epoch == epoch
                    && s.current_request_id == request
            })
            .ok_or(())?
            .identity
            .clone();
        let s = core
            .share
            .as_mut()
            .filter(|s| {
                s.target.as_ref() == Some(&live)
                    && s.view.transfer_id == id
                    && s.request_id.as_deref() == Some(request)
                    && s.claimed
                    && s.view.phase == Phase::Delivering
            })
            .ok_or(())?;
        if accepted && Instant::now() < s.deadline {
            s.view.phase = Phase::Accepted;
        } else {
            s.fail();
        }
        Ok(())
    }
    pub(crate) fn share_fail(&self, id: &str) {
        if let Ok(mut core) = self.core.lock() {
            if let Some(s) = core.share.as_mut().filter(|s| s.view.transfer_id == id) {
                s.fail();
            }
        }
    }
    pub(crate) fn share_failed_identity(&self, id: &str) -> Option<PanelSessionIdentity> {
        let core = self.core.lock().ok()?;
        let share = core
            .share
            .as_ref()
            .filter(|s| s.view.transfer_id == id && s.view.phase == Phase::Error)?;
        let live = &core.session.as_ref()?.identity;
        (share.target.as_ref() == Some(live) || &share.source == live).then(|| live.clone())
    }
    pub(crate) fn share_locked(&self) -> bool {
        self.core
            .lock()
            .ok()
            .is_some_and(|c| c.share.as_ref().is_some_and(|s| s.locked()))
    }
    pub(crate) fn share_end(&self, id: &str) -> Result<Option<PanelSessionIdentity>, ()> {
        let mut core = self.core.lock().map_err(|_| ())?;
        let s = core
            .share
            .as_ref()
            .filter(|s| s.view.transfer_id == id)
            .ok_or(())?;
        let restore = (s.view.phase == Phase::Choosing).then(|| s.source.clone());
        core.share = None;
        Ok(restore.filter(|i| core.session.as_ref().is_some_and(|s| s.identity == *i)))
    }
}

#[cfg(test)]
impl PluginPanelController {
    pub(crate) fn share_test_opening_target(&self) -> (PanelSessionIdentity, PanelSessionIdentity) {
        fn owner(plugin: &str) -> PanelOwner {
            PanelOwner {
                plugin_id: plugin.into(),
                plugin_generation: 1,
                activation_id: 2,
                admission_epoch: 3,
                command_label: plugin.into(),
                request_id: "init".into(),
                submission_token: "submit".into(),
                argument: String::new(),
                host_keys: vec![],
                host_key_focus: PanelHostKeyFocus::Content,
            }
        }
        let source = self.open_session(owner("com.test.ocr")).unwrap();
        self.core.lock().unwrap().session.as_mut().unwrap().phase = PanelPhase::Ready;
        let receiver = Receiver {
            plugin_id: "com.test.translate".into(),
            plugin_name: "Translate".into(),
            receiver_id: "translate".into(),
            title: "Translate".into(),
            command_id: "default".into(),
            max_bytes: 8192,
            generation: 1,
            activation_id: 2,
            admission_epoch: 3,
            disabled_reason: None,
        };
        self.share_begin(
            &source.content_label,
            source.session_epoch,
            Payload {
                r#type: "text/plain".into(),
                text: " edited\n原文  ".into(),
            },
            vec![receiver],
        )
        .unwrap();
        self.share_select("1", "com.test.translate", "translate")
            .unwrap();
        let target = self
            .replace_session(owner("com.test.translate"), Some("1"))
            .unwrap()
            .0;
        assert_eq!(self.share_view().unwrap().phase, Phase::Opening);
        assert_eq!(self.live_identity(), Some(target.clone()));
        (source, target)
    }

    pub(crate) fn share_test_target(&self) -> (PanelSessionIdentity, PanelSessionIdentity) {
        let (source, target) = self.share_test_opening_target();
        let mut core = self.core.lock().unwrap();
        let live = core.session.as_mut().unwrap();
        live.phase = PanelPhase::Ready;
        live.ready_notification = Some(PanelDeliveryTicket {
            session_epoch: target.session_epoch,
            request_id: "init".into(),
            submission_token: "submit".into(),
        });
        core.share.as_mut().unwrap().view.phase = Phase::Delivering;
        (source, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_share_opening_target_ready_failure_retains_error_without_descriptor() {
        let c = PluginPanelController::default();
        let (source, target) = c.share_test_opening_target();
        // A late source teardown cannot retire the replacement target.
        assert!(c.teardown_session(Some(source.session_epoch)).is_none());
        assert_eq!(c.share_view().unwrap().phase, Phase::Opening);
        assert!(c
            .core
            .lock()
            .unwrap()
            .share
            .as_ref()
            .unwrap()
            .payload
            .is_some());
        // mount_scoped uses this controller timeout -> teardown before share_target.
        assert_eq!(
            c.wait_until_ready(target.session_epoch, Duration::ZERO),
            Err(PublicPluginManagementError::RuntimeNotReady)
        );
        assert_eq!(
            c.teardown_session(Some(target.session_epoch)),
            Some(target.clone())
        );
        let view = c
            .share_view()
            .expect("failed opening target must retain its error");
        assert_eq!(view.phase, Phase::Error);
        assert_eq!(view.target_label.as_deref(), Some("Translate"));
        assert!(
            view.target.is_none(),
            "mount never returned a target descriptor"
        );
        assert!(view.error.is_some());
        assert!(
            c.live_identity().is_none(),
            "neither target nor source is restored"
        );
        assert!(c
            .core
            .lock()
            .unwrap()
            .share
            .as_ref()
            .unwrap()
            .payload
            .is_none());
        // Actual DTO consumed by pollShare while its panel is still the source.
        let dto = serde_json::to_value(view).unwrap();
        assert_eq!(dto["sourceEpoch"], source.session_epoch.to_string());
        assert_eq!(dto["phase"], "error");
        assert_eq!(dto["targetLabel"], "Translate");
        assert!(dto["target"].is_null());
        assert!(!dto.to_string().contains("原文"));
        assert!(!c.mark_ready(&target.content_label, target.session_epoch, true, false));
        assert!(c
            .share_claim(&target.content_label, target.session_epoch, "init")
            .is_err());
        assert!(c
            .share_received(
                &target.content_label,
                target.session_epoch,
                "init",
                "1",
                true
            )
            .is_err());
        assert!(c
            .replace_session(test_owner("com.test.translate"), Some("1"))
            .is_none());
        assert_eq!(c.share_view().unwrap().phase, Phase::Error);
        assert!(c.share_end("1").unwrap().is_none());
        assert!(c.share_view().is_none());
    }

    fn test_owner(plugin: &str) -> PanelOwner {
        PanelOwner {
            plugin_id: plugin.into(),
            plugin_generation: 1,
            activation_id: 2,
            admission_epoch: 3,
            command_label: plugin.into(),
            request_id: "new".into(),
            submission_token: "new".into(),
            argument: String::new(),
            host_keys: vec![],
            host_key_focus: PanelHostKeyFocus::Content,
        }
    }

    #[test]
    fn plugin_share_opening_target_teardown_cannot_poison_new_navigation_or_close() {
        let c = PluginPanelController::default();
        let (_, target) = c.share_test_opening_target();
        let new = c.open_session(test_owner("com.test.new")).unwrap();
        assert!(c.teardown_session(Some(target.session_epoch)).is_none());
        c.share_fail("1"); // late plugin_share_select error completion
        assert!(c.share_view().is_none());
        assert_eq!(c.live_identity(), Some(new.clone()));
        assert_eq!(c.teardown_session(Some(new.session_epoch)), Some(new));
        assert!(c.share_view().is_none());
        assert!(c.live_identity().is_none());
    }

    #[test]
    fn plugin_share_claim_once_and_receiver_failure_never_restores_source() {
        let c = PluginPanelController::default();
        let (source, target) = c.share_test_target();
        assert!(c.share_locked());
        assert!(c
            .share_claim(&source.content_label, source.session_epoch, "init")
            .is_err());
        assert!(c
            .share_claim(&target.content_label, target.session_epoch, "old-init")
            .is_err());
        let body = c
            .share_claim(&target.content_label, target.session_epoch, "init")
            .unwrap()
            .unwrap();
        assert_eq!(body.payload.text, " edited\n原文  ");
        assert!(c
            .share_claim(&target.content_label, target.session_epoch, "init")
            .is_err());
        c.share_received(
            &target.content_label,
            target.session_epoch,
            "init",
            "1",
            false,
        )
        .unwrap();
        assert_eq!(c.share_view().unwrap().phase, Phase::Error);
        assert_eq!(c.live_identity(), Some(target));
        assert!(c
            .core
            .lock()
            .unwrap()
            .share
            .as_ref()
            .unwrap()
            .payload
            .is_none());
        assert!(c.share_end("1").unwrap().is_none());
    }
    #[test]
    fn plugin_share_late_receipt_and_mount_cannot_overwrite_new_document() {
        let c = PluginPanelController::default();
        let (_, target) = c.share_test_target();
        c.share_claim(&target.content_label, target.session_epoch, "init")
            .unwrap();
        let owner = PanelOwner {
            plugin_id: "com.test.new".into(),
            plugin_generation: 4,
            activation_id: 5,
            admission_epoch: 6,
            command_label: "new".into(),
            request_id: "new".into(),
            submission_token: "new".into(),
            argument: String::new(),
            host_keys: vec![],
            host_key_focus: PanelHostKeyFocus::Content,
        };
        let new = c.open_session(owner.clone()).unwrap();
        assert!(c
            .share_received(
                &target.content_label,
                target.session_epoch,
                "init",
                "1",
                true
            )
            .is_err());
        assert!(c.replace_session(owner, Some("1")).is_none());
        assert_eq!(c.live_identity(), Some(new));
        assert!(c.share_view().is_none());
    }
    #[test]
    fn plugin_share_timeout_releases_text_and_stays_target_error() {
        let c = PluginPanelController::default();
        let (_, target) = c.share_test_target();
        c.core.lock().unwrap().share.as_mut().unwrap().deadline = Instant::now();
        assert_eq!(c.share_view().unwrap().phase, Phase::Error);
        assert_eq!(c.live_identity(), Some(target));
        assert!(c.share_locked());
        c.host_hidden();
        assert!(c.share_view().is_none());
    }
}
