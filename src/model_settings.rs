//! Maeklog-wide execution settings, independent of Codex's user configuration.
use crate::i18n::Language;
use crate::{AppResult, config, language_settings, model_catalog, storage};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SETTINGS_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub model: String,
    /// None means the discovered model has no reasoning-effort override.
    pub effort: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: "gpt-6-luna".into(),
            effort: Some("medium".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfile {
    pub settings: Settings,
    pub language: Language,
}

impl ExecutionProfile {
    pub fn current() -> AppResult<Self> {
        Ok(Self {
            settings: load()?,
            language: language_settings::fresh_effective()?,
        })
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        if !valid_token(&self.model, 128)
            || self
                .effort
                .as_deref()
                .is_some_and(|effort| !valid_token(effort, 32))
        {
            return Err(crate::tr!(
                "모델 또는 추론 강도 값이 유효하지 않습니다 · 설정은 변경되지 않았습니다",
                "Invalid model or reasoning effort; settings were not changed"
            )
            .into());
        }
        Ok(())
    }

    /// Uses structured arguments; validated strings cannot introduce CLI/TOML syntax.
    pub fn configure(&self, command: &mut Command) -> AppResult<()> {
        self.validate()?;
        command.arg("--model").arg(&self.model);
        if let Some(effort) = &self.effort {
            command
                .arg("--config")
                .arg(format!("model_reasoning_effort=\"{effort}\""));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.effort.as_ref().map_or_else(
            || {
                format!(
                    "{} · {}",
                    self.model,
                    crate::tr!("강도 지정 없음", "no effort override")
                )
            },
            |effort| format!("{} · {effort}", self.model),
        )
    }
}

pub(crate) fn valid_token(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'/'))
}

pub fn path() -> AppResult<PathBuf> {
    Ok(config::model_settings_config_dir()?.join("model-settings.json"))
}

pub fn load() -> AppResult<Settings> {
    let path = path()?;
    // An absent file is the original default and causes no settings-directory mutation.
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        _ => load_at(&path),
    }
}

pub fn save(settings: &Settings, catalog: &model_catalog::Catalog) -> AppResult<()> {
    validate_selection(settings, catalog)?;
    save_at(&path()?, settings)
}

fn validate_selection(settings: &Settings, catalog: &model_catalog::Catalog) -> AppResult<()> {
    settings.validate()?;
    let model = catalog.find(&settings.model).ok_or_else(|| {
        crate::tr!(
            "선택한 모델이 현재 Codex 모델 목록에 없습니다 · 설정은 변경되지 않았습니다",
            "Selected model is absent from the current Codex catalog; settings were not changed"
        )
        .to_string()
    })?;
    let supported = match &settings.effort {
        Some(effort) => model
            .supported_reasoning_efforts
            .iter()
            .any(|option| &option.reasoning_effort == effort),
        None => model.supported_reasoning_efforts.is_empty(),
    };
    if !supported {
        return Err(crate::tr!(
            "선택한 모델은 이 추론 강도를 지원하지 않습니다 · 설정은 변경되지 않았습니다",
            "Selected model does not support this reasoning effort; settings were not changed"
        )
        .into());
    }
    Ok(())
}

