//! App-wide UI and generated-summary language, independent of model preferences.
use crate::i18n::{self, Language};
use crate::{AppResult, config, storage};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SETTINGS_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Preference {
    #[default]
    Auto,
    En,
    Ko,
}

impl Preference {
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::En => "en",
            Self::Ko => "ko",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::En => "English",
            Self::Ko => "한국어",
        }
    }

    const fn resolve(self, system: Language) -> Language {
        match self {
            Self::Auto => system,
            Self::En => Language::En,
            Self::Ko => Language::Ko,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPreference {
    language: Preference,
}

pub fn effective(preference: Preference) -> Language {
    preference.resolve(i18n::system())
}

/// A strict fresh read for execution-profile freezing; invalid preferences abort
/// the request, while app-authored error messages use the last cached UI language.
pub fn fresh_effective() -> AppResult<Language> {
    load().map(effective)
}

pub fn path() -> AppResult<PathBuf> {
    Ok(config::model_settings_config_dir()?.join("language-settings.json"))
}

pub fn load() -> AppResult<Preference> {
    load_at(&path()?)
}

pub fn save(preference: Preference) -> AppResult<()> {
    save_at(&path()?, preference)?;
    // Publish only committed settings. Other panels use refresh_cached on their poll.
    i18n::cache_language(effective(preference));
    Ok(())
}

fn ensure_regular_or_missing(path: &Path) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => Err(crate::tr!(
            "언어 설정 경로는 일반 파일이어야 합니다 · 기존 설정 유지",
            "Language settings path must be a regular file; previous settings retained"
        )
        .into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn lock(path: &Path) -> AppResult<File> {
    let parent = path
        .parent()
        .ok_or("language settings path has no parent")?;
    config::private_dir(parent.to_path_buf())?;
    let lock_path = parent.join(".language-settings.lock");
    ensure_regular_or_missing(&lock_path)?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        if let Some(file) = storage::try_lock_file(&lock_path)? {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            return Ok(file);
        }
        if Instant::now() >= deadline {
            return Err(crate::tr!(
                "언어 설정이 사용 중입니다 · 기존 설정 유지 · 잠시 후 다시 시도하세요",
                "Language settings are busy; previous settings retained; retry shortly"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn load_at(path: &Path) -> AppResult<Preference> {
    // Absent preferences preserve Auto and do not create directories or lock files.
    if fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        return Ok(Preference::Auto);
    }
    let _lock = lock(path)?;
    read_locked(path)
}

fn read_locked(path: &Path) -> AppResult<Preference> {
    ensure_regular_or_missing(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Preference::Auto),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(crate::tr!(
            "언어 설정 파일이 너무 큽니다 · 기존 설정 유지",
            "Language settings file is too large; previous settings retained"
        )
        .into());
    }
    serde_json::from_slice::<StoredPreference>(&bytes)
        .map(|settings| settings.language)
        .map_err(|_| {
            crate::tr!(
                "언어 설정 파일을 읽을 수 없습니다 · 기존 설정과 화면 언어 유지",
                "Language settings file is malformed; previous settings and UI language retained"
            )
            .into()
        })
}

fn save_at(path: &Path, preference: Preference) -> AppResult<()> {
    let _lock = lock(path)?;
    // Do not replace malformed or unexpected existing preference data.
    read_locked(path)?;
    storage::atomic_write_json(
        path,
        &StoredPreference {
            language: preference,
        },
    )
}

pub fn cli(arguments: &[String]) -> AppResult<()> {
    match arguments {
        [action] if action == "show" => show(),
        [action, tag] if action == "set" => {
            let preference = match tag.as_str() {
                "auto" => Preference::Auto,
                "en" => Preference::En,
                "ko" => Preference::Ko,
                _ => return Err(usage()),
            };
            save(preference)?;
            println!(
                "{}: {} ({}) · {}",
                crate::tr!("언어 설정 저장", "Language preference saved"),
                preference.label(),
                preference.tag(),
                crate::tr!(
                    "다음 요약 요청부터 적용",
                    "applies from the next summary request"
                )
            );
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> Box<dyn std::error::Error + Send + Sync> {
    crate::tr!(
        "사용법: maeklog language <show|set auto|en|ko>",
        "Usage: maeklog language <show|set auto|en|ko>"
    )
    .into()
}

fn show() -> AppResult<()> {
    let preference = load()?;
    let language = effective(preference);
    i18n::cache_language(language);
    println!(
        "{}: {} ({})",
        crate::tr!("언어 설정", "Language preference"),
        preference.label(),
        preference.tag()
    );
    println!(
        "{}: {} ({})",
        crate::tr!("화면·요약 언어", "Interface and summary language"),
        language.prompt_name(),
        language.tag()
    );
    println!(
        "{}: {}",
        crate::tr!("설정 파일", "Settings file"),
        path()?.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "maeklog-language-settings-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ))
            .join("language-settings.json")
    }

    #[test]
    fn auto_resolves_system_while_explicit_choice_overrides_it() {
        for system in [Language::En, Language::Ko] {
            assert_eq!(Preference::Auto.resolve(system), system);
            assert_eq!(Preference::En.resolve(system), Language::En);
            assert_eq!(Preference::Ko.resolve(system), Language::Ko);
        }
        assert_eq!(Preference::default(), Preference::Auto);
        for preference in [Preference::Auto, Preference::En, Preference::Ko] {
            assert_eq!(
                serde_json::from_str::<Preference>(&format!("\"{}\"", preference.tag())).unwrap(),
                preference
            );
        }
    }

    #[test]
    fn absent_file_defaults_without_writes_and_private_saves_leave_model_settings_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture();
        let parent = path.parent().unwrap();
        assert_eq!(load_at(&path).unwrap(), Preference::Auto);
        assert!(!parent.exists());
        config::private_dir(parent.to_path_buf()).unwrap();
        let model = parent.join("model-settings.json");
        let model_bytes = b"{\"model\":\"preserve-model\",\"effort\":\"high\"}\n";
        fs::write(&model, model_bytes).unwrap();
        for preference in [Preference::Ko, Preference::En, Preference::Auto] {
            save_at(&path, preference).unwrap();
            assert_eq!(load_at(&path).unwrap(), preference);
            assert_eq!(fs::read(&model).unwrap(), model_bytes);
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(parent).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(fs::read_dir(parent).unwrap().count(), 3);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn malformed_oversize_and_unknown_preferences_are_never_overwritten_or_exposed() {
        let path = fixture();
        save_at(&path, Preference::Ko).unwrap();
        let mut cases = vec![
            b"{invalid SECRET_NEVER_LOG".to_vec(),
            b"{\"language\":\"SECRET_NEVER_LOG\"}".to_vec(),
            b"{\"language\":\"ko\",\"unexpected\":\"SECRET_NEVER_LOG\"}".to_vec(),
            b"{\"language\":\"en\\n--config\"}".to_vec(),
        ];
        cases.push(vec![b'x'; usize::try_from(MAX_SETTINGS_BYTES + 1).unwrap()]);
        for bytes in cases {
            fs::write(&path, &bytes).unwrap();
            let error = load_at(&path).unwrap_err().to_string();
            assert!(!error.contains("SECRET_NEVER_LOG"));
            assert!(save_at(&path, Preference::En).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn symlink_preferences_and_locks_do_not_modify_their_target() {
        use std::os::unix::fs::symlink;
        for lock_is_symlink in [false, true] {
            let path = fixture();
            let parent = config::private_dir(path.parent().unwrap().to_path_buf()).unwrap();
            let target = parent.join("untouched");
            fs::write(&target, "keep").unwrap();
            let link = if lock_is_symlink {
                parent.join(".language-settings.lock")
            } else {
                path.clone()
            };
            symlink(&target, link).unwrap();
            assert!(save_at(&path, Preference::Ko).is_err());
            assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
            fs::remove_dir_all(parent).unwrap();
        }
    }

    #[test]
    fn a_busy_lock_times_out_and_preserves_saved_preference() {
        let path = fixture();
        save_at(&path, Preference::Ko).unwrap();
        let bytes = fs::read(&path).unwrap();
        let held = storage::try_lock_file(&path.with_file_name(".language-settings.lock"))
            .unwrap()
            .unwrap();
        let started = Instant::now();
        assert!(save_at(&path, Preference::En).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(held);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_readers_and_writers_only_observe_complete_preferences() {
        let path = fixture();
        save_at(&path, Preference::Auto).unwrap();
        thread::scope(|scope| {
            for preference in [Preference::Auto, Preference::Ko, Preference::En] {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..8 {
                        save_at(path, preference).unwrap();
                    }
                });
            }
            let path = &path;
            scope.spawn(move || {
                for _ in 0..12 {
                    assert!(
                        [Preference::Auto, Preference::En, Preference::Ko]
                            .contains(&load_at(path).unwrap())
                    );
                }
            });
        });
        assert!(
            [Preference::Auto, Preference::En, Preference::Ko].contains(&load_at(&path).unwrap())
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
