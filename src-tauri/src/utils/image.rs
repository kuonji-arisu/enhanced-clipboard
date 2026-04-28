/// 图片处理工具：文件写入、展示图生成、BLAKE3 内容哈希。
use std::io::BufWriter;
use std::path::Path;

use arboard::ImageData as ClipboardImage;
use image::codecs::png::{CompressionType as PngCompression, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, ImageEncoder, RgbaImage};

/// 缩略图最大宽度（像素）
pub(crate) const THUMB_MAX_W: u32 = 600;

/// 缩略图最大高度（像素）
pub(crate) const THUMB_MAX_H: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayAssetFormat {
    Png,
    Jpeg,
}

impl DisplayAssetFormat {
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
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let buf = BufWriter::new(file);
    PngEncoder::new_with_quality(buf, PngCompression::Fast, PngFilter::NoFilter)
        .write_image(rgba, width, height, image::ColorType::Rgba8)
        .map_err(|e| {
            let _ = std::fs::remove_file(path);
            e.to_string()
        })
}

pub(crate) fn needs_downscale(width: u32, height: u32) -> bool {
    width > THUMB_MAX_W || height > THUMB_MAX_H
}

pub(crate) fn has_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|px| px[3] != 255)
}

pub(crate) fn choose_display_format(rgba: &[u8], width: u32, height: u32) -> DisplayAssetFormat {
    if has_alpha(rgba) || !needs_downscale(width, height) {
        DisplayAssetFormat::Png
    } else {
        DisplayAssetFormat::Jpeg
    }
}

/// 从 RGBA 原始字节生成列表展示资产。
/// 有 alpha 的图片和小图保存为 PNG；大图且无 alpha 时保存为 JPEG 以控制体积。
pub(crate) fn save_display_asset(
    rgba: &[u8],
    width: u32,
    height: u32,
    path: &Path,
    format: DisplayAssetFormat,
) -> Result<(), String> {
    let display_rgba = if needs_downscale(width, height) {
        thumbnail_from_raw(rgba, width, height, THUMB_MAX_W, THUMB_MAX_H)
    } else {
        RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| "Invalid image buffer".to_string())?
    };
    match format {
        DisplayAssetFormat::Png => write_image_to_file(
            path,
            display_rgba.as_raw(),
            display_rgba.width(),
            display_rgba.height(),
        ),
        DisplayAssetFormat::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(display_rgba).to_rgb8();
            rgb.save(path).map_err(|e| e.to_string())
        }
    }
}

/// 对 4K 输入：全量 RgbaImage 方法需要 ~32 MB 拷贝 + 8M 像素遍历；
/// 此函数只遍历 600×300 = 180K 目标像素，速度快约 40 倍，内存分配也小得多。
fn thumbnail_from_raw(bytes: &[u8], src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> RgbaImage {
    let scale = (src_w as f32 / max_w as f32)
        .max(src_h as f32 / max_h as f32)
        .max(1.0);
    let dst_w = (src_w as f32 / scale).round() as u32;
    let dst_h = (src_h as f32 / scale).round() as u32;
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
