/// 图片处理工具：文件写入、缩略图生成、SHA-256 采样哈希、快速指纹。
use std::io::BufWriter;
use std::path::Path;

use arboard::ImageData as ClipboardImage;
use image::codecs::png::{CompressionType as PngCompression, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, ImageEncoder, RgbaImage};
use sha2::{Digest, Sha256};

/// 图片哈希采样字节数（8 KB）
const IMAGE_HASH_SAMPLE_BYTES: usize = 8192;

/// 缩略图最大宽度（像素）
const THUMB_MAX_W: u32 = 600;

/// 缩略图最大高度（像素）
const THUMB_MAX_H: u32 = 300;

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

/// 从 RGBA 原始字节生成缩略图并保存为 JPEG 文件。
///
/// 返回值语义：
/// - `Ok(true)` — 缩略图已写入 `path`（图片超过阈值）
/// - `Ok(false)` — 图片本身已足够小，无需缩略图
/// - `Err` — 尺寸超过阈值但 JPEG 写入失败（磁盘满、权限等）
pub(crate) fn save_thumbnail(
    rgba: &[u8],
    width: u32,
    height: u32,
    path: &Path,
) -> Result<bool, String> {
    if width <= THUMB_MAX_W && height <= THUMB_MAX_H {
        return Ok(false);
    }
    let thumb_rgba = thumbnail_from_raw(rgba, width, height, THUMB_MAX_W, THUMB_MAX_H);
    // JPEG 不支持 Alpha 通道，转换为 RGB 后保存
    let rgb = DynamicImage::ImageRgba8(thumb_rgba).to_rgb8();
    rgb.save(path).map(|_| true).map_err(|e| e.to_string())
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

/// 对原始字节的首、中、尾各取 IMAGE_HASH_SAMPLE_BYTES 字节 + 总长度进行 SHA-256 哈希。
/// 三段采样可有效区分仅中下部不同的图片（如滚动截图）。
pub(crate) fn hash_image_sample(data: &[u8]) -> String {
    let sample = IMAGE_HASH_SAMPLE_BYTES;
    let len = data.len();
    let mut hasher = Sha256::new();
    // 首段
    hasher.update(&data[..len.min(sample)]);
    // 中段
    if len > sample * 2 {
        let mid = len / 2;
        let half = sample / 2;
        hasher.update(&data[mid.saturating_sub(half)..len.min(mid + half)]);
    }
    // 尾段
    if len > sample {
        hasher.update(&data[len - len.min(sample)..]);
    }
    hasher.update(len.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

/// 基于元数据（宽、高、字节数）和五个关键像素（四角 + 中心）进行异或运算生成快速指纹。
/// 仅用于 SHA-256 前的廉价预筛：指纹不变则跳过，避免无意义的哈希开销。
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
