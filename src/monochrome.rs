//! Convert an image to solid black and white, compositing transparency onto white.
use image::{DynamicImage, Rgba, RgbaImage};

pub fn threshold(image: &DynamicImage, cutoff: u8) -> RgbaImage {
    let mut result = image.to_rgba8();
    for pixel in result.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let luminance = (u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722) / 10000;
        let on_white = (luminance * u32::from(a) + 255 * (255 - u32::from(a))) / 255;
        let value = if on_white < u32::from(cutoff) { 0 } else { 255 };
        *pixel = Rgba([value, value, value, 255]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn transparent_black_is_white_and_gray_thresholds() {
        let mut image = RgbaImage::new(3, 1);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([64, 64, 64, 255]));
        image.put_pixel(2, 0, Rgba([192, 192, 192, 255]));
        let out = threshold(&DynamicImage::ImageRgba8(image), 128);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [0, 0, 0, 255]);
        assert_eq!(out.get_pixel(2, 0).0, [255, 255, 255, 255]);
    }
}
