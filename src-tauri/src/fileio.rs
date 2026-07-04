use base64::Engine;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[tauri::command]
pub fn save_text_file(path: String, content: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("路径包含非法字符".to_string());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&target, content).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct ImportedDesktopPetAsset {
    pub stored_path: String,
    pub file_name: String,
    /// Base64-encoded data URL for immediate display (avoids asset protocol issues)
    pub data_url: String,
}

fn desktop_pet_asset_dir() -> Result<PathBuf, String> {
    let base_dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| "无法定位本地应用数据目录".to_string())?;
    Ok(base_dir.join("duskterm").join("desktop-pet-assets"))
}

fn sanitize_action_key(action_key: &str) -> String {
    let cleaned: String = action_key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    if cleaned.is_empty() {
        "action".to_string()
    } else {
        cleaned
    }
}

#[tauri::command]
pub fn import_desktop_pet_asset(
    source_path: String,
    action_key: String,
) -> Result<ImportedDesktopPetAsset, String> {
    let source = PathBuf::from(&source_path);
    if !source.exists() {
        return Err("源文件不存在".to_string());
    }
    if !source.is_file() {
        return Err("源路径不是文件".to_string());
    }

    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.trim().to_lowercase())
        .filter(|ext| !ext.is_empty())
        .ok_or_else(|| "无法识别资源文件扩展名".to_string())?;

    let asset_dir = desktop_pet_asset_dir()?;
    fs::create_dir_all(&asset_dir).map_err(|e| e.to_string())?;

    let file_name = format!(
        "{}-{}.{}",
        sanitize_action_key(&action_key),
        uuid::Uuid::new_v4(),
        extension
    );
    let target = asset_dir.join(&file_name);
    fs::copy(&source, &target).map_err(|e| e.to_string())?;

    // Read copied file as base64 data URL for reliable frontend display
    let bytes = fs::read(&target).map_err(|e| format!("读取资源失败: {}", e))?;
    let mime = mime_for_extension(&extension);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);

    Ok(ImportedDesktopPetAsset {
        stored_path: path_to_string(&target)?,
        file_name,
        data_url,
    })
}

fn mime_for_extension(ext: &str) -> &str {
    match ext {
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn path_to_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| "资源路径包含无法处理的字符".to_string())
}
