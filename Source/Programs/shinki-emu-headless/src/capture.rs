use crate::{Arguments, Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use shinki_emu_manager_core::model::{FrameFormat, FrameLease, FrameRowOrder};
use std::path::Path;

#[derive(Debug, Clone, clap::Args)]
pub struct CompareArguments {
    #[arg(long)]
    pub actual: std::path::PathBuf,
    #[arg(long)]
    pub baseline: std::path::PathBuf,
    #[arg(long, default_value_t = 0)]
    pub max_channel_delta: u8,
    #[arg(long, default_value_t = 0.0)]
    pub changed_pixel_ratio: f64,
}

pub fn compare_cli(args: CompareArguments) -> std::process::ExitCode {
    let result = (|| -> Result<Comparison> {
        if !(0.0..=1.0).contains(&args.changed_pixel_ratio) {
            return Err(Error::Run("changed-pixel-ratio must be in 0..=1".into()));
        }
        let actual = image::open(args.actual)?.to_rgba8();
        let expected = image::open(args.baseline)?.to_rgba8();
        compare(
            actual.as_raw(),
            actual.width(),
            actual.height(),
            expected.as_raw(),
            expected.width(),
            expected.height(),
            args.max_channel_delta,
            args.changed_pixel_ratio,
        )
    })();
    match result {
        Ok(comparison) => {
            println!(
                "{}",
                serde_json::to_string(&comparison).expect("serializable comparison")
            );
            std::process::ExitCode::from(if comparison.passed { 0 } else { 3 })
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(error.exit_code())
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Capture {
    pub id: String,
    pub sample: u64,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub pixel_hash: String,
    pub png_hash: String,
    pub comparison: Option<Comparison>,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub passed: bool,
    pub exact: bool,
    pub changed_pixel_ratio: f64,
    pub max_channel_delta: u8,
}
pub fn hash(bytes: &[u8]) -> String {
    encode_hash(&Sha256::digest(bytes))
}

fn encode_hash(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(7 + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}

pub(crate) fn canonical(frame: &FrameLease) -> Result<Vec<u8>> {
    let mut pixels = Vec::with_capacity(frame.info.width as usize * frame.info.height as usize * 4);
    canonical_rows(frame, |row| pixels.extend_from_slice(row))?;
    Ok(pixels)
}

fn canonical_rows(frame: &FrameLease, mut consume: impl FnMut(&[u8])) -> Result<()> {
    frame.validate()?;
    let info = frame.info;
    let bytes = frame
        .pixels()
        .ok_or_else(|| Error::Run("missing CPU frame pixels".into()))?;
    // FrameInfo validates the 16384-pixel limit; scratch storage stays on the owner stack.
    let mut normalized = [0u8; 16384 * 4];
    let row_bytes = info.width as usize * 4;
    let bgra = matches!(
        info.format,
        FrameFormat::Bgra8Srgb { .. } | FrameFormat::Bgra8Linear { .. }
    );
    let linear = matches!(
        info.format,
        FrameFormat::Rgba8Linear { .. } | FrameFormat::Bgra8Linear { .. }
    );
    for y in 0..info.height as usize {
        let row = if info.rows == FrameRowOrder::TopToBottom {
            y
        } else {
            info.height as usize - 1 - y
        };
        let start = row * info.stride as usize;
        for (pixel, output) in bytes[start..start + row_bytes]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(normalized[..row_bytes].as_chunks_mut::<4>().0.iter_mut())
        {
            let rgb = if bgra {
                [pixel[2], pixel[1], pixel[0]]
            } else {
                [pixel[0], pixel[1], pixel[2]]
            };
            for (channel, value) in output[..3].iter_mut().zip(rgb) {
                *channel = if linear { linear_to_srgb(value) } else { value };
            }
            output[3] = 255;
        }
        consume(&normalized[..row_bytes]);
    }
    Ok(())
}
fn linear_to_srgb(value: u8) -> u8 {
    static TABLE: std::sync::LazyLock<[u8; 256]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|value| {
            let x = value as f64 / 255.0;
            let srgb = if x <= 0.0031308 {
                x * 12.92
            } else {
                1.055 * x.powf(1.0 / 2.4) - 0.055
            };
            (srgb * 255.0).round() as u8
        })
    });
    TABLE[value as usize]
}
pub(crate) fn pixel_hash(frame: &FrameLease) -> Result<String> {
    let mut digest = Sha256::new();
    canonical_rows(frame, |row| digest.update(row))?;
    Ok(encode_hash(&digest.finalize()))
}

pub(crate) fn save(frame: &FrameLease, id: &str, sample: u64, args: &Arguments) -> Result<Capture> {
    let rgba = canonical(frame)?;
    let mut capture = save_rgba(
        &rgba,
        frame.info.width,
        frame.info.height,
        id,
        sample,
        &args.output,
    )?;
    if let Some(baseline) = &args.baseline {
        let expected = image::open(baseline.join(format!("{id}.png")))?.to_rgba8();
        let comparison = compare(
            &rgba,
            frame.info.width,
            frame.info.height,
            expected.as_raw(),
            expected.width(),
            expected.height(),
            args.max_channel_delta,
            args.changed_pixel_ratio,
        )?;
        let passed = comparison.passed;
        capture.comparison = Some(comparison);
        std::fs::write(
            args.output.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&capture)?,
        )?;
        if !passed {
            return Err(Error::Assertion(format!(
                "checkpoint {id} differs from baseline"
            )));
        }
    }
    Ok(capture)
}

/// Shared PNG/hash convention for core frames and Manager software-rendered UI.
pub fn save_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    id: &str,
    sample: u64,
    output: &Path,
) -> Result<Capture> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::Run(
            "capture ID must contain only letters, digits, '-', '_' or '.'".into(),
        ));
    }
    if width == 0
        || height == 0
        || (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(rgba.len())
    {
        return Err(Error::Run("invalid RGBA capture dimensions".into()));
    }
    std::fs::create_dir_all(output)?;
    let filename = format!("{id}.png");
    let path = output.join(&filename);
    image::save_buffer_with_format(
        &path,
        rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    let capture = Capture {
        id: id.into(),
        sample,
        path: filename,
        width,
        height,
        pixel_hash: hash(rgba),
        png_hash: hash(&std::fs::read(&path)?),
        comparison: None,
    };
    std::fs::write(
        output.join(format!("{id}.json")),
        serde_json::to_vec_pretty(&capture)?,
    )?;
    Ok(capture)
}

#[allow(clippy::too_many_arguments)]
fn compare(
    actual: &[u8],
    width: u32,
    height: u32,
    expected: &[u8],
    expected_width: u32,
    expected_height: u32,
    threshold: u8,
    allowed_ratio: f64,
) -> Result<Comparison> {
    if (width, height) != (expected_width, expected_height) || actual.len() != expected.len() {
        return Err(Error::Assertion("baseline dimensions differ".into()));
    }
    let mut changed = 0usize;
    let mut max_delta = 0;
    for (a, b) in actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0.iter())
    {
        let delta = a
            .iter()
            .zip(b)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        max_delta = max_delta.max(delta);
        changed += usize::from(delta > threshold);
    }
    let ratio = changed as f64 / (width as f64 * height as f64);
    Ok(Comparison {
        passed: ratio <= allowed_ratio,
        exact: max_delta == 0,
        changed_pixel_ratio: ratio,
        max_channel_delta: max_delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shinki_emu_manager_core::model::*;
    #[test]
    fn normalizes_stride_bgra_bottom_up_and_ignored_alpha() {
        let frame = FrameLease::from_vec(
            vec![30, 20, 10, 0, 99, 99, 99, 99, 60, 50, 40, 7, 99, 99, 99, 99],
            FrameInfo {
                width: 1,
                height: 2,
                logical_width: 1,
                logical_height: 2,
                stride: 8,
                format: FrameFormat::Bgra8Srgb {
                    alpha: FrameAlpha::Ignored,
                },
                rows: FrameRowOrder::BottomToTop,
            },
            FrameIdentity {
                session_epoch: 1,
                output_epoch: 1,
                generation: 1,
            },
        )
        .unwrap();
        assert_eq!(
            canonical(&frame).unwrap(),
            [40, 50, 60, 255, 10, 20, 30, 255]
        );
    }
    #[test]
    fn pixel_comparison_enforces_exact_and_tolerance_boundaries() {
        let a = [0, 0, 0, 255, 10, 10, 10, 255];
        let b = [1, 0, 0, 255, 20, 10, 10, 255];
        assert!(!compare(&a, 2, 1, &b, 2, 1, 0, 0.0).unwrap().passed);
        assert!(compare(&a, 2, 1, &a, 2, 1, 0, 0.0).unwrap().exact);
        assert!(compare(&a, 2, 1, &b, 2, 1, 1, 0.5).unwrap().passed);
        assert!(!compare(&a, 2, 1, &b, 2, 1, 1, 0.49).unwrap().passed);
        assert!(compare(&a, 2, 1, &b, 1, 2, 255, 1.0).is_err());
    }
    #[test]
    fn writes_png_and_hash_that_roundtrip_to_canonical_pixels() {
        let output = tempfile::tempdir().unwrap();
        let pixels = [20, 40, 60, 255];
        let saved = save_rgba(&pixels, 1, 1, "checkpoint", 1, output.path()).unwrap();
        assert_eq!(
            image::open(output.path().join(saved.path))
                .unwrap()
                .to_rgba8()
                .as_raw(),
            &pixels
        );
        assert_eq!(saved.pixel_hash, hash(&pixels));
        assert!(save_rgba(&pixels, 1, 1, "../escape", 1, output.path()).is_err());
    }
}
