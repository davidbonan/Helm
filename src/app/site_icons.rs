//! Site icons of the browser panes (browser.md §10): the saved ones loaded at
//! launch, each site's fetched once a launch, all decoded off the UI thread and
//! kept as textures.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossbeam_channel::{Receiver, Sender};

use crate::browser::favicons;

type SiteIcon = (String, image::RgbaImage);

pub(crate) struct SiteIcons {
    /// `None` off the app (tests): nothing is read or written.
    dir: Option<PathBuf>,
    textures: HashMap<String, egui::TextureHandle>,
    /// Sites already fetched this launch.
    fetched: HashSet<String>,
    sender: Sender<SiteIcon>,
    receiver: Receiver<SiteIcon>,
}

impl Default for SiteIcons {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            dir: None,
            textures: HashMap::new(),
            fetched: HashSet::new(),
            sender,
            receiver,
        }
    }
}

impl SiteIcons {
    /// The support dir's icons, coming in from a background read.
    pub(crate) fn load(ctx: &egui::Context) -> Self {
        let icons = Self {
            dir: crate::persistence::support_file(favicons::ICONS_DIR),
            ..Self::default()
        };
        if let Some(dir) = icons.dir.clone() {
            icons.in_background(ctx, move || favicons::load_saved(&dir));
        }
        icons
    }

    pub(crate) fn textures(&self) -> &HashMap<String, egui::TextureHandle> {
        &self.textures
    }

    /// Fetches the icon `page_url` names, once a launch for its site.
    pub(crate) fn fetch(&mut self, page_url: &str, icon_url: &str, ctx: &egui::Context) {
        let (Some(dir), Some(site)) = (self.dir.clone(), favicons::site_of(page_url)) else {
            return;
        };
        if !self.fetched.insert(site.to_owned()) {
            return;
        }
        let (site, icon_url) = (site.to_owned(), icon_url.to_owned());
        self.in_background(ctx, move || {
            favicons::download(&icon_url)
                .and_then(|bytes| keep(&dir, site, &bytes))
                .into_iter()
                .collect()
        });
    }

    /// Icons an import brought, as image files by site.
    pub(crate) fn take_in(&self, icons: Vec<(String, Vec<u8>)>, ctx: &egui::Context) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        self.in_background(ctx, move || {
            icons
                .into_iter()
                .filter_map(|(site, bytes)| keep(&dir, site, &bytes))
                .collect()
        });
    }

    /// The icons decoded since the last frame, made textures.
    pub(crate) fn receive(&mut self, ctx: &egui::Context) {
        for (site, icon) in self.receiver.try_iter() {
            let size = [icon.width() as usize, icon.height() as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, icon.as_raw());
            let texture = ctx.load_texture(
                format!("site-icon-{site}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.textures.insert(site, texture);
        }
    }

    fn in_background(
        &self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Vec<SiteIcon> + Send + 'static,
    ) {
        let (sender, ctx) = (self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            for icon in job() {
                let _ = sender.send(icon);
            }
            ctx.request_repaint();
        });
    }
}

/// Decodes and saves an icon file; the icon, when it is one.
fn keep(dir: &std::path::Path, site: String, bytes: &[u8]) -> Option<SiteIcon> {
    let icon = favicons::decode(bytes)?;
    if let Err(err) = favicons::save(dir, &site, &icon) {
        eprintln!("helm: cannot save the icon of {site}: {err}");
    }
    Some((site, icon))
}
