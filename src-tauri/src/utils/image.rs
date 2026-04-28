/// 图片处理工具：文件写入、展示图生成、SHA-256 内容哈希、快速指纹。
use std::io::BufWriter;
use std::path::Path;

use arboard::ImageData as ClipboardImage;
use image::codecs::png::{CompressionType as PngCompression, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, ImageEncoder, RgbaImage};
use sha2::{Digest, Sha256};

/// 缩略图最大宽度（像素）
pub(crate) const THUMB_MAX_W: u32 = 600;

/// 缩略图最大高度（像素）
pub(crate) const THUMB_MAX_H: u32 = 300;

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

/// 从 RGBA 原始字节生成列表展示图并保存为 JPEG 文件。
/// 大图会缩小到展示尺寸；小图也写入独立 display asset，避免前端加载原图路径。
pub(crate) fn save_thumbnail(
    rgba: &[u8],
    width: u32,
    height: u32,
    path: &Path,
) -> Result<(), String> {
    let thumb_rgba = if width <= THUMB_MAX_W && height <= THUMB_MAX_H {
        RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| "Invalid image buffer".to_string())?
    } else {
        thumbnail_from_raw(rgba, width, height, THUMB_MAX_W, THUMB_MAX_H)
    };
    // JPEG 不支持 Alpha 通道，转换为 RGB 后保存
    let rgb = DynamicImage::ImageRgba8(thumb_rgba).to_rgb8();
    rgb.save(path).map_err(|e| e.to_string())
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

/// 对完整 RGBA 字节、尺寸和长度进行 SHA-256。用于剪贴板会话去重；
/// quick fingerprint 只能作为廉价提示，不能作为最终相等判断。
pub(crate) fn hash_image_content(img: &ClipboardImage) -> String {
    hash_image_bytes(img.width, img.height, &img.bytes)
}

pub(crate) fn hash_image_bytes(width: usize, height: usize, bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update((width as u64).to_le_bytes());
    hasher.update((height as u64).to_le_bytes());
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 基于元数据（宽、高、字节数）和五个关键像素（四角 + 中心）进行异或运算生成快速指纹。
/// 仅用于廉价确认“图片一定变化”：指纹不同可直接进入捕获流程；
/// 指纹相同绝不能作为拒绝依据，仍需完整内容 hash disambiguate。
pub(crate) fn image_quick_fingerprint(img: &ClipboardImage) -> u64 {
    let w = img.width;
    let h = img.height;
    let bytes = &img.bytes;
    // 元数据混入（宽高放入高低 32 位，字节数乘以黄金比例质数散列）
    let mut fp = (w as u64) | ((h as u64) << 32);
    fp ^= (bytes.len() as u64).wrapping_mul(0x9e3779b97f4a7c15);
    // 采样像素（每像素 RGBA 4 字节）
    let sample = |x: usize, y: usize| -> u64 {
        let idx = (y * w + x) * 4;
        if idx + 4 <= bytes.len() {
            u32::from_le_bytes([bytes[idx], bytes[idx + 1], bytes[idx + 2], bytes[idx + 3]]) as u64
        } else {
            0
        }
    };
    fp ^= sample(0, 0);
    fp ^= sample(w.saturating_sub(1), 0);
    fp ^= sample(0, h.saturating_sub(1));
    fp ^= sample(w.saturating_sub(1), h.saturating_sub(1));
    fp ^= sample(w / 2, h / 2);
    fp
}
