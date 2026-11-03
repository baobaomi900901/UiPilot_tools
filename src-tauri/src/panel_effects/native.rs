use super::*;
#[cfg(windows)]
mod sapi {
    use super::*;
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{
            Globalization::{LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES},
            Media::Speech::*,
            System::Com::*,
        },
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
    fn language_from_locale(locale: &str) -> Option<SpeechLanguage> {
        let locale = locale.to_ascii_lowercase();
        let mut parts = locale.split('-');
        let primary = parts.next()?;
        let code = match primary {
            // Windows speech uses zh-HK for Cantonese, not Mandarin. Ambiguous
            // Chinese locales (including Macao) do not advertise a fallback.
            "zh" => match locale.as_str() {
                "zh-cn" | "zh-sg" | "zh-hans" | "zh-hans-cn" | "zh-hans-sg" => "zh-CHS",
                "zh-tw" | "zh-hant" | "zh-hant-tw" => "zh-CHT",
                "zh-hk" | "zh-hant-hk" => "yue",
                _ => return None,
            },
            "yue" => match locale.as_str() {
                "yue" | "yue-hk" | "yue-hant" | "yue-hant-hk" => "yue",
                _ => return None,
            },
            "sr" => match parts.next() {
                Some("latn") => "sr-Latn",
                Some("cyrl") => "sr-Cyrl",
                _ => return None,
            },
            // The provider's Bosnian entry is explicitly Latin.
            "bs" if parts.next() == Some("cyrl") => return None,
            "fil" => "tl",
            "jv" => "jw",
            "nb" => "no",
            _ => primary,
        };
        SpeechLanguage::from_code(code)
    }
    pub(super) fn languages(value: &str) -> Vec<SpeechLanguage> {
        let mut result = Vec::new();
        for id in value
            .split(';')
            .filter_map(|part| u16::from_str_radix(part.trim(), 16).ok())
        {
            // Default/custom LCIDs can resolve to the current user's language,
            // which is not evidence that this token can speak that language.
            if id & 0x3ff == 0 {
                continue;
            }
            let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH, including NUL.
            let length = unsafe {
                LCIDToLocaleName(u32::from(id), Some(&mut name), LOCALE_ALLOW_NEUTRAL_NAMES)
            };
            if length <= 1 {
                continue;
            }
            let Ok(locale) = String::from_utf16(&name[..length as usize - 1]) else {
                continue;
            };
            if let Some(language) = language_from_locale(&locale) {
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
        fn language_ids_map_installed_locales_without_collapsing_languages() {
            let actual: Vec<_> =
                languages("409;809;804;404;411;412;40c;401;81a;c1a;c04;bad!;ffffffff")
                    .into_iter()
                    .map(|language| serde_json::to_value(language).unwrap())
                    .collect();
            assert_eq!(
                actual,
                vec!["en", "zh-CHS", "zh-CHT", "ja", "ko", "fr", "ar", "sr-Latn", "sr-Cyrl", "yue"]
            );
        }
        #[test]
        fn locale_mapping_preserves_scripts_and_provider_aliases() {
            for (locale, expected) in [
                ("ja-JP", "ja"),
                ("ko-KR", "ko"),
                ("fr-CA", "fr"),
                ("ar-SA", "ar"),
                ("zh-CN", "zh-CHS"),
                ("zh-SG", "zh-CHS"),
                ("zh-TW", "zh-CHT"),
                ("zh-HK", "yue"),
                ("yue-Hant-HK", "yue"),
                ("sr-Latn-RS", "sr-Latn"),
                ("sr-Cyrl-RS", "sr-Cyrl"),
                ("bs-Latn-BA", "bs"),
                ("fil-PH", "tl"),
                ("jv-Latn-ID", "jw"),
                ("nb-NO", "no"),
            ] {
                assert_eq!(
                    language_from_locale(locale).map(|language| language.as_str()),
                    Some(expected),
                    "{locale}"
                );
            }
            for locale in [
                "",
                "xx-XX",
                "auto",
                "zh",
                "zh-MO",
                "zh-Hans-HK",
                "yue-Hans-CN",
                "sr",
                "sr-RS",
                "bs-Cyrl-BA",
                "nn-NO",
                "gsw-CH",
            ] {
                assert_eq!(language_from_locale(locale), None, "{locale}");
            }
        }
        #[test]
        fn unknown_and_user_default_lcids_never_select_an_unrelated_voice() {
            assert!(languages("0;400;800;c00;1000;ffff;bad!").is_empty());
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
