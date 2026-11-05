//! Rasterize the canonical project mark for native build and package resources.
use anyhow::{Context, Result};
use resvg::{tiny_skia, usvg};
use std::{fs, path::Path};

pub struct Icon(usvg::Tree);

impl Icon {
    pub fn new() -> Result<Self> {
        Ok(Self(usvg::Tree::from_data(
            include_bytes!("../../Assets/Branding/shinkiemu-icon.svg"),
            &usvg::Options::default(),
        )?))
    }

    pub fn render(&self, size: u32, opaque: bool) -> Result<tiny_skia::Pixmap> {
        let mut image = tiny_skia::Pixmap::new(size, size).context("icon dimensions")?;
        if opaque {
            image.fill(tiny_skia::Color::from_rgba8(11, 18, 38, 255));
        }
        resvg::render(
            &self.0,
            tiny_skia::Transform::from_scale(
                size as f32 / self.0.size().width(),
                size as f32 / self.0.size().height(),
            ),
            &mut image.as_mut(),
        );
        Ok(image)
    }

    pub fn png(&self, size: u32, opaque: bool, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.render(size, opaque)?.encode_png()?)?;
        Ok(())
    }

    pub fn android(&self, root: &Path) -> Result<()> {
        let res = root.join("res");
        fs::create_dir_all(res.join("values"))?;
        fs::create_dir_all(res.join("mipmap-anydpi-v26"))?;
        for (density, size) in [
            ("mdpi", 48),
            ("hdpi", 72),
            ("xhdpi", 96),
            ("xxhdpi", 144),
            ("xxxhdpi", 192),
        ] {
            let dir = res.join(format!("mipmap-{density}"));
            self.png(size, false, &dir.join("ic_launcher_foreground.png"))?;
            self.png(size, true, &dir.join("ic_launcher.png"))?;
            self.png(size, true, &dir.join("ic_launcher_round.png"))?;
        }
        fs::write(
            res.join("values/ic_launcher.xml"),
            "<resources><color name=\"ic_launcher_background\">#0b1226</color></resources>",
        )?;
        let xml = "<adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\"><background android:drawable=\"@color/ic_launcher_background\"/><foreground android:drawable=\"@mipmap/ic_launcher_foreground\"/></adaptive-icon>";
        for name in ["ic_launcher", "ic_launcher_round"] {
            fs::write(res.join(format!("mipmap-anydpi-v26/{name}.xml")), xml)?;
        }
        Ok(())
    }

    pub fn ios(&self, root: &Path) -> Result<()> {
        let catalog = root.join("Assets.xcassets/AppIcon.appiconset");
        fs::create_dir_all(&catalog)?;
        let mut images = Vec::new();
        for (idiom, points, scale, pixels) in [
            ("iphone", "60", "2", 120),
            ("iphone", "60", "3", 180),
            ("ipad", "76", "1", 76),
            ("ipad", "76", "2", 152),
            ("ipad", "83.5", "2", 167),
            ("ios-marketing", "1024", "1", 1024),
        ] {
            let name = format!("AppIcon{points}x{points}@{scale}x.png");
            self.png(pixels, true, &root.join(&name))?;
            fs::copy(root.join(&name), catalog.join(&name))?;
            images.push(serde_json::json!({"idiom":idiom,"size":format!("{points}x{points}"),"scale":format!("{scale}x"),"filename":name}));
        }
        fs::write(
            catalog.join("Contents.json"),
            serde_json::to_vec(
                &serde_json::json!({"images":images,"info":{"author":"xcode","version":1}}),
            )?,
        )?;
        Ok(())
    }

    pub fn icns(&self, path: &Path) -> Result<()> {
        let mut chunks = Vec::new();
        for (size, kind) in [
            (16, *b"icp4"),
            (32, *b"icp5"),
            (64, *b"icp6"),
            (128, *b"ic07"),
            (256, *b"ic08"),
            (512, *b"ic09"),
            (1024, *b"ic10"),
        ] {
            let png = self.render(size, true)?.encode_png()?;
            chunks.extend_from_slice(&kind);
            chunks.extend_from_slice(&u32::try_from(png.len() + 8)?.to_be_bytes());
            chunks.extend_from_slice(&png);
        }
        let size = u32::try_from(chunks.len() + 8)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut data = Vec::with_capacity(size as usize);
        data.extend_from_slice(b"icns");
        data.extend_from_slice(&size.to_be_bytes());
        data.extend_from_slice(&chunks);
        fs::write(path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_android_density_resources_and_opaque_apple_icons() {
        let temp = crate::temporary::directory().unwrap();
        let icon = Icon::new().unwrap();
        icon.android(temp.path()).unwrap();
        let density =
            tiny_skia::Pixmap::load_png(temp.path().join("res/mipmap-xxxhdpi/ic_launcher.png"))
                .unwrap();
        assert_eq!((density.width(), density.height()), (192, 192));
        assert!(
            density
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        assert!(
            fs::read_to_string(temp.path().join("res/mipmap-anydpi-v26/ic_launcher.xml"))
                .unwrap()
                .contains("adaptive-icon")
        );
        icon.ios(temp.path()).unwrap();
        let ios = tiny_skia::Pixmap::load_png(temp.path().join("AppIcon60x60@3x.png")).unwrap();
        assert_eq!((ios.width(), ios.height()), (180, 180));
        let catalog: serde_json::Value = serde_json::from_slice(
            &fs::read(
                temp.path()
                    .join("Assets.xcassets/AppIcon.appiconset/Contents.json"),
            )
            .unwrap(),
        )
        .unwrap();
        for image in catalog["images"].as_array().unwrap() {
            assert!(
                temp.path()
                    .join("Assets.xcassets/AppIcon.appiconset")
                    .join(image["filename"].as_str().unwrap())
                    .is_file()
            );
        }
    }

    #[test]
    fn encodes_png_chunks_inside_a_complete_icns_container() {
        let temp = crate::temporary::directory().unwrap();
        let path = temp.path().join("ShinkiEMU.icns");
        Icon::new().unwrap().icns(&path).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len()
        );
        let mut at = 8;
        let mut count = 0;
        while at < bytes.len() {
            let len = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let png = tiny_skia::Pixmap::decode_png(&bytes[at + 8..at + len]).unwrap();
            assert_eq!(png.width(), png.height());
            at += len;
            count += 1;
        }
        assert_eq!(count, 7);
        assert_eq!(at, bytes.len());
    }
}
