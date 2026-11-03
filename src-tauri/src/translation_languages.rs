//! Canonical Youdao text translation languages. Catalog agreement is tested below.
use serde::{Deserialize, Serialize};

macro_rules! languages {
    ($($variant:ident => $code:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub(crate) enum TranslationLanguage {
            $(#[serde(rename = $code)] $variant),+
        }
        pub(crate) const CODES: &[&str] = &[$($code),+];
        impl TranslationLanguage {
            pub(crate) fn from_code(code: &str) -> Option<Self> {
                match code { $($code => Some(Self::$variant),)+ _ => None }
            }
            pub(crate) fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }
        }
    };
}

languages! {
    Ar => "ar",
    De => "de",
    English => "en",
    Es => "es",
    Fr => "fr",
    Hi => "hi",
    Id => "id",
    It => "it",
    Ja => "ja",
    Ko => "ko",
    Nl => "nl",
    Pt => "pt",
    Ru => "ru",
    Th => "th",
    Vi => "vi",
    Chinese => "zh-CHS",
    ZhCht => "zh-CHT",
    Af => "af",
    Am => "am",
    Az => "az",
    Be => "be",
    Bg => "bg",
    Bn => "bn",
    Bs => "bs",
    Ca => "ca",
    Ceb => "ceb",
    Co => "co",
    Cs => "cs",
    Cy => "cy",
    Da => "da",
    El => "el",
    Eo => "eo",
    Et => "et",
    Eu => "eu",
    Fa => "fa",
    Fi => "fi",
    Fj => "fj",
    Fy => "fy",
    Ga => "ga",
    Gd => "gd",
    Gl => "gl",
    Gu => "gu",
    Ha => "ha",
    Haw => "haw",
    He => "he",
    Hr => "hr",
    Ht => "ht",
    Hu => "hu",
    Hy => "hy",
    Ig => "ig",
    Is => "is",
    Jw => "jw",
    Ka => "ka",
    Kk => "kk",
    Km => "km",
    Kn => "kn",
    Ku => "ku",
    Ky => "ky",
    La => "la",
    Lb => "lb",
    Lo => "lo",
    Lt => "lt",
    Lv => "lv",
    Mg => "mg",
    Mi => "mi",
    Mk => "mk",
    Ml => "ml",
    Mn => "mn",
    Mr => "mr",
    Ms => "ms",
    Mt => "mt",
    Mww => "mww",
    My => "my",
    Ne => "ne",
    No => "no",
    Ny => "ny",
    Otq => "otq",
    Pa => "pa",
    Pl => "pl",
    Ps => "ps",
    Ro => "ro",
    Sd => "sd",
    Si => "si",
    Sk => "sk",
    Sl => "sl",
    Sm => "sm",
    Sn => "sn",
    So => "so",
    Sq => "sq",
    SrCyrl => "sr-Cyrl",
    SrLatn => "sr-Latn",
    St => "st",
    Su => "su",
    Sv => "sv",
    Sw => "sw",
    Ta => "ta",
    Te => "te",
    Tg => "tg",
    Tl => "tl",
    Tlh => "tlh",
    To => "to",
    Tr => "tr",
    Ty => "ty",
    Uk => "uk",
    Ur => "ur",
    Uz => "uz",
    Xh => "xh",
    Yi => "yi",
    Yo => "yo",
    Yua => "yua",
    Yue => "yue",
    Zu => "zu",
}

/// Only provider responses are case insensitive. Requests must use canonical codes.
pub(crate) fn normalize_code(value: &str) -> Option<&'static str> {
    CODES
        .iter()
        .copied()
        .find(|code| code.eq_ignore_ascii_case(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_and_serde_have_exactly_the_official_codes() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../docs/plugin-sdk/youdao-languages.json"))
                .unwrap();
        let official: Vec<_> = catalog["languages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|language| language["id"].as_str().unwrap())
            .collect();
        assert_eq!(CODES, official);
        assert_eq!(CODES.len(), 112);
        for code in CODES {
            let language = TranslationLanguage::from_code(code).unwrap();
            assert_eq!(language.as_str(), *code);
            assert_eq!(serde_json::to_value(language).unwrap(), *code);
            assert_eq!(
                serde_json::from_value::<TranslationLanguage>(serde_json::json!(code)).unwrap(),
                language
            );
            assert_eq!(normalize_code(&code.to_ascii_uppercase()), Some(*code));
        }
        for invalid in ["auto", "", "unknown", " en", "EN", "zh-chs", "sr-cyrl"] {
            assert_eq!(TranslationLanguage::from_code(invalid), None);
            assert!(
                serde_json::from_value::<TranslationLanguage>(serde_json::json!(invalid)).is_err()
            );
        }
        assert_eq!(normalize_code("unknown"), None);
        assert_eq!(normalize_code(" en"), None);
    }
}
