use crate::AppResult;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct HookRuntimeConfig {
    pub state_dir: PathBuf,
    pub plugin_root: PathBuf,
}

pub fn state_root() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("MAEKLOG_STATE_DIR") {
        return private_dir(PathBuf::from(path));
    }
    // An installed runtime can explicitly retain an existing record store.
    // Herdr's plugin-default state directory must not redirect UI/worker calls
    // away from that store. The explicit app override above still wins.
    if let Some(runtime) = hook_runtime_config()? {
        return private_dir(runtime.state_dir);
    }
    if let Some(path) = env::var_os("HERDR_PLUGIN_STATE_DIR") {
        return private_dir(PathBuf::from(path));
    }

    let base = env::var_os("XDG_STATE_HOME").map_or_else(
        || home_dir().map(|home| home.join(".local/state")),
        |path| Ok(PathBuf::from(path)),
    )?;
    private_dir(base.join("maeklog"))
}

pub fn plugin_root() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("MAEKLOG_PLUGIN_ROOT") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("HERDR_PLUGIN_ROOT") {
        return Ok(PathBuf::from(path));
    }
    if let Some(runtime) = hook_runtime_config()? {
        return Ok(runtime.plugin_root);
    }
    Ok(env::current_dir()?)
}

pub fn plugin_config_dir() -> AppResult<PathBuf> {
    env::var_os("HERDR_PLUGIN_CONFIG_DIR").map_or_else(
        || {
            let base = env::var_os("XDG_CONFIG_HOME").map_or_else(
                || home_dir().map(|home| home.join(".config")),
                |path| Ok(PathBuf::from(path)),
            )?;
            Ok(base.join("herdr/plugins/config/maeklog"))
        },
        |path| Ok(PathBuf::from(path)),
    )
}

/// App preferences follow the UI's explicit config directory, or the installed
/// namespace-owned hook's directory. Other hook/install/state paths are unchanged.
pub fn model_settings_config_dir() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe()?;
    let runtime = if settings_hook_executable(&executable) {
        read_settings_runtime(&executable)?
    } else {
        None
    };
    settings_config_directory(None, &executable, runtime.as_ref())?
        .map_or_else(plugin_config_dir, Ok)
}

fn settings_hook_executable(executable: &Path) -> bool {
    executable
        .file_name()
        .is_some_and(|name| name == "maeklog-hook")
        && executable
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "bin")
}

fn settings_config_directory(
    explicit: Option<&Path>,
    executable: &Path,
    runtime: Option<&HookRuntimeConfig>,
) -> AppResult<Option<PathBuf>> {
    if let Some(path) = explicit {
        return Ok(Some(path.to_path_buf()));
    }
    if !settings_hook_executable(executable) {
        return Ok(None);
    }
    let Some(runtime) = runtime else {
        return Ok(None);
    };
    if !runtime.state_dir.is_absolute() || !runtime.plugin_root.is_absolute() {
        return Err(invalid_settings_runtime());
    }
    Ok(executable
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf))
}

fn invalid_settings_runtime() -> Box<dyn std::error::Error + Send + Sync> {
    crate::tr!(
        "설치된 Maeklog 실행 설정이 유효하지 않습니다 · 설정 경로를 선택하지 않았습니다",
        "Installed Maeklog runtime is invalid; no configured path was selected"
    )
    .into()
}

fn read_settings_runtime(executable: &Path) -> AppResult<Option<HookRuntimeConfig>> {
    let Some(directory) = executable.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    read_installed_runtime(&directory.join("hook-runtime.json"))
}

fn read_installed_runtime(path: &Path) -> AppResult<Option<HookRuntimeConfig>> {
    use std::io::Read;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() || metadata.len() > 16 * 1024 {
        return Err(invalid_settings_runtime());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        return Err(invalid_settings_runtime());
    }
    let runtime: HookRuntimeConfig =
        serde_json::from_slice(&bytes).map_err(|_| invalid_settings_runtime())?;
    if !runtime.state_dir.is_absolute()
        || !runtime.plugin_root.is_absolute()
        || [&runtime.state_dir, &runtime.plugin_root]
            .iter()
            .any(|path| {
                path.components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            })
    {
        return Err(invalid_settings_runtime());
    }
    Ok(Some(runtime))
}

pub fn codex_home() -> AppResult<PathBuf> {
    Ok(env::var_os("CODEX_HOME").map_or(home_dir()?.join(".codex"), PathBuf::from))
}

pub fn herdr_session_name() -> String {
    env::var("HERDR_SESSION").unwrap_or_else(|_| "default".to_string())
}

pub fn codex_bin() -> PathBuf {
    env::var_os("MAEKLOG_CODEX_BIN").map_or_else(|| PathBuf::from("codex"), PathBuf::from)
}

pub fn herdr_bin() -> PathBuf {
    env::var_os("HERDR_BIN_PATH").map_or_else(|| PathBuf::from("herdr"), PathBuf::from)
}

