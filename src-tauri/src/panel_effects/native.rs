use super::*;
#[cfg(windows)]
mod sapi {
    use super::*;
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{Media::Speech::*, System::Com::*},
    };
    struct ComApartment;
    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct Voice {
        dto: SpeechVoice,
        token: ISpObjectToken,
    }
    pub(crate) struct NativeSpeech {
        // Rust drops fields in declaration order: interfaces before COM apartment.
        voice: Option<ISpVoice>,
        voices: Vec<Voice>,
        apartment: Option<ComApartment>,
    }
    unsafe fn take_string(value: PWSTR) -> String {
        let text = unsafe { value.to_string() }.unwrap_or_default();
        unsafe {
            CoTaskMemFree(Some(value.0.cast()));
        }
        text
    }
    pub(super) fn languages(value: &str) -> Vec<SpeechLanguage> {
        let mut result = Vec::new();
        for id in value
            .split(';')
            .filter_map(|part| u16::from_str_radix(part.trim(), 16).ok())
        {
            let language = if id & 0x3ff == 0x09 {
                Some(SpeechLanguage::English)
            } else if id == 0x0804 {
                Some(SpeechLanguage::Chinese)
            } else {
                None
            };
            if let Some(language) = language {
                if !result.contains(&language) {
                    result.push(language);
                }
            }
        }
        result
    }
    impl NativeSpeech {
        pub(crate) fn new() -> Self {
            let mut result = Self {
                voice: None,
                voices: Vec::new(),
                apartment: None,
            };
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    return result;
                }
                result.apartment = Some(ComApartment);
                let init = || -> windows::core::Result<(ISpVoice, Vec<Voice>)> {
                    let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_INPROC_SERVER)?;
                    let category: ISpObjectTokenCategory =
                        CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_INPROC_SERVER)?;
                    category.SetId(SPCAT_VOICES, false)?;
                    let tokens = category.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
                    let mut count = 0;
                    tokens.GetCount(&mut count)?;
                    let mut voices = Vec::new();
                    for index in 0..count.min(512) {
                        let token = tokens.Item(index)?;
                        let attributes = token.OpenKey(w!("Attributes"))?;
                        let languages =
                            languages(&take_string(attributes.GetStringValue(w!("Language"))?));
                        if languages.is_empty() {
                            continue;
                        }
                        let id = take_string(token.GetId()?);
                        let name = take_string(token.GetStringValue(PCWSTR::null())?);
                        for language in languages {
                            voices.push(Voice {
                                dto: SpeechVoice {
                                    id: id.clone(),
                                    name: name.clone(),
                                    language,
                                },
                                token: token.clone(),
                            });
                        }
                    }
                    Ok((voice, voices))
                };
                if let Ok((voice, voices)) = init() {
                    result.voice = Some(voice);
                    result.voices = voices;
                }
            }
            result
        }
    }
    impl SpeechDriver for NativeSpeech {
        fn voices(&mut self) -> Result<Vec<SpeechVoice>, EffectError> {
            if self.voice.is_none() {
                return Err(EffectError::SpeechUnavailable);
            }
            Ok(self.voices.iter().map(|v| v.dto.clone()).collect())
        }
        fn start(&mut self, text: &str, language: SpeechLanguage) -> Result<u32, EffectError> {
            let voice = self.voice.as_ref().ok_or(EffectError::SpeechUnavailable)?;
            let selected = self
                .voices
                .iter()
                .find(|v| v.dto.language == language)
                .ok_or(EffectError::SpeechVoiceUnavailable)?;
            let text = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let mut stream = 0;
            unsafe {
                voice
                    .SetVoice(&selected.token)
                    .map_err(|_| EffectError::SpeechFailed)?;
                voice
                    .Speak(
                        PCWSTR(text.as_ptr()),
                        (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32,
                        Some(&mut stream),
                    )
                    .map_err(|_| EffectError::SpeechFailed)?;
            }
            Ok(stream)
        }
        fn stop(&mut self) -> Result<(), EffectError> {
            if let Some(voice) = &self.voice {
                unsafe {
                    voice
                        .Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None)
                        .map_err(|_| EffectError::SpeechFailed)?;
                }
            }
            Ok(())
        }
        fn finished(&mut self, stream: u32) -> Result<bool, EffectError> {
            let voice = self.voice.as_ref().ok_or(EffectError::SpeechUnavailable)?;
            let mut status = SPVOICESTATUS::default();
            unsafe {
                voice
                    .GetStatus(&mut status, std::ptr::null_mut())
                    .map_err(|_| EffectError::SpeechFailed)?;
            }
            if status.ulCurrentStream != stream {
                return Ok(false);
            }
            if status.hrLastResult.is_err() {
                return Err(EffectError::SpeechFailed);
            }
            Ok(status.dwRunningState == SPRS_DONE.0 as u32)
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn language_ids_are_exact_and_local() {
            assert_eq!(
                languages("409;809;804;404;411"),
                vec![SpeechLanguage::English, SpeechLanguage::Chinese]
            );
            assert!(languages("404;411;bad!").is_empty());
        }
    }
}
#[cfg(windows)]
pub(super) use sapi::NativeSpeech;
#[cfg(not(windows))]
pub(super) struct NativeSpeech;
#[cfg(not(windows))]
impl NativeSpeech {
    pub(super) fn new() -> Self {
        Self
    }
}
#[cfg(not(windows))]
impl SpeechDriver for NativeSpeech {
    fn voices(&mut self) -> Result<Vec<SpeechVoice>, EffectError> {
        Err(EffectError::SpeechUnavailable)
    }
    fn start(&mut self, _: &str, _: SpeechLanguage) -> Result<u32, EffectError> {
        Err(EffectError::SpeechUnavailable)
    }
    fn stop(&mut self) -> Result<(), EffectError> {
        Ok(())
    }
    fn finished(&mut self, _: u32) -> Result<bool, EffectError> {
        Ok(false)
    }
}
