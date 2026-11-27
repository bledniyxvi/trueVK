use eframe::egui;
use image::{GenericImageView, RgbaImage};
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

pub fn compute_dominant_color_from_rgba(rgba: &RgbaImage) -> Option<[u8; 3]> {
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut r_sum: u64 = 0;
    let mut g_sum: u64 = 0;
    let mut b_sum: u64 = 0;
    let mut weight_sum: u64 = 0;
    let mut count: u32 = 0;
    let step = 4usize;
    for y in (0..height as usize).step_by(step) {
        for x in (0..width as usize).step_by(step) {
            let [r, g, b, a] = rgba.get_pixel(x as u32, y as u32).0;
            if a < 128 {
                continue;
            }
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let chroma = max - min;
            if max < 28 || chroma < 16 {
                continue;
            }
            let w = chroma as u64;
            r_sum += r as u64 * w;
            g_sum += g as u64 * w;
            b_sum += b as u64 * w;
            weight_sum += w;
            count += 1;
        }
    }
    if count < 4 || weight_sum == 0 {
        return None;
    }
    let mut r = (r_sum / weight_sum) as f32;
    let mut g = (g_sum / weight_sum) as f32;
    let mut b = (b_sum / weight_sum) as f32;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max <= 1.0 {
        return None;
    }
    let sat = (max - min) / max;
    if sat < 0.45 && max > min {
        let factor = 0.55 / sat;
        r = (max - (max - r) * factor).clamp(0.0, 255.0);
        g = (max - (max - g) * factor).clamp(0.0, 255.0);
        b = (max - (max - b) * factor).clamp(0.0, 255.0);
    }
    let max2 = r.max(g).max(b);
    if max2 > 1.0 {
        let scale = max2.clamp(130.0, 220.0) / max2;
        r = (r * scale).clamp(0.0, 255.0);
        g = (g * scale).clamp(0.0, 255.0);
        b = (b * scale).clamp(0.0, 255.0);
    }
    Some([r.round() as u8, g.round() as u8, b.round() as u8])
}

pub fn load_cover(client: &reqwest::blocking::Client, url: &str) -> Option<(egui::ColorImage, Option<[u8; 3]>)> {
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
    let color = compute_dominant_color_from_rgba(&rgba);
    Some((
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba.as_raw()),
        color,
    ))
}

pub enum CoverSlot {
    Loading,
    Ready(egui::TextureHandle, Option<[u8; 3]>),
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
        if matches!(previous, Some(CoverSlot::Ready(_, _))) {
            self.ready_count = self.ready_count.saturating_sub(1);
        }
    }

    pub fn insert_ready(&mut self, url: String, texture: egui::TextureHandle, dominant_color: Option<[u8; 3]>) {
        let previous = self.slots.insert(url.clone(), CoverSlot::Ready(texture, dominant_color));
        match previous {
            Some(CoverSlot::Ready(_, _)) => {}
            _ => {
                self.ready_count += 1;
                self.order.push_back(url);
            }
        }
        self.evict();
    }

    pub fn texture(&mut self, url: &str) -> Option<egui::TextureHandle> {
        let texture = match self.slots.get(url) {
            Some(CoverSlot::Ready(texture, _)) => Some(texture.clone()),
            _ => None,
        };
        if texture.is_some() {
            self.touch(url);
        }
        texture
    }

    pub fn dominant_color(&self, url: &str) -> Option<[u8; 3]> {
        match self.slots.get(url) {
            Some(CoverSlot::Ready(_, color)) => *color,
            _ => None,
        }
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
            if matches!(self.slots.get(&url), Some(CoverSlot::Ready(_, _))) {
                self.slots.remove(&url);
                self.ready_count = self.ready_count.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn dominant_color_for_saturated_image() {
        let rgba = RgbaImage::from_pixel(8, 8, Rgba([100, 150, 200, 255]));
        let color = compute_dominant_color_from_rgba(&rgba).expect("saturated image should have a dominant color");
        assert!((color[0] as i32 - 100).abs() <= 10);
        assert!((color[1] as i32 - 150).abs() <= 10);
        assert!((color[2] as i32 - 200).abs() <= 10);
    }

    #[test]
    fn dominant_color_none_for_black_and_gray() {
        assert!(compute_dominant_color_from_rgba(&RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))).is_none());
        assert!(compute_dominant_color_from_rgba(&RgbaImage::from_pixel(8, 8, Rgba([128, 128, 128, 255]))).is_none());
        assert!(compute_dominant_color_from_rgba(&RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]))).is_none());
    }

    #[test]
    fn dominant_color_for_pastel_image() {
        let color = compute_dominant_color_from_rgba(&RgbaImage::from_pixel(8, 8, Rgba([210, 245, 252, 255])))
            .expect("pastel image should have a dominant color");
        assert!(color[2] > color[0]);
    }
}
