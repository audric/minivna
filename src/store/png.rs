//! Writing a picture of the graph.

use std::path::Path;

use super::FileError;

/// Write `width` x `height` pixels, 4 bytes each (red, green, blue, alpha).
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), FileError> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(FileError::Format(format!(
            "{} bytes of pixels do not make a {width} x {height} picture",
            rgba.len()
        )));
    }
    image::save_buffer_with_format(path, rgba, width, height, image::ColorType::Rgba8, image::ImageFormat::Png)
        .map_err(|e| match e {
            image::ImageError::IoError(io) => FileError::Io(io),
            other => FileError::Format(other.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("graph.png");
        let pixels: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 9, 8, 7, 255];
        write_png(&path, 2, 2, &pixels).unwrap();
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (2, 2));
        assert_eq!(back.into_raw(), pixels);
    }

    #[test]
    fn wrong_buffer_size_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(write_png(&dir.path().join("x.png"), 2, 2, &[0; 5]), Err(FileError::Format(_))));
    }
}
