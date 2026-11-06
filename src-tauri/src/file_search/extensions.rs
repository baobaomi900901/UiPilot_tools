//! Shared Find categories and the host-owned extension allowlist.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileCategory {
    All,
    Folder,
    Excel,
    Word,
    Ppt,
    Pdf,
    Image,
    Video,
    Audio,
    Archive,
}
impl FileCategory {
    pub(crate) fn parse_wire(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "folder" => Some(Self::Folder),
            "excel" => Some(Self::Excel),
            "word" => Some(Self::Word),
            "ppt" => Some(Self::Ppt),
            "pdf" => Some(Self::Pdf),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "archive" => Some(Self::Archive),
            _ => None,
        }
    }
}

macro_rules! extensions {
    ($category:ident, $exts:literal) => {
        (FileCategory::$category, $exts, concat!("file: ext:", $exts))
    };
}

const FILE_TYPES: &[(FileCategory, &str, &str)] = &[
    extensions!(Excel, "xls;xlsx;xlsm;xlsb;csv"),
    extensions!(Word, "doc;docx;docm;rtf"),
    extensions!(Ppt, "ppt;pptx;pptm"),
    extensions!(Pdf, "pdf"),
    extensions!(Image, "bmp;gif;heic;jpeg;jpg;png;svg;tif;tiff;webp"),
    extensions!(Video, "avi;m4v;mkv;mov;mp4;webm;wmv"),
    extensions!(Audio, "aac;flac;m4a;mp3;ogg;wav;wma"),
    extensions!(Archive, "7z;bz2;gz;rar;tar;tgz;zip"),
];

impl FileCategory {
    #[cfg(windows)]
    pub(crate) fn everything_predicate(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Folder => "folder:",
            _ => FILE_TYPES.iter().find(|entry| entry.0 == self)
                .expect("all file categories have a corresponding extension filter").2,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        FILE_TYPES.iter().find(|entry| entry.1.split(';')
            .any(|ext| ext.eq_ignore_ascii_case(extension))).map(|entry| entry.0)
    }
}
