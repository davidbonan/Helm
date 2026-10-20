//! Site icons (browser.md §10): one PNG per site (`host:port`), at most 64 px,
//! in the support dir's `favicons/`.

use std::path::{Path, PathBuf};

use super::url;

pub const ICONS_DIR: &str = "favicons";
const ICON_SIZE: u32 = 64;
const FETCH_TIMEOUT_SECS: &str = "5";
const MAX_ICON_BYTES: &str = "1048576";

/// The site a page's icon is kept under; `None` for a page off the web.
pub fn site_of(page_url: &str) -> Option<&str> {
    let site = url::display_parts(page_url).0;
    (url::is_web(page_url) && !site.is_empty() && !site.contains(['/', '\\'])).then_some(site)
}

/// An icon file's bytes, at most 64 px wide; `None` when not an image helm
/// decodes (SVG included).
pub fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let fitted = if image.width().max(image.height()) > ICON_SIZE {
        image.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    Some(fitted.to_rgba8())
}

/// The icon at `icon_url`, through `curl`: nothing on a failure, a timeout or
/// a file over 1 MB.
pub fn download(icon_url: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new("curl")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            FETCH_TIMEOUT_SECS,
            "--max-filesize",
            MAX_ICON_BYTES,
            "--",
            icon_url,
        ])
        .output()
        .ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}

fn icon_path(dir: &Path, site: &str) -> PathBuf {
    dir.join(format!("{site}.png"))
}

pub fn save(dir: &Path, site: &str, icon: &image::RgbaImage) -> image::ImageResult<()> {
    std::fs::create_dir_all(dir)?;
    icon.save_with_format(icon_path(dir, site), image::ImageFormat::Png)
}

/// Every icon kept in `dir`, by site.
pub fn load_saved(dir: &Path) -> Vec<(String, image::RgbaImage)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let site = path.file_name()?.to_str()?.strip_suffix(".png")?.to_owned();
            let icon = image::open(&path).ok()?.to_rgba8();
            Some((site, icon))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(size: u32) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(size, size)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode");
        bytes.into_inner()
    }

    #[test]
    fn an_icon_is_kept_per_host_and_port() {
        assert_eq!(
            site_of("http://localhost:5173/admin"),
            Some("localhost:5173")
        );
        assert_eq!(site_of("https://github.com/helm"), Some("github.com"));
        assert_eq!(site_of("about:blank"), None);
    }

    #[test]
    fn a_big_icon_shrinks_to_64_px_and_a_small_one_keeps_its_size() {
        assert_eq!(
            decode(&png(180)).map(|icon| icon.dimensions()),
            Some((64, 64))
        );
        assert_eq!(
            decode(&png(16)).map(|icon| icon.dimensions()),
            Some((16, 16))
        );
        assert_eq!(decode(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
    }

    #[test]
    fn saved_icons_load_back_under_their_site() {
        let dir = tempfile::tempdir().expect("tempdir");
        let icon = decode(&png(32)).expect("decode");
        save(dir.path(), "localhost:5173", &icon).expect("save");
        let loaded = load_saved(dir.path());
        assert_eq!(loaded, vec![("localhost:5173".to_owned(), icon)]);
    }
}
