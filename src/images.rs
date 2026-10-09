#[allow(unused_imports)]
use std::sync::Arc;
use image::{ColorType, DynamicImage, GenericImageView, ImageError};
use image::codecs::bmp::BmpEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::{Kind, StreamDeckError};
use crate::info::{ImageFormat, ImageMirroring, ImageMode, ImageRotation};

/// Converts image into image data depending on provided kind of device
pub fn convert_image(kind: Kind, image: DynamicImage) -> Result<Vec<u8>, ImageError> {
    convert_image_with_format(kind.key_image_format(), image)
}

/// Converts image into image data depending on provided image format
pub fn convert_image_with_format(image_format: ImageFormat, image: DynamicImage) -> Result<Vec<u8>, ImageError> {
    // Ensuring size of the image
    let (ws, hs) = image_format.size;

    // Applying rotation
    let image = match image_format.rotation {
        ImageRotation::Rot0 => image,
        ImageRotation::Rot90 => image.rotate90(),
        ImageRotation::Rot180 => image.rotate180(),
        ImageRotation::Rot270 => image.rotate270(),
    };

    let image = image.resize_exact(ws as u32, hs as u32, FilterType::Nearest);

    // Applying mirroring
    let image = match image_format.mirror {
        ImageMirroring::None => image,
        ImageMirroring::X => image.fliph(),
        ImageMirroring::Y => image.flipv(),
        ImageMirroring::Both => image.fliph().flipv(),
    };

    let image_data = image.into_rgb8().to_vec();

    // Encoding image
    match image_format.mode {
        ImageMode::None => Ok(vec![]),
        ImageMode::BMP => {
            let mut buf = Vec::new();
            let mut encoder = BmpEncoder::new(&mut buf);
            encoder.encode(&image_data, ws as u32, hs as u32, ColorType::Rgb8.into())?;
            Ok(buf)
        }
        ImageMode::JPEG => {
            let mut buf = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut buf, 90);
            encoder.encode(&image_data, ws as u32, hs as u32, ColorType::Rgb8.into())?;
            Ok(buf)
        }
    }
}

/// Converts image into image data depending on provided kind of device, can be safely ran inside [multi_thread](tokio::runtime::Builder::new_multi_thread) runtime
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub fn convert_image_async(kind: Kind, image: DynamicImage) -> Result<Vec<u8>, StreamDeckError> {
    Ok(tokio::task::block_in_place(move || convert_image(kind, image))?)
}

/// Converts image into image data depending on provided image format, can be safely ran inside [multi_thread](tokio::runtime::Builder::new_multi_thread) runtime
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub fn convert_image_with_format_async(format: ImageFormat, image: DynamicImage) -> Result<Vec<u8>, StreamDeckError> {
    Ok(tokio::task::block_in_place(move || convert_image_with_format(format, image))?)
}

/// Rect to be used when trying to send image to lcd screen
pub struct ImageRect {
    /// Width of the image
    pub w: u16,

    /// Height of the image
    pub h: u16,

    /// Data of the image row by row as RGB
    pub data: Vec<u8>,
}

impl ImageRect {
    /// Converts image to image rect
    pub fn from_image(image: DynamicImage) -> Result<ImageRect, StreamDeckError> {
        let (image_w, image_h) = image.dimensions();

        let image_data = image.into_rgb8().to_vec();

        let mut buf = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut buf, 90);
        encoder.encode(&image_data, image_w, image_h, ColorType::Rgb8.into())?;

        Ok(ImageRect {
            w: image_w as u16,
            h: image_h as u16,
            data: buf,
        })
    }

    /// Converts image to image rect, can be safely ran inside [multi_thread](tokio::runtime::Builder::new_multi_thread) runtime
    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    pub fn from_image_async(image: DynamicImage) -> Result<ImageRect, StreamDeckError> {
        tokio::task::block_in_place(move || ImageRect::from_image(image))
    }

    /// Converts image to image rect in the JPEG flavour the device's screen can decode.
    /// The Galleon 100 SD's screen shows 4:4:4 JPEGs as diagonal stripes, so its images use
    /// 4:2:0 chroma subsampling. Other devices get the same output as [ImageRect::from_image].
    pub fn from_image_for_kind(kind: Kind, image: DynamicImage) -> Result<ImageRect, StreamDeckError> {
        if kind != Kind::Galleon100Sd {
            return ImageRect::from_image(image);
        }

        let image = image.into_rgb8();
        let (image_w, image_h) = image.dimensions();

        let mut buf = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut buf, 90);
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
        encoder
            .encode(image.as_raw(), image_w as u16, image_h as u16, jpeg_encoder::ColorType::Rgb)
            .map_err(|e| ImageError::Encoding(image::error::EncodingError::new(image::ImageFormat::Jpeg.into(), e)))?;

        Ok(ImageRect {
            w: image_w as u16,
            h: image_h as u16,
            data: buf,
        })
    }

    /// Converts image to image rect for the device, can be safely ran inside [multi_thread](tokio::runtime::Builder::new_multi_thread) runtime
    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    pub fn from_image_for_kind_async(kind: Kind, image: DynamicImage) -> Result<ImageRect, StreamDeckError> {
        tokio::task::block_in_place(move || ImageRect::from_image_for_kind(kind, image))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Horizontal and vertical sampling factors of the luma component in a baseline JPEG
    fn luma_sampling(jpeg: &[u8]) -> (u8, u8) {
        let sof = jpeg.windows(2).position(|w| w == [0xff, 0xc0]).expect("baseline SOF0 marker");
        let factors = jpeg[sof + 11];
        (factors >> 4, factors & 0x0f)
    }

    #[test]
    fn galleon_screen_images_use_420_subsampling() {
        let image = DynamicImage::new_rgb8(360, 384);
        let rect = ImageRect::from_image_for_kind(Kind::Galleon100Sd, image).unwrap();

        assert_eq!((rect.w, rect.h), (360, 384));
        assert_eq!(luma_sampling(&rect.data), (2, 2));
    }

    #[test]
    fn other_devices_keep_the_existing_encoding() {
        let image = DynamicImage::new_rgb8(200, 100);
        let rect = ImageRect::from_image_for_kind(Kind::Plus, image.clone()).unwrap();

        assert_eq!(rect.data, ImageRect::from_image(image).unwrap().data);
    }
}
