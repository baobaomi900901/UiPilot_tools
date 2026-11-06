//! Image authority shared by dispatch and the fixed OCR service.
use super::{PluginRequestContext, PublicPluginManager};
use crate::launcher_image::LauncherImage;

impl PublicPluginManager {
    pub(crate) fn bound_ocr_image(
        &self,
        context: &PluginRequestContext,
        id: Option<&str>,
    ) -> Result<LauncherImage, &'static str> {
        if let Some(binding) = self
            .panel_images
            .lock()
            .map_err(|_| "expiredRequest")?
            .get(context)
            .cloned()
        {
            return binding.image(id);
        }
        self.launcher_images.bound(context, id)
    }

    pub(crate) fn claim_ocr_image(
        &self,
        context: &PluginRequestContext,
        id: &str,
    ) -> Result<LauncherImage, &'static str> {
        if let Some(binding) = self
            .panel_images
            .lock()
            .map_err(|_| "expiredRequest")?
            .get(context)
            .cloned()
        {
            return binding.claim(id);
        }
        self.launcher_images.claim(context, id)
    }
}
