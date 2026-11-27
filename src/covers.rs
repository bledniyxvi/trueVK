use eframe::egui;
use image::GenericImageView;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

const COVER_MAX_DIM: u32 = 512;
const COVER_TIMEOUT_SECS: u64 = 10;

pub fn build_client(user_agent: &str) -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(COVER_TIMEOUT_SECS))
        .user_agent(user_agent)
        .build()
        .ok()
}

pub fn load_cover(client: &reqwest::blocking::Client, url: &str) -> Option<egui::ColorImage> {
    let response = client.get(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 || width > COVER_MAX_DIM || height > COVER_MAX_DIM {
        return None;
    }
    let rgba = decoded.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba.as_raw(),
    ))
}

pub enum CoverSlot {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

pub struct CoverCache {
    slots: HashMap<String, CoverSlot>,
    order: VecDeque<String>,
    max_ready: usize,
    ready_count: usize,
}

impl CoverCache {
    pub fn new(max_ready: usize) -> Self {
        Self {
            slots: HashMap::new(),
            order: VecDeque::new(),
            max_ready: max_ready.max(1),
            ready_count: 0,
        }
    }

    pub fn needs_request(&self, url: &str) -> bool {
        !self.slots.contains_key(url)
    }

    pub fn mark_loading(&mut self, url: &str) {
        if !self.slots.contains_key(url) {
            self.slots.insert(url.to_string(), CoverSlot::Loading);
        }
    }

    pub fn mark_failed(&mut self, url: &str) {
        let previous = self.slots.insert(url.to_string(), CoverSlot::Failed);
        if matches!(previous, Some(CoverSlot::Ready(_))) {
            self.ready_count = self.ready_count.saturating_sub(1);
        }
    }

    pub fn insert_ready(&mut self, url: String, texture: egui::TextureHandle) {
        let previous = self.slots.insert(url.clone(), CoverSlot::Ready(texture));
        match previous {
            Some(CoverSlot::Ready(_)) => {}
            _ => {
                self.ready_count += 1;
                self.order.push_back(url);
            }
        }
        self.evict();
    }

    pub fn texture(&mut self, url: &str) -> Option<egui::TextureHandle> {
        let texture = match self.slots.get(url) {
            Some(CoverSlot::Ready(texture)) => Some(texture.clone()),
            _ => None,
        };
        if texture.is_some() {
            self.touch(url);
        }
        texture
    }

    fn touch(&mut self, url: &str) {
        if let Some(pos) = self.order.iter().position(|item| item == url) {
            if let Some(item) = self.order.remove(pos) {
                self.order.push_back(item);
            }
        }
    }

    fn evict(&mut self) {
        while self.ready_count > self.max_ready {
            let url = match self.order.pop_front() {
                Some(url) => url,
                None => break,
            };
            if matches!(self.slots.get(&url), Some(CoverSlot::Ready(_))) {
                self.slots.remove(&url);
                self.ready_count = self.ready_count.saturating_sub(1);
            }
        }
    }
}