fn ensure_regular_or_missing(path: &Path) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => Err(crate::tr!(
            "모델 설정 경로는 일반 파일이어야 합니다",
            "Model settings path must be a regular file"
        )
        .into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn lock(path: &Path) -> AppResult<File> {
    let parent = path.parent().ok_or("model settings path has no parent")?;
    config::private_dir(parent.to_path_buf())?;
    let lock_path = parent.join(".model-settings.lock");
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
                "모델 설정이 사용 중입니다 · 잠시 후 다시 시도하세요",
                "Model settings are busy; retry shortly"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn load_at(path: &Path) -> AppResult<Settings> {
    let _lock = lock(path)?;
    ensure_regular_or_missing(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(crate::tr!(
            "모델 설정 파일이 너무 큽니다",
            "Model settings file is too large"
        )
        .into());
    }
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|_| {
        crate::tr!(
            "모델 설정 파일을 읽을 수 없습니다 · 기본 모델로 바꾸지 않았습니다",
            "Model settings file is malformed; no default model was substituted"
        )
    })?;
    settings.validate()?;
    Ok(settings)
}

fn save_at(path: &Path, settings: &Settings) -> AppResult<()> {
    settings.validate()?;
    let _lock = lock(path)?;
    ensure_regular_or_missing(path)?;
    storage::atomic_write_json(path, settings)
}

pub fn cli(arguments: &[String]) -> AppResult<()> {
    match arguments {
        [action] if action == "show" => show(),
        [action] if action == "list" => list(),
        [action, model] if action == "set" => set(model, None),
        [action, model, flag, effort] if action == "set" && flag == "--effort" => {
            set(model, Some(effort))
        }
        _ => Err(crate::tr!(
            "사용법: maeklog model <show|list|set MODEL [--effort EFFORT]>",
            "Usage: maeklog model <show|list|set MODEL [--effort EFFORT]>"
        )
        .into()),
    }
}

fn show() -> AppResult<()> {
    let profile = ExecutionProfile::current()?;
    let settings = profile.settings;
    println!(
        "{}: {}",
        crate::tr!("요약 모델", "Summary model"),
        settings.model
    );
    println!(
        "{}: {}",
        crate::tr!("추론 강도", "Reasoning effort"),
        settings
            .effort
            .as_deref()
            .unwrap_or(crate::tr!("지정 없음", "no override"))
    );
    println!(
        "{}: Standard (service_tier=default, fast_mode=false)",
        crate::tr!("서비스", "Service")
    );
    println!(
        "{}: {} ({})",
        crate::tr!("화면·요약 언어", "Interface and summary language"),
        profile.language.prompt_name(),
        profile.language.tag()
    );
    println!(
        "{}: {}",
        crate::tr!("설정 파일", "Settings file"),
        path()?.display()
    );
    Ok(())
}

fn list() -> AppResult<()> {
    let catalog = model_catalog::discover()?;
    println!(
        "{}",
        crate::tr!(
            "Codex 모델 목록 · 계정의 실제 사용 권한을 보장하지 않습니다",
            "Codex model catalog; listed models do not guarantee account access"
        )
    );
    if catalog.models.is_empty() {
        println!(
            "{}",
            crate::tr!(
                "표시 가능한 텍스트 모델이 없습니다 · 현재 설정은 유지됩니다",
                "No visible text models; current settings are retained"
            )
        );
    }
    for model in &catalog.models {
        let efforts = model
            .supported_reasoning_efforts
            .iter()
            .map(|e| e.reasoning_effort.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("{} · {}", model.model, model.display_name);
        println!(
            "  {}: {} · {}: {}",
            crate::tr!("강도", "efforts"),
            if efforts.is_empty() {
                crate::tr!("지정 없음", "no override")
            } else {
                &efforts
            },
            crate::tr!("기본 강도", "default effort"),
            model
                .default_reasoning_effort
                .as_deref()
                .unwrap_or(crate::tr!("지정 없음", "no override"))
        );
        if model.is_default {
            println!(
                "  {}",
                crate::tr!("Codex 목록의 기본 모델", "Codex catalog default")
            );
        }
    }
    Ok(())
}

fn set(model: &str, effort: Option<&String>) -> AppResult<()> {
    Settings {
        model: model.to_owned(),
        effort: effort.cloned(),
    }
    .validate()?;
    let catalog = model_catalog::discover()?;
    let entry = catalog.find(model).ok_or_else(|| {
        crate::tr!(
            "선택한 모델이 현재 Codex 모델 목록에 없습니다",
            "Selected model is absent from the current Codex catalog"
        )
        .to_string()
    })?;
    let settings = Settings {
        model: entry.model.clone(),
        effort: effort
            .cloned()
            .or_else(|| entry.default_reasoning_effort.clone()),
    };
    save(&settings, &catalog)?;
    println!(
        "{}: {} · Standard",
        crate::tr!("요약 모델 저장", "Summary model saved"),
        settings.label()
    );
    println!(
        "{}",
        crate::tr!(
            "이후 일일 요약 호출과 새 진행 요약 요청에 적용됩니다 · 저장만으로 모델을 호출하지 않습니다",
            "Applies to future daily invocations and new live-summary requests; saving makes no model call"
        )
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
                "maeklog-model-settings-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ))
            .join("model-settings.json")
    }

    #[test]
    fn absent_file_keeps_previous_default_and_private_atomic_round_trip() {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture();
        assert_eq!(load_at(&path).unwrap(), Settings::default());
        assert!(!path.exists());
        let settings = Settings {
            model: "catalog-model".into(),
            effort: Some("high".into()),
        };
        save_at(&path, &settings).unwrap();
        assert_eq!(load_at(&path).unwrap(), settings);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn malformed_settings_and_injection_are_errors_without_replacement() {
        let path = fixture();
        let settings = Settings::default();
        save_at(&path, &settings).unwrap();
        for token in [
            "-bad",
            "model\n--config",
            "a\"b",
            "x\\y",
            "",
            "x y",
            "a\u{1b}b",
        ] {
            assert!(
                Settings {
                    model: token.into(),
                    effort: Some("medium".into())
                }
                .validate()
                .is_err()
            );
            assert!(
                Settings {
                    model: "safe".into(),
                    effort: Some(token.into())
                }
                .validate()
                .is_err()
            );
        }
        fs::write(
            &path,
            b"{\"model\":\"safe\",\"effort\":\"high\",\"service_tier\":\"fast\"}",
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(load_at(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn symlink_settings_are_not_followed() {
        use std::os::unix::fs::symlink;
        let path = fixture();
        config::private_dir(path.parent().unwrap().to_path_buf()).unwrap();
        let target = path.with_file_name("global-config");
        fs::write(&target, "keep").unwrap();
        symlink(&target, &path).unwrap();
        assert!(save_at(&path, &Settings::default()).is_err());
        assert!(load_at(&path).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn selections_follow_only_discovered_efforts_and_nonreasoning_omits_override() {
        let catalog = model_catalog::Catalog {
            models: vec![model_catalog::Model {
                id: "catalog-id".into(),
                model: "catalog-model".into(),
                display_name: "Catalog model".into(),
                supported_reasoning_efforts: vec![],
                default_reasoning_effort: None,
                is_default: false,
            }],
        };
        let settings = Settings {
            model: "catalog-model".into(),
            effort: None,
        };
        assert!(validate_selection(&settings, &catalog).is_ok());
        assert!(
            validate_selection(
                &Settings {
                    effort: Some("medium".into()),
                    ..settings.clone()
                },
                &catalog
            )
            .is_err()
        );
        let mut command = Command::new("unused");
        settings.configure(&mut command).unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec!["--model", "catalog-model"]
        );
        assert!(validate_selection(&Settings::default(), &catalog).is_err());
    }
}
