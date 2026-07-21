use icu_casemap::CaseMapper;
use unicode_normalization::UnicodeNormalization;

pub(crate) const FOLD_ALGORITHM_ID: &str = "uipilot-unicode-15.1-full-fold-nfc-v1";

pub(crate) fn fold_name(value: &str) -> String {
    let first_nfc: String = value.nfc().collect();
    let folded = CaseMapper::new().fold_string(&first_nfc);
    folded.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::{fold_name, FOLD_ALGORITHM_ID};
    use icu_casemap::CaseMapper;
    use rusqlite::Connection;

    #[test]
    fn dependency_contract_unicode_15_1_and_full_fold() {
        assert_eq!(FOLD_ALGORITHM_ID, "uipilot-unicode-15.1-full-fold-nfc-v1");
        assert_eq!(unicode_normalization::UNICODE_VERSION, (15, 1, 0));
        for (input, expected) in [
            ("UiPilot", "uipilot"),
            ("Straße", "strasse"),
            ("CAFE\u{301}", "café"),
            ("Σ", "σ"),
            ("σ", "σ"),
            ("ς", "σ"),
            ("İ", "i\u{307}"),
            ("Ｕｉ", "ｕｉ"),
        ] {
            assert_eq!(fold_name(input), expected);
        }
        assert_ne!(fold_name("Ｕｉ"), "ui");
        let mapper = CaseMapper::new();
        assert_eq!(mapper.simple_fold('\u{1fd3}'), '\u{0390}');
        assert_eq!(mapper.simple_fold('\u{1fe3}'), '\u{03b0}');
        assert_eq!(mapper.simple_fold('\u{fb05}'), '\u{fb06}');
    }

    #[test]
    fn dependency_contract_bundled_sqlite_identity_and_fts5() {
        let connection = Connection::open_in_memory().unwrap();
        let identity = connection
            .query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap();
        assert_eq!(identity.0, "3.53.2");
        assert_eq!(
            identity.1,
            "2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        connection.execute_batch("CREATE VIRTUAL TABLE names USING fts5(value, tokenize='trigram case_sensitive 1');").unwrap();
    }
}
