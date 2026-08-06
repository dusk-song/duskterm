use std::path::{Path, PathBuf};

use tokio::fs::{self, OpenOptions};

use super::DownloadCollisionPolicy;

const MAX_SAFE_FILE_NAME_CHARS: usize = 180;

pub struct DownloadTarget {
    pub display_name: String,
    pub target_path: PathBuf,
    pub temp_path: PathBuf,
    pub file: tokio::fs::File,
}

pub async fn prepare_download_target(
    directory: &Path,
    remote_name: &[u8],
    collision_policy: DownloadCollisionPolicy,
    operation_id: &str,
) -> Result<DownloadTarget, String> {
    let metadata = fs::metadata(directory)
        .await
        .map_err(|error| format!("无法访问下载目录：{error}"))?;
    if !metadata.is_dir() {
        return Err("选择的下载目标不是目录".to_string());
    }

    let display_name = sanitize_remote_file_name(remote_name);
    let target_path = resolve_collision(directory, &display_name, collision_policy).await?;
    let temp_name = format!(".{}.duskterm.part.{}", display_name, operation_id);
    let temp_path = directory.join(temp_name);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .await
        .map_err(|error| format!("无法创建下载临时文件：{error}"))?;

    Ok(DownloadTarget {
        display_name,
        target_path,
        temp_path,
        file,
    })
}

pub async fn commit_download(
    temp: PathBuf,
    target: PathBuf,
    collision_policy: DownloadCollisionPolicy,
    display_name: &str,
) -> Result<PathBuf, String> {
    if collision_policy == DownloadCollisionPolicy::AutoRename {
        return commit_download_without_overwrite(temp, target, display_name).await;
    }

    #[cfg(unix)]
    {
        fs::rename(&temp, &target)
            .await
            .map_err(|error| format!("提交下载文件失败：{error}"))?;
    }

    #[cfg(windows)]
    {
        let target_for_commit = target.clone();
        tokio::task::spawn_blocking(move || atomic_replace_windows(&temp, &target_for_commit))
            .await
            .map_err(|error| format!("提交下载文件任务失败：{error}"))??;
    }

    #[cfg(not(any(unix, windows)))]
    {
        if fs::try_exists(&target).await.unwrap_or(false) {
            return Err("当前平台不支持安全覆盖已有文件".to_string());
        }
        fs::rename(&temp, &target)
            .await
            .map_err(|error| format!("提交下载文件失败：{error}"))?;
    }

    Ok(target)
}

pub async fn cleanup_temp_file(path: &Path) {
    let _ = fs::remove_file(path).await;
}

async fn resolve_collision(
    directory: &Path,
    file_name: &str,
    collision_policy: DownloadCollisionPolicy,
) -> Result<PathBuf, String> {
    let initial = directory.join(file_name);
    if collision_policy == DownloadCollisionPolicy::Overwrite
        || !fs::try_exists(&initial)
            .await
            .map_err(|error| format!("检查下载目标失败：{error}"))?
    {
        return Ok(initial);
    }

    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());

    for index in 1..=9999 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(candidate_name);
        if !fs::try_exists(&candidate)
            .await
            .map_err(|error| format!("检查下载目标失败：{error}"))?
        {
            return Ok(candidate);
        }
    }

    Err("无法为下载文件生成不冲突的名称".to_string())
}

async fn commit_download_without_overwrite(
    temp: PathBuf,
    initial_target: PathBuf,
    display_name: &str,
) -> Result<PathBuf, String> {
    let directory = initial_target
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "下载目标缺少父目录".to_string())?;
    let mut target = initial_target;

    for _ in 0..=9999 {
        match fs::hard_link(&temp, &target).await {
            Ok(()) => {
                // The final name already points at the fully synced inode. A
                // failed cleanup must not turn a successful download into a
                // false failure or remove the committed target.
                cleanup_temp_file(&temp).await;
                return Ok(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                target = resolve_collision(
                    &directory,
                    display_name,
                    DownloadCollisionPolicy::AutoRename,
                )
                .await?;
            }
            Err(error) => {
                return Err(format!("以不覆盖方式提交下载文件失败：{error}"));
            }
        }
    }

    Err("无法为下载文件生成不冲突的名称".to_string())
}

fn sanitize_remote_file_name(remote_name: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(remote_name);
    let basename = decoded
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();

    let mut sanitized = String::with_capacity(basename.len());
    let mut utf16_units = 0usize;
    for character in basename.chars() {
        let character_units = character.len_utf16();
        if utf16_units + character_units > MAX_SAFE_FILE_NAME_CHARS {
            break;
        }
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            sanitized.push('_');
        } else {
            sanitized.push(character);
        }
        utf16_units += character_units;
    }

    let trimmed = sanitized.trim_matches([' ', '.']);
    let mut result = if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        "download".to_string()
    } else {
        trimmed.to_string()
    };

    let stem = Path::new(&result)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&result)
        .to_ascii_uppercase();
    if is_windows_reserved_name(&stem) {
        result.insert(0, '_');
    }
    result
}

fn is_windows_reserved_name(stem: &str) -> bool {
    matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

#[cfg(windows)]
fn atomic_replace_windows(temp: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    if !target.exists() {
        return std::fs::rename(temp, target).map_err(|error| format!("提交下载文件失败：{error}"));
    }

    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let temp_wide: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        ReplaceFileW(
            PCWSTR(target_wide.as_ptr()),
            PCWSTR(temp_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
        .map_err(|error| format!("提交下载文件失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{is_windows_reserved_name, sanitize_remote_file_name};

    #[test]
    fn sanitizes_remote_paths_and_windows_names_consistently() {
        assert_eq!(
            sanitize_remote_file_name(b"../../report?.zip"),
            "report_.zip"
        );
        assert_eq!(sanitize_remote_file_name(b"folder\\CON.txt"), "_CON.txt");
        assert_eq!(sanitize_remote_file_name(b".."), "download");
        assert!(is_windows_reserved_name("COM1"));
        assert!(!is_windows_reserved_name("COM10"));
        assert!(
            sanitize_remote_file_name("😀".repeat(200).as_bytes())
                .encode_utf16()
                .count()
                <= 180
        );
    }
}
