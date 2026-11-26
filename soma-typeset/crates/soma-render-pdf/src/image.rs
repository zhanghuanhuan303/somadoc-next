//! Image assets: PNG is decoded to raw samples, JPEG travels as it is.
//!
//! The renderer never touches the file system — the caller supplies bytes
//! through [`ImageSource`](crate::ImageSource) — so this module only has to turn
//! a byte slice into what a PDF image XObject needs: a colour space, a size,
//! and either raw samples (`/FlateDecode`) or the original JPEG
//! (`/DCTDecode`, which every reader decodes itself).
//!
//! PNG support is deliberately narrow: 8 bits per sample, no interlacing, no
//! tRNS. That is what figures in documents actually are, and a format the
//! renderer cannot read falls back to the placeholder box instead of failing
//! the page.

use alloc::vec;
use alloc::vec::Vec;

use crate::font::PdfError;
use crate::inflate;

/// How the samples reach a PDF reader.
pub(crate) enum Filter {
    /// Raw samples, deflated by the writer.
    Flate,
    /// The original JPEG, decoded by the reader.
    Dct,
}

pub(crate) enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
}

impl ColorSpace {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Gray => "/DeviceGray",
            Self::Rgb => "/DeviceRGB",
            Self::Cmyk => "/DeviceCMYK",
        }
    }
}

pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    pub filter: Filter,
    /// Samples (packed when needed) or the JPEG stream.
    pub data: Vec<u8>,
}

/// Decode `bytes`, or say why the format is unsupported.
pub(crate) fn decode(bytes: &[u8]) -> Result<Image, PdfError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(bytes);
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return jpeg(bytes);
    }
    Err(PdfError::new(
        "image format is unsupported: this backend reads PNG and JPEG",
    ))
}

fn png(data: &[u8]) -> Result<Image, PdfError> {
    let mut cursor = 8usize;
    let mut header: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Vec<u8> = Vec::new();
    let mut compressed: Vec<u8> = Vec::new();
    while cursor + 8 <= data.len() {
        let length = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        let kind = &data[cursor + 4..cursor + 8];
        let body = data
            .get(cursor + 8..cursor + 8 + length)
            .ok_or_else(|| PdfError::new("PNG chunk is truncated"))?;
        match kind {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(PdfError::new("PNG header is truncated"));
                }
                let width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                let height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                header = Some((width, height, body[8], body[9]));
                if body[10] != 0 || body[11] != 0 || body[12] != 0 {
                    return Err(PdfError::new(
                        "PNG must use the standard compression, filter and no interlacing",
                    ));
                }
            }
            b"PLTE" => palette = body.to_vec(),
            b"IDAT" => compressed.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        cursor += 12 + length;
    }
    let (width, height, depth, colour) =
        header.ok_or_else(|| PdfError::new("PNG has no header chunk"))?;
    if depth != 8 {
        return Err(PdfError::new(
            "PNG must use 8 bits per sample; this backend does not scale others",
        ));
    }
    let channels = match colour {
        0 => 1usize,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        other => {
            return Err(PdfError::new(alloc::format!(
                "PNG colour type {other} is unsupported"
            )))
        }
    };
    let stride = (width as usize)
        .checked_mul(channels)
        .ok_or_else(|| PdfError::new("PNG width is out of range"))?;
    let rows = height as usize;
    let raw = inflate::zlib(&compressed)?;
    if raw.len() < rows * (stride + 1) {
        return Err(PdfError::new("PNG image data is truncated"));
    }

    let mut pixels = vec![0u8; rows * stride];
    for row in 0..rows {
        let start = row * (stride + 1);
        let filter = raw[start];
        let line = &raw[start + 1..start + 1 + stride];
        let above = if row == 0 { None } else { Some(row - 1) };
        unfilter(filter, line, &mut pixels, row, stride, channels, above)?;
    }

    let (color_space, data) = match colour {
        0 => (ColorSpace::Gray, pixels),
        2 => (ColorSpace::Rgb, pixels),
        3 => {
            if palette.len() < 3 {
                return Err(PdfError::new("PNG palette is missing"));
            }
            let mut rgb = Vec::with_capacity(pixels.len() * 3);
            for index in &pixels {
                let at = *index as usize * 3;
                let entry = palette
                    .get(at..at + 3)
                    .ok_or_else(|| PdfError::new("PNG palette index is out of range"))?;
                rgb.extend_from_slice(entry);
            }
            (ColorSpace::Rgb, rgb)
        }
        4 => (ColorSpace::Gray, over_white(&pixels, 1)),
        _ => (ColorSpace::Rgb, over_white(&pixels, 3)),
    };

    Ok(Image {
        width,
        height,
        color_space,
        filter: Filter::Flate,
        data,
    })
}

