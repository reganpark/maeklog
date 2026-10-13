//! App-authored text follows the saved app preference, or the terminal locale in Auto.
//!
//! Nonempty `LC_ALL`, `LC_MESSAGES`, and `LANG` are considered in that order. Korean
//! locales use Korean; all other locales use English. GNU gettext's `LANGUAGE`
//! variable and a desktop's separate language preferences are not consulted.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::{AppResult, language_settings};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Ko,
    En,
}

impl Language {
    pub const fn prompt_name(self) -> &'static str {
        match self {
            Self::Ko => "Korean",
            Self::En => "English",
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
        }
    }
}

/// Auto resolves the terminal locale once per process.
pub fn system() -> Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    *LANGUAGE.get_or_init(|| {
        let lc_all = std::env::var("LC_ALL").ok();
        let lc_messages = std::env::var("LC_MESSAGES").ok();
        let lang = std::env::var("LANG").ok();
        detect(lc_all.as_deref(), lc_messages.as_deref(), lang.as_deref())
    })
}

/// Cached reads never access settings files, including while localizing an error.
pub fn current() -> Language {
    cached().current()
}

/// Startup and a throttled panel poll call this; errors retain the last valid UI language.
pub fn refresh_cached() -> AppResult<bool> {
    cached().refresh(language_settings::fresh_effective())
}

pub(crate) fn cache_language(language: Language) {
    cached().replace(language);
}

fn cached() -> &'static CachedLanguage {
    static CACHE: OnceLock<CachedLanguage> = OnceLock::new();
    CACHE.get_or_init(|| CachedLanguage::new(system()))
}

struct CachedLanguage {
    value: AtomicU8,
}

impl CachedLanguage {
    const fn new(language: Language) -> Self {
        Self {
            value: AtomicU8::new(match language {
                Language::Ko => 0,
                Language::En => 1,
            }),
        }
    }

    fn current(&self) -> Language {
        match self.value.load(Ordering::Relaxed) {
            0 => Language::Ko,
            _ => Language::En,
        }
    }

    fn replace(&self, language: Language) -> bool {
        let value = match language {
            Language::Ko => 0,
            Language::En => 1,
        };
        self.value.swap(value, Ordering::Relaxed) != value
    }

    fn refresh(&self, language: AppResult<Language>) -> AppResult<bool> {
        language.map(|value| self.replace(value))
    }
}

fn detect(lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> Language {
    let locale = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty());
    match locale {
        Some(value)
            if value
                .split(['_', '-', '.', '@'])
                .next()
                .is_some_and(|tag| tag.eq_ignore_ascii_case("ko")) =>
        {
            Language::Ko
        }
        _ => Language::En,
    }
}

/// Selects one expression lazily, so formatted text is computed only once.
#[macro_export]
macro_rules! tr {
    ($ko:literal, $en:literal; $($args:tt)*) => {
        match $crate::i18n::current() {
            $crate::i18n::Language::Ko => format!($ko, $($args)*),
            $crate::i18n::Language::En => format!($en, $($args)*),
        }
    };
    ($ko:expr, $en:expr $(,)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Language::Ko => $ko,
            $crate::i18n::Language::En => $en,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{CachedLanguage, Language, detect};

    #[test]
    fn explicit_refresh_changes_ui_language_and_errors_keep_the_last_valid_value() {
        let cache = CachedLanguage::new(Language::En);
        assert!(!cache.refresh(Ok(Language::En)).unwrap());
        assert!(cache.refresh(Ok(Language::Ko)).unwrap());
        assert_eq!(cache.current(), Language::Ko);
        assert!(cache.refresh(Err("malformed preference".into())).is_err());
        assert_eq!(cache.current(), Language::Ko);
        assert!(!cache.refresh(Ok(Language::Ko)).unwrap());
        assert!(cache.refresh(Ok(Language::En)).unwrap());
    }

    #[test]
    fn honors_locale_precedence_and_ignores_empty_values() {
        assert_eq!(
            detect(Some("en_US.UTF-8"), Some("ko_KR.UTF-8"), Some("ko")),
            Language::En
        );
        assert_eq!(
            detect(Some("ko_KR.UTF-8"), Some("en_US.UTF-8"), Some("en")),
            Language::Ko
        );
        assert_eq!(
            detect(Some(""), Some("ko_KR.UTF-8"), Some("en")),
            Language::Ko
        );
        assert_eq!(detect(Some("  "), Some(""), Some("ko")), Language::Ko);
        assert_eq!(detect(None, Some("en"), Some("ko")), Language::En);
        assert_eq!(detect(None, None, None), Language::En);
    }

    #[test]
    fn accepts_korean_locale_forms() {
        for locale in [
            "ko",
            "ko_KR.UTF-8",
            "ko-KR",
            "KO_kr",
            "ko_KR@variant",
            " ko_KR ",
        ] {
            assert_eq!(detect(None, None, Some(locale)), Language::Ko, "{locale}");
        }
    }

    #[test]
    fn falls_back_to_english_for_other_locales() {
        for locale in [
            "en",
            "en_US.UTF-8",
            "C",
            "C.UTF-8",
            "POSIX",
            "ja_JP.UTF-8",
            "korean",
            "",
        ] {
            assert_eq!(detect(Some(locale), None, None), Language::En, "{locale}");
        }
        assert_eq!(
            detect(Some("C"), Some("ko_KR"), Some("ko_KR")),
            Language::En
        );
        assert_eq!(
            detect(Some("ja_JP"), Some("ko_KR"), Some("ko_KR")),
            Language::En
        );
    }

    #[test]
    fn language_tags_are_stable_profile_values() {
        assert_eq!(Language::Ko.prompt_name(), "Korean");
        assert_eq!(Language::En.prompt_name(), "English");
        assert_eq!(Language::Ko.tag(), "ko");
        assert_eq!(Language::En.tag(), "en");
        assert_eq!(serde_json::to_string(&Language::Ko).unwrap(), "\"ko\"");
        assert_eq!(
            serde_json::from_str::<Language>("\"en\"").unwrap(),
            Language::En
        );
    }
}
