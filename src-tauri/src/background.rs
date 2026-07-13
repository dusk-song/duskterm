use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_EDGE: u32 = 7680;
const MAX_SOURCE_PIXELS: u64 = 100_000_000;
const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackgroundAsset {
    pub resource_id: String,
    pub file_name: String,
    pub original_path: String,
    pub optimized_path: String,
    pub width: u32,
    pub height: u32,
}

fn root_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .map(|dir| dir.join("duskterm").join("backgrounds"))
        .ok_or_else(|| "无法定位应用数据目录".to_string())
}

fn validate_resource_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err("无效的背景资源标识".to_string());
    }
    Ok(())
}

fn validate_source(path: &Path) -> Result<(u32, u32), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("背景图片不存在".to_string());
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err("背景图片文件不能超过 128 MB".to_string());
    }
    let dimensions =
        image::image_dimensions(path).map_err(|error| format!("无法读取背景图片尺寸: {error}"))?;
    let pixels = dimensions.0 as u64 * dimensions.1 as u64;
    if dimensions.0 == 0 || dimensions.1 == 0 || pixels > MAX_SOURCE_PIXELS {
        return Err("背景图片像素数量过大（上限 1 亿像素）".to_string());
    }
    Ok(dimensions)
}

fn target_dimensions(width: u32, height: u32, target_width: u32, target_height: u32) -> (u32, u32) {
    let limit_width = target_width.clamp(1, MAX_EDGE);
    let limit_height = target_height.clamp(1, MAX_EDGE);
    let scale = (limit_width as f64 / width as f64)
        .max(limit_height as f64 / height as f64)
        .min(MAX_EDGE as f64 / width as f64)
        .min(MAX_EDGE as f64 / height as f64)
        .min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn encode_cache(
    source: &Path,
    target: &Path,
    target_width: u32,
    target_height: u32,
) -> Result<(u32, u32), String> {
    let (source_width, source_height) = validate_source(source)?;
    let image = image::open(source).map_err(|error| format!("无法解码背景图片: {error}"))?;
    let (width, height) =
        target_dimensions(source_width, source_height, target_width, target_height);
    let output = if width != source_width || height != source_height {
        image.resize_exact(width, height, FilterType::Lanczos3)
    } else {
        image
    };
    let encoder = webp::Encoder::from_image(&output)
        .map_err(|error| format!("创建 WebP 编码器失败: {error}"))?;
    let encoded = encoder.encode(90.0);
    fs::write(target, &*encoded).map_err(|error| format!("生成背景缓存失败: {error}"))?;
    Ok((width, height))
}

fn original_in(directory: &Path) -> Result<PathBuf, String> {
    fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file() && path.file_stem().and_then(|value| value.to_str()) == Some("original")
        })
        .ok_or_else(|| "背景原图不存在".to_string())
}

fn asset_from_directory(
    resource_id: &str,
    directory: &Path,
    target_width: u32,
    target_height: u32,
) -> Result<BackgroundAsset, String> {
    let original = original_in(directory)?;
    let optimized = directory.join("optimized.webp");
    let source_dimensions = validate_source(&original)?;
    let desired = target_dimensions(
        source_dimensions.0,
        source_dimensions.1,
        target_width,
        target_height,
    );
    if optimized.is_file() {
        match image::image_dimensions(&optimized) {
            Ok((width, height)) if width >= desired.0 && height >= desired.1 => {}
            _ => {
                let _ = fs::remove_file(&optimized);
            }
        }
    }
    let (width, height) = if optimized.is_file() {
        image::image_dimensions(&optimized).map_err(|error| format!("背景缓存损坏: {error}"))?
    } else {
        encode_cache(&original, &optimized, target_width, target_height)?
    };
    let file_name = fs::read(directory.join("metadata.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BackgroundAsset>(&bytes).ok())
        .map(|asset| asset.file_name)
        .unwrap_or_else(|| {
            original
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("background")
                .to_string()
        });
    Ok(BackgroundAsset {
        resource_id: resource_id.to_string(),
        file_name,
        original_path: original.to_string_lossy().into_owned(),
        optimized_path: optimized.to_string_lossy().into_owned(),
        width,
        height,
    })
}

#[tauri::command]
pub async fn import_background_image(
    source_path: String,
    target_width: u32,
    target_height: u32,
) -> Result<BackgroundAsset, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        validate_source(&source)?;
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            return Err("仅支持 PNG、JPG 和 WebP 静态图片".to_string());
        }
        let resource_id = Uuid::new_v4().to_string();
        let root = root_dir()?;
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let temporary = root.join(format!(".tmp-{resource_id}"));
        let destination = root.join(&resource_id);
        let result = (|| {
            fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
            let original = temporary.join(format!("original.{extension}"));
            fs::copy(&source, &original).map_err(|error| error.to_string())?;
            let optimized = temporary.join("optimized.webp");
            let (width, height) = encode_cache(&original, &optimized, target_width, target_height)?;
            let asset = BackgroundAsset {
                resource_id: resource_id.clone(),
                file_name: source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("background")
                    .to_string(),
                original_path: destination
                    .join(format!("original.{extension}"))
                    .to_string_lossy()
                    .into_owned(),
                optimized_path: destination
                    .join("optimized.webp")
                    .to_string_lossy()
                    .into_owned(),
                width,
                height,
            };
            fs::write(
                temporary.join("metadata.json"),
                serde_json::to_vec_pretty(&asset).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
            Ok(asset)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn ensure_background_image(
    resource_id: String,
    target_width: u32,
    target_height: u32,
) -> Result<BackgroundAsset, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_resource_id(&resource_id)?;
        asset_from_directory(
            &resource_id,
            &root_dir()?.join(&resource_id),
            target_width,
            target_height,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn delete_background_image(resource_id: String) -> Result<(), String> {
    validate_resource_id(&resource_id)?;
    let directory = root_dir()?.join(resource_id);
    if directory.exists() {
        fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::target_dimensions;
    #[test]
    fn does_not_upscale_small_images() {
        assert_eq!(target_dimensions(1200, 800, 3840, 2160), (1200, 800));
    }
    #[test]
    fn covers_target_without_losing_aspect_ratio() {
        assert_eq!(target_dimensions(8000, 4000, 3840, 2160), (4320, 2160));
    }
}