/// Undo one PNG scanline filter, writing into `pixels` in place.
fn unfilter(
    filter: u8,
    line: &[u8],
    pixels: &mut [u8],
    row: usize,
    stride: usize,
    channels: usize,
    above: Option<usize>,
) -> Result<(), PdfError> {
    let out = row * stride;
    for index in 0..stride {
        let left = if index >= channels {
            pixels[out + index - channels]
        } else {
            0
        };
        let up = above.map_or(0, |row| pixels[row * stride + index]);
        let up_left = match above {
            Some(row) if index >= channels => pixels[row * stride + index - channels],
            _ => 0,
        };
        let value = match filter {
            0 => line[index],
            1 => line[index].wrapping_add(left),
            2 => line[index].wrapping_add(up),
            3 => {
                let average = ((u16::from(left) + u16::from(up)) / 2) as u8;
                line[index].wrapping_add(average)
            }
            4 => line[index].wrapping_add(paeth(left, up, up_left)),
            other => {
                return Err(PdfError::new(alloc::format!(
                    "PNG scanline filter {other} is unsupported"
                )))
            }
        };
        pixels[out + index] = value;
    }
    Ok(())
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let p = i16::from(left) + i16::from(up) - i16::from(up_left);
    let pa = (p - i16::from(left)).abs();
    let pb = (p - i16::from(up)).abs();
    let pc = (p - i16::from(up_left)).abs();
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Drop the alpha channel by compositing over white, which is what a figure on
/// paper looks like. Integer arithmetic only, rounded half up.
fn over_white(pixels: &[u8], channels: usize) -> Vec<u8> {
    let pixels_per_channel = pixels.len() / (channels + 1);
    let mut out = Vec::with_capacity(pixels_per_channel * channels);
    for pixel in 0..pixels_per_channel {
        let base = pixel * (channels + 1);
        let alpha = u32::from(pixels[base + channels]);
        for channel in 0..channels {
            let value = u32::from(pixels[base + channel]) * alpha + 255 * (255 - alpha);
            out.push(((value + 127) / 255) as u8);
        }
    }
    out
}

fn jpeg(data: &[u8]) -> Result<Image, PdfError> {
    let mut cursor = 2usize;
    while cursor + 2 <= data.len() {
        if data[cursor] != 0xff {
            return Err(PdfError::new("JPEG marker is missing"));
        }
        let marker = data[cursor + 1];
        cursor += 2;
        // Markers without a length: SOI, TEM and the restart markers.
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xd9 {
            break;
        }
        let length = data
            .get(cursor..cursor + 2)
            .map(|slice| usize::from(u16::from_be_bytes([slice[0], slice[1]])))
            .ok_or_else(|| PdfError::new("JPEG segment is truncated"))?;
        // SOF0..SOF15 except DHT (C4), JPG (C8) and DAC (CC) carry the frame.
        let frame = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if frame {
            let body = data
                .get(cursor..cursor + length)
                .ok_or_else(|| PdfError::new("JPEG frame header is truncated"))?;
            if body.len() < 8 {
                return Err(PdfError::new("JPEG frame header is too short"));
            }
            let height = u32::from(u16::from_be_bytes([body[3], body[4]]));
            let width = u32::from(u16::from_be_bytes([body[5], body[6]]));
            let components = body[7];
            let color_space = match components {
                1 => ColorSpace::Gray,
                3 => ColorSpace::Rgb,
                4 => ColorSpace::Cmyk,
                other => {
                    return Err(PdfError::new(alloc::format!(
                        "JPEG with {other} components is unsupported"
                    )))
                }
            };
            return Ok(Image {
                width,
                height,
                color_space,
                filter: Filter::Dct,
                data: data.to_vec(),
            });
        }
        cursor += length;
    }
    Err(PdfError::new("JPEG has no frame header"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PNG built by hand: `width` × `height` 8-bit RGB, all one colour,
    /// scanline filter 0, deflated with our own encoder.
    fn png_of(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.resize(raw.len() + 1, 0);
            for _ in 0..width {
                raw.extend_from_slice(&rgb);
            }
        }
        let compressed = crate::deflate::zlib(&raw).expect("compresses");

        let mut out = Vec::new();
        out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out.extend_from_slice(&[0, 0, 0, 0]); // CRC: the decoder ignores it
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &compressed);
        chunk(b"IEND", &[]);
        out
    }

    #[test]
    fn a_flat_rgb_png_decodes() {
        let bytes = png_of(4, 3, [0x12, 0x34, 0x56]);
        let image = decode(&bytes).expect("decodes");
        assert_eq!(image.width, 4);
        assert_eq!(image.height, 3);
        assert!(matches!(image.filter, Filter::Flate));
        assert_eq!(image.data.len(), 4 * 3 * 3);
        assert_eq!(&image.data[..3], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn a_jpeg_frame_header_is_read_without_decoding() {
        // SOI, APP0 (minimal), SOF0, EOI.
        let mut jpeg = vec![0xff, 0xd8];
        jpeg.extend_from_slice(&[0xff, 0xe0, 0x00, 0x06, 1, 2, 3, 4]);
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 8]);
        jpeg.extend_from_slice(&[0x01, 0x40]); // height 320
        jpeg.extend_from_slice(&[0x02, 0x80]); // width 640
        jpeg.push(3); // components
        jpeg.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        let image = decode(&jpeg).expect("decodes");
        assert_eq!(image.width, 640);
        assert_eq!(image.height, 320);
        assert!(matches!(image.filter, Filter::Dct));
        assert!(matches!(image.color_space, ColorSpace::Rgb));
        assert_eq!(image.data, jpeg, "a JPEG travels byte for byte");
    }

    #[test]
    fn an_unknown_format_is_named() {
        let error = decode(b"GIF89a").err().expect("rejected");
        assert!(error.reason.contains("PNG and JPEG"), "{}", error.reason);
    }

    #[test]
    fn alpha_composites_over_white() {
        // 50% white pixel and 50% black pixel, in RGBA.
        let pixels = [255, 255, 255, 128, 0, 0, 0, 128];
        let out = over_white(&pixels, 3);
        assert_eq!(out, vec![255, 255, 255, 127, 127, 127]);
    }
}
