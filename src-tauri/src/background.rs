use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageReader;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_EDGE: u32 = 7680;
const MAX_SOURCE_PIXELS: u64 = 100_000_000;
const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const JPEG_QUALITY: u8 = 88;

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
        .ok_or_else(|| "Failed to locate application data directory".to_string())
}

fn validate_resource_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err("Invalid background resource id".to_string());
    }
    Ok(())
}

fn validate_source(path: &Path) -> Result<(u32, u32), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Background image does not exist".to_string());
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err("Background image file cannot exceed 128 MB".to_string());
    }
    let dimensions = ImageReader::open(path)
        .map_err(|error| format!("Failed to open background image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("Failed to detect background image format: {error}"))?
        .into_dimensions()
        .map_err(|error| format!("Failed to read background image dimensions: {error}"))?;
    let pixels = dimensions.0 as u64 * dimensions.1 as u64;
    if dimensions.0 == 0 || dimensions.1 == 0 || pixels > MAX_SOURCE_PIXELS {
        return Err("Background image pixel count is too large".to_string());
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

fn should_reuse_original(width: u32, height: u32, target_width: u32, target_height: u32) -> bool {
    let limit_width = target_width.clamp(1, MAX_EDGE);
    let limit_height = target_height.clamp(1, MAX_EDGE);
    width <= limit_width && height <= limit_height
}

fn encode_cache(
    source: &Path,
    target: &Path,
    target_width: u32,
    target_height: u32,
) -> Result<(u32, u32), String> {
    let (source_width, source_height) = validate_source(source)?;
    let image = ImageReader::open(source)
        .map_err(|error| format!("Failed to open background image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("Failed to detect background image format: {error}"))?
        .decode()
        .map_err(|error| format!("Failed to decode background image: {error}"))?;
    let (width, height) =
        target_dimensions(source_width, source_height, target_width, target_height);
    let output = if width != source_width || height != source_height {
        image.resize_exact(width, height, FilterType::Triangle)
    } else {
        image
    };
    let mut file = File::create(target)
        .map_err(|error| format!("Failed to create background cache: {error}"))?;
    let mut encoder = JpegEncoder::new_with_quality(&mut file, JPEG_QUALITY);
    let rgb = output.to_rgb8();
    encoder
        .encode_image(&rgb)
        .map_err(|error| format!("Failed to encode background cache: {error}"))?;
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
        .ok_or_else(|| "Background original image does not exist".to_string())
}

fn asset_from_directory(
    resource_id: &str,
    directory: &Path,
    target_width: u32,
    target_height: u32,
) -> Result<BackgroundAsset, String> {
    let original = original_in(directory)?;
    let source_dimensions = validate_source(&original)?;
    let use_original = should_reuse_original(
        source_dimensions.0,
        source_dimensions.1,
        target_width,
        target_height,
    );
    let optimized = if use_original {
        original.clone()
    } else {
        directory.join("optimized.jpg")
    };
    let desired = target_dimensions(
        source_dimensions.0,
        source_dimensions.1,
        target_width,
        target_height,
    );
    if !use_original && optimized.is_file() {
        match image::image_dimensions(&optimized) {
            Ok((width, height)) if width >= desired.0 && height >= desired.1 => {}
            _ => {
                let _ = fs::remove_file(&optimized);
            }
        }
    }
    let (width, height) = if use_original {
        source_dimensions
    } else if optimized.is_file() {
        image::image_dimensions(&optimized)
            .map_err(|error| format!("Background cache is corrupted: {error}"))?
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
            return Err("Only PNG, JPG, JPEG and WebP static images are supported".to_string());
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
            let source_dimensions = validate_source(&original)?;
            let use_original = should_reuse_original(
                source_dimensions.0,
                source_dimensions.1,
                target_width,
                target_height,
            );
            let (width, height, optimized_path) = if use_original {
                (
                    source_dimensions.0,
                    source_dimensions.1,
                    destination
                        .join(format!("original.{extension}"))
                        .to_string_lossy()
                        .into_owned(),
                )
            } else {
                let optimized = temporary.join("optimized.jpg");
                let (width, height) =
                    encode_cache(&original, &optimized, target_width, target_height)?;
                (
                    width,
                    height,
                    destination
                        .join("optimized.jpg")
                        .to_string_lossy()
                        .into_owned(),
                )
            };
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
                optimized_path,
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
    use super::{should_reuse_original, target_dimensions, validate_source};
    use image::{DynamicImage, ImageFormat};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn does_not_upscale_small_images() {
        assert_eq!(target_dimensions(1200, 800, 3840, 2160), (1200, 800));
    }

    #[test]
    fn covers_target_without_losing_aspect_ratio() {
        assert_eq!(target_dimensions(8000, 4000, 3840, 2160), (4320, 2160));
    }

    #[test]
    fn reuses_reasonable_sized_images_for_fast_import() {
        assert!(should_reuse_original(3200, 1800, 3840, 2160));
    }

    #[test]
    fn does_not_reuse_oversized_images() {
        assert!(!should_reuse_original(8000, 4000, 3840, 2160));
    }

    #[test]
    fn detects_dimensions_from_content_when_extension_is_inaccurate() {
        let path =
            std::env::temp_dir().join(format!("duskterm-background-format-{}.jpg", Uuid::new_v4()));
        let mut file = fs::File::create(&path).expect("create temporary image");
        DynamicImage::new_rgba8(7, 5)
            .write_to(&mut file, ImageFormat::Png)
            .expect("write PNG content");

        assert_eq!(validate_source(&path).expect("read dimensions"), (7, 5));
        let _ = fs::remove_file(path);
    }
}
