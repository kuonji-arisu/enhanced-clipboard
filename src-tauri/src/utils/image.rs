/// 图片处理工具：文件写入、预览图生成、BLAKE3 内容哈希。
use std::io::{BufWriter, Write};
use std::path::Path;

use arboard::ImageData as ClipboardImage;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageEncoder, RgbaImage};

/// 缩略图最大宽度（像素）
pub(crate) const THUMB_MAX_W: u32 = 600;

/// 缩略图最大高度（像素）
pub(crate) const THUMB_MAX_H: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewAssetFormat {
    Png,
    Jpeg,
}

impl PreviewAssetFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// 将 RGBA 原始字节以 Fast PNG 压缩写入磁盘。
pub(crate) fn write_image_to_file(
    path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    write_buffered_file(path, |writer| {
        encode_png_to_writer(writer, rgba, width, height)
    })
}

/// Encodes a PNG and confirms that every buffered byte reached the underlying
/// writer before reporting success. Public only as a narrow integration-test
/// seam for final-write failures; production callers write through artifact
/// paths above.
#[doc(hidden)]
pub fn encode_png_to_writer<W: Write>(
    writer: &mut W,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let mut encoder = png::Encoder::new(&mut *writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_filter(png::FilterType::NoFilter);
    encoder.set_adaptive_filter(png::AdaptiveFilterType::NonAdaptive);
    let mut png_writer = encoder.write_header().map_err(|error| error.to_string())?;
    png_writer
        .write_image_data(rgba)
        .map_err(|error| error.to_string())?;
    png_writer.finish().map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

/// JPEG counterpart to [`encode_png_to_writer`].
#[doc(hidden)]
pub fn encode_jpeg_to_writer<W: Write>(
    writer: &mut W,
    rgb: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    JpegEncoder::new(&mut *writer)
        .write_image(rgb, width, height, image::ColorType::Rgb8)
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

pub(crate) fn needs_downscale(width: u32, height: u32) -> bool {
    width > THUMB_MAX_W || height > THUMB_MAX_H
}

pub(crate) fn preview_asset_dimensions(width: u32, height: u32) -> (u32, u32) {
    if !needs_downscale(width, height) {
        return (width, height);
    }
    let scale = (width as f32 / THUMB_MAX_W as f32)
        .max(height as f32 / THUMB_MAX_H as f32)
        .max(1.0);
    (
        (width as f32 / scale).round() as u32,
        (height as f32 / scale).round() as u32,
    )
}

pub(crate) fn has_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|px| px[3] != 255)
}

pub(crate) fn choose_preview_format(rgba: &[u8], width: u32, height: u32) -> PreviewAssetFormat {
    if has_alpha(rgba) || !needs_downscale(width, height) {
        PreviewAssetFormat::Png
    } else {
        PreviewAssetFormat::Jpeg
    }
}

/// 从 RGBA 原始字节生成列表预览资产。
/// 有 alpha 的图片和小图保存为 PNG；大图且无 alpha 时保存为 JPEG 以控制体积。
pub(crate) fn save_preview_asset(
    rgba: &[u8],
    width: u32,
    height: u32,
    path: &Path,
    format: PreviewAssetFormat,
) -> Result<(), String> {
    let preview_rgba = if needs_downscale(width, height) {
        thumbnail_from_raw(rgba, width, height)
    } else {
        RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| "Invalid image buffer".to_string())?
    };
    match format {
        PreviewAssetFormat::Png => write_image_to_file(
            path,
            preview_rgba.as_raw(),
            preview_rgba.width(),
            preview_rgba.height(),
        ),
        PreviewAssetFormat::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(preview_rgba).to_rgb8();
            write_buffered_file(path, |writer| {
                encode_jpeg_to_writer(writer, rgb.as_raw(), rgb.width(), rgb.height())
            })
        }
    }
}

fn write_buffered_file(
    path: &Path,
    encode: impl FnOnce(&mut BufWriter<std::fs::File>) -> Result<(), String>,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    let result = encode(&mut writer);

    // Close the handle before cleanup so removal also works on Windows.
    drop(writer);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// 对 4K 输入：全量 RgbaImage 方法需要 ~32 MB 拷贝 + 8M 像素遍历；
/// 此函数只遍历 600×300 = 180K 目标像素，速度快约 40 倍，内存分配也小得多。
fn thumbnail_from_raw(bytes: &[u8], src_w: u32, src_h: u32) -> RgbaImage {
    let scale = (src_w as f32 / THUMB_MAX_W as f32)
        .max(src_h as f32 / THUMB_MAX_H as f32)
        .max(1.0);
    let (dst_w, dst_h) = preview_asset_dimensions(src_w, src_h);
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = ((dx as f32 + 0.5) * scale) as u32;
            let sy = ((dy as f32 + 0.5) * scale) as u32;
            let si = ((sy.min(src_h - 1) * src_w + sx.min(src_w - 1)) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            out[di..di + 4].copy_from_slice(&bytes[si..si + 4]);
        }
    }
    RgbaImage::from_raw(dst_w, dst_h, out).unwrap_or_default()
}

/// 对完整 RGBA 字节、尺寸和长度进行 BLAKE3 哈希。
/// 仅用于剪贴板会话去重，不作为图片加密、认证或安全边界。
pub fn hash_image_content(img: &ClipboardImage) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(img.width as u64).to_le_bytes());
    hasher.update(&(img.height as u64).to_le_bytes());
    hasher.update(&(img.bytes.len() as u64).to_le_bytes());
    hasher.update(&img.bytes);
    hasher.finalize().to_hex().to_string()
}
