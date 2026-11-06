//! Physical-pixel geometry for the GPU experiment, independent of drivers.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[repr(C)]
pub(crate) struct Geometry {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub content_width: u32,
    pub content_height: u32,
    pub pad_x: u32,
    pub pad_y: u32,
}

#[derive(Default, Debug)]
pub(crate) struct Request {
    pub crop: Option<[u32; 4]>,
    pub size: Option<[u32; 2]>,
}

pub(crate) fn numbers<const N: usize>(value: &str, separator: char) -> Result<[u32; N], String> {
    value
        .split(separator)
        .map(|part| -> Result<u32, String> {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Geometry requires unsigned decimal integers".into());
            }
            part.parse()
                .map_err(|_| "Geometry integer is too large".into())
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| format!("Geometry needs {N} values separated by '{separator}'"))
}

impl Request {
    pub fn resolve(&self, source_width: u32, source_height: u32) -> Result<Geometry, String> {
        let [x, y, width, height] = self.crop.unwrap_or([0, 0, source_width, source_height]);
        let [output_width, output_height] = self.size.unwrap_or([width, height]);
        let result = Geometry {
            x,
            y,
            width,
            height,
            output_width,
            output_height,
            content_width: output_width,
            content_height: output_height,
            pad_x: 0,
            pad_y: 0,
        };
        result.validate(source_width, source_height)?;
        if u64::from(width) * u64::from(output_height)
            != u64::from(height) * u64::from(output_width)
        {
            return Err(
                "Output size must preserve the crop's aspect ratio; use record for fit/padding"
                    .into(),
            );
        }
        Ok(result)
    }

    pub fn resolve_fit(&self, source_width: u32, source_height: u32) -> Result<Geometry, String> {
        let [x, y, width, height] = self.crop.unwrap_or([0, 0, source_width, source_height]);
        let [output_width, output_height] = self.size.unwrap_or([width, height]);
        if width < 2 || height < 2 {
            return Err("Region is too small after conversion to even pixels".into());
        }
        let ratio = (output_width as f64 / width as f64).min(output_height as f64 / height as f64);
        let content_width = ((width as f64 * ratio).round() as u32).min(output_width) & !1;
        let content_height = ((height as f64 * ratio).round() as u32).min(output_height) & !1;
        let result = Geometry {
            x,
            y,
            width,
            height,
            output_width,
            output_height,
            content_width,
            content_height,
            pad_x: ((output_width - content_width) / 2) & !1,
            pad_y: ((output_height - content_height) / 2) & !1,
        };
        result.validate(source_width, source_height)?;
        Ok(result)
    }
}

impl Geometry {
    pub fn validate(self, source_width: u32, source_height: u32) -> Result<(), String> {
        if ![
            source_width,
            source_height,
            self.width,
            self.height,
            self.output_width,
            self.output_height,
            self.content_width,
            self.content_height,
        ]
        .iter()
        .all(|n| (2..=8192).contains(n) && n % 2 == 0)
        {
            return Err("GPU crop/size requires even dimensions from 2 to 8192 pixels".into());
        }
        if self
            .x
            .checked_add(self.width)
            .is_none_or(|end| end > source_width)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|end| end > source_height)
        {
            return Err("Crop lies outside the selected monitor's physical pixels".into());
        }
        if self
            .pad_x
            .checked_add(self.content_width)
            .is_none_or(|n| n > self.output_width)
            || self
                .pad_y
                .checked_add(self.content_height)
                .is_none_or(|n| n > self.output_height)
        {
            return Err("Content lies outside output padding".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_crop_scale_and_upscale_are_explicit() {
        assert_eq!(
            Request::default().resolve(1920, 1080).unwrap().output_width,
            1920
        );
        let request = Request {
            crop: Some([321, 181, 1280, 720]),
            size: Some([640, 360]),
        };
        let geometry = request.resolve(1920, 1080).unwrap();
        assert_eq!(
            (geometry.x, geometry.y, geometry.output_height),
            (321, 181, 360)
        );
        assert!(Request {
            size: Some([3840, 2160]),
            ..Request::default()
        }
        .resolve(1920, 1080)
        .is_ok());
    }
    #[test]
    fn reject_stretch_odd_empty_outside_and_overflow() {
        for request in [
            Request {
                crop: Some([u32::MAX, 0, 1280, 720]),
                size: None,
            },
            Request {
                crop: Some([641, 0, 1280, 720]),
                size: None,
            },
            Request {
                crop: Some([0, 0, 1279, 720]),
                size: None,
            },
            Request {
                crop: Some([0, 0, 0, 720]),
                size: None,
            },
            Request {
                crop: None,
                size: Some([1280, 800]),
            },
            Request {
                crop: None,
                size: Some([1280, 721]),
            },
        ] {
            assert!(request.resolve(1920, 1080).is_err(), "{request:?}");
        }
        for bad in [
            "1,2,3",
            "1,2,3,4,5",
            "-1,0,64,64",
            "0,0,64, 64",
            "0,0,64,4294967296",
        ] {
            assert!(numbers::<4>(bad, ',').is_err());
        }
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;
    #[test]
    fn fit_never_stretches_or_exceeds_the_canvas_and_padding_is_even() {
        for (width, height) in [(1920, 1080), (800, 600), (3440, 1440), (640, 480)] {
            for size in [[1280, 720], [1920, 1080], [2560, 1440], [3840, 2160]] {
                let g = Request {
                    crop: None,
                    size: Some(size),
                }
                .resolve_fit(width, height)
                .unwrap();
                assert!(g.pad_x + g.content_width <= size[0]);
                assert!(g.pad_y + g.content_height <= size[1]);
                assert_eq!((g.pad_x % 2, g.pad_y % 2), (0, 0));
                let ratio = (size[0] as f64 / width as f64).min(size[1] as f64 / height as f64);
                assert!((g.content_width as f64 - width as f64 * ratio).abs() < 2.0);
                assert!((g.content_height as f64 - height as f64 * ratio).abs() < 2.0);
            }
        }
    }
}
