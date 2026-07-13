use std::io::{self, Write};

use enhanced_clipboard_lib::utils::image::{encode_jpeg_to_writer, encode_png_to_writer};

#[derive(Default)]
struct FlushFailWriter {
    bytes: Vec<u8>,
    flush_calls: usize,
}

impl Write for FlushFailWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_calls += 1;
        Err(io::Error::other("injected final flush failure"))
    }
}

#[test]
fn png_encoder_reports_final_flush_failure() {
    let mut writer = FlushFailWriter::default();

    let error = encode_png_to_writer(&mut writer, &[10, 20, 30, 255], 1, 1)
        .expect_err("final PNG flush should fail");

    assert!(error.contains("injected final flush failure"));
    assert_eq!(writer.flush_calls, 1);
    assert!(!writer.bytes.is_empty());
}

#[test]
fn jpeg_encoder_reports_final_flush_failure() {
    let mut writer = FlushFailWriter::default();

    let error = encode_jpeg_to_writer(&mut writer, &[10, 20, 30], 1, 1)
        .expect_err("final JPEG flush should fail");

    assert!(error.contains("injected final flush failure"));
    assert_eq!(writer.flush_calls, 1);
    assert!(!writer.bytes.is_empty());
}

#[test]
fn successful_png_and_jpeg_outputs_are_decodable() {
    let mut png = Vec::new();
    encode_png_to_writer(&mut png, &[10, 20, 30, 255], 1, 1).expect("encode PNG");
    let decoded_png = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .expect("decode encoded PNG");
    assert_eq!((decoded_png.width(), decoded_png.height()), (1, 1));

    let mut jpeg = Vec::new();
    encode_jpeg_to_writer(&mut jpeg, &[10, 20, 30], 1, 1).expect("encode JPEG");
    let decoded_jpeg = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .expect("decode encoded JPEG");
    assert_eq!((decoded_jpeg.width(), decoded_jpeg.height()), (1, 1));
}