pub fn private_dir(path: PathBuf) -> AppResult<PathBuf> {
    fs::create_dir_all(&path)?;
    set_directory_mode(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn set_directory_mode(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_mode(_path: &Path) -> AppResult<()> {
    Ok(())
}

fn home_dir() -> AppResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".into())
}

fn hook_runtime_config() -> AppResult<Option<HookRuntimeConfig>> {
    let executable = env::current_exe()?;
    let explicit = env::var_os("HERDR_PLUGIN_CONFIG_DIR");
    let directory = installed_runtime_directory(explicit.as_deref().map(Path::new), &executable)
        .map_or_else(plugin_config_dir, Ok)?;
    read_installed_runtime(&directory.join("hook-runtime.json"))
}

fn installed_runtime_directory(explicit: Option<&Path>, executable: &Path) -> Option<PathBuf> {
    if let Some(directory) = explicit {
        return Some(directory.to_path_buf());
    }
    if settings_hook_executable(executable)
        && let Some(directory) = executable.parent().and_then(Path::parent)
    {
        return Some(directory.to_path_buf());
    }
    None
}

#[cfg(test)]
mod settings_directory_tests {
    use super::*;

    fn runtime() -> HookRuntimeConfig {
        HookRuntimeConfig {
            state_dir: "/private/state".into(),
            plugin_root: "/private/plugin".into(),
        }
    }

    #[test]
    fn explicit_config_wins_and_owned_installed_hook_uses_its_parent() {
        let executable = Path::new("/custom/config/bin/maeklog-hook");
        let runtime = runtime();
        assert_eq!(
            settings_config_directory(Some(Path::new("/explicit")), executable, Some(&runtime))
                .unwrap(),
            Some("/explicit".into())
        );
        assert_eq!(
            settings_config_directory(None, executable, Some(&runtime)).unwrap(),
            Some("/custom/config".into())
        );
        assert_eq!(
            settings_config_directory(None, executable, None).unwrap(),
            None
        );
    }

    #[test]
    fn normal_cli_and_nonowned_or_deleted_helper_do_not_infer_a_config_directory() {
        let runtime = runtime();
        for path in [
            "/repo/target/release/maeklog",
            "/custom/bin/unrelated-hook",
            "/custom/lib/maeklog-hook",
            "/custom/bin/maeklog-hook (deleted)",
        ] {
            assert_eq!(
                settings_config_directory(None, Path::new(path), Some(&runtime)).unwrap(),
                None
            );
        }
    }

    #[test]
    fn installed_runtime_directory_preserves_explicit_and_owned_scope() {
        // Owned/explicit resolution never needs HOME or a default directory.
        let hook = Path::new("/custom/config/bin/maeklog-hook");
        assert_eq!(
            installed_runtime_directory(Some(Path::new("/explicit")), hook),
            Some(PathBuf::from("/explicit"))
        );
        assert_eq!(
            installed_runtime_directory(None, hook),
            Some(PathBuf::from("/custom/config"))
        );
        for executable in [
            "/repo/target/release/maeklog",
            "/unrelated/bin/other-hook",
            "/custom/lib/maeklog-hook",
        ] {
            assert_eq!(
                installed_runtime_directory(None, Path::new(executable)),
                None
            );
        }
    }

    #[test]
    fn installed_runtime_requires_bounded_regular_file_and_absolute_paths() {
        let directory =
            env::temp_dir().join(format!("maeklog-runtime-validation-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("hook-runtime.json");
        fs::write(
            &file,
            br#"{"state_dir":"relative","plugin_root":"/plugin"}"#,
        )
        .unwrap();
        assert!(read_installed_runtime(&file).is_err());
        fs::write(
            &file,
            br#"{"state_dir":"/state/../other","plugin_root":"/plugin"}"#,
        )
        .unwrap();
        assert!(read_installed_runtime(&file).is_err());
        fs::write(&file, br#"{"state_dir":"/state","plugin_root":"/plugin"}"#).unwrap();
        assert_eq!(
            read_installed_runtime(&file).unwrap().unwrap().state_dir,
            Path::new("/state")
        );
        fs::write(&file, vec![b' '; 16 * 1024 + 1]).unwrap();
        assert!(read_installed_runtime(&file).is_err());
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(read_installed_runtime(&file).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn invalid_owned_runtime_fails_clearly_but_explicit_override_stays_authoritative() {
        let invalid = HookRuntimeConfig {
            state_dir: PathBuf::new(),
            plugin_root: "relative".into(),
        };
        let executable = Path::new("/custom/config/bin/maeklog-hook");
        assert!(settings_config_directory(None, executable, Some(&invalid)).is_err());
        assert_eq!(
            settings_config_directory(Some(Path::new("/explicit")), executable, Some(&invalid))
                .unwrap(),
            Some("/explicit".into())
        );
    }
}
