use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgba};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ColorRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorRgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_css_rgb(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn hsl(&self) -> (f32, f32, f32) {
        rgb_to_hsl(self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    Vivid,      // High color intensity & brightness -> fast, vivid pulsing
    Mellow,     // Low color intensity or dark -> slow, soft breathing
    Monochrome, // Grayscale or single hue -> single-hue low intensity
    DualTone,   // Two strong distinct hues -> rotating gradient
    Balanced,   // Neutral balanced art
}

impl Default for Mood {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaletteInfo {
    pub primary: ColorRgb,
    pub secondary: ColorRgb,
    pub accent: ColorRgb,
    /// Average perceived brightness (luma) of the art, 0..1.
    pub avg_brightness: f32,
    /// Average colorfulness (chroma = max-min of RGB) of the art, 0..1.
    /// Named "saturation" to keep the existing frontend payload shape.
    pub avg_saturation: f32,
    pub mood: Mood,
}

impl PaletteInfo {
    /// Returns default fallback palette when album art is unavailable (Spotify green accent).
    pub fn fallback() -> Self {
        Self {
            primary: ColorRgb::new(30, 215, 96),   // Spotify Green #1ed760
            secondary: ColorRgb::new(20, 150, 68),
            accent: ColorRgb::new(255, 255, 255),
            avg_brightness: 0.5,
            avg_saturation: 0.7,
            mood: Mood::Balanced,
        }
    }
}

/// Extracts a 5-cluster palette and mood metrics from thumbnail bytes.
pub fn extract_palette(image_bytes: &[u8]) -> PaletteInfo {
    let start_time = Instant::now();

    let img = match image::load_from_memory(image_bytes) {
        Ok(img) => img,
        Err(e) => {
            debug!("[Palette] Failed to decode image bytes: {:?}, using fallback", e);
            return PaletteInfo::fallback();
        }
    };

    let result = extract_palette_from_image(&img);
    let elapsed = start_time.elapsed();
    debug!("[Palette] Extraction completed in {:?}", elapsed);
    result
}

/// Core palette extraction logic on a decoded DynamicImage.
pub fn extract_palette_from_image(img: &DynamicImage) -> PaletteInfo {
    // 1. Downscale to 48x48 for fast k-means. Triangle filtering averages neighbouring
    //    pixels instead of picking noisy single ones (Nearest).
    let target_size = 48;
    let downscaled = img.resize_exact(target_size, target_size, FilterType::Triangle);

    let mut pixels: Vec<[f32; 3]> = Vec::with_capacity((target_size * target_size) as usize);
    let mut total_brightness = 0.0f32; // luma
    let mut total_chroma = 0.0f32; // colorfulness

    for (_, _, Rgba([r, g, b, a])) in downscaled.pixels() {
        // Skip (nearly) transparent pixels
        if a < 32 {
            continue;
        }

        pixels.push([r as f32, g as f32, b as f32]);
        total_brightness += luma01(r, g, b);
        total_chroma += chroma01(r, g, b);
    }

    if pixels.is_empty() {
        return PaletteInfo::fallback();
    }

    let count = pixels.len() as f32;
    let avg_brightness = total_brightness / count;
    let avg_chroma = total_chroma / count;

    // 2. Run k-means (k=5)
    let k = 5;
    let clusters = run_kmeans(&pixels, k, 12);

    // 3. Score & filter clusters
    let mut filtered_clusters: Vec<ScoredCluster> = Vec::new();
    let mut all_clusters: Vec<ScoredCluster> = Vec::new();

    for cluster in &clusters {
        // Empty clusters must never become palette colors
        if cluster.count == 0 {
            continue;
        }

        let r = cluster.centroid[0].round().clamp(0.0, 255.0) as u8;
        let g = cluster.centroid[1].round().clamp(0.0, 255.0) as u8;
        let b = cluster.centroid[2].round().clamp(0.0, 255.0) as u8;

        let color = ColorRgb::new(r, g, b);
        let chroma = chroma01(r, g, b);
        let luma = luma01(r, g, b);
        let (_, _, hsl_light) = rgb_to_hsl(r, g, b);
        let pop_ratio = cluster.count as f32 / count;

        // Rank score: population * (colorfulness + 0.15)
        let score = pop_ratio * (chroma + 0.15);

        let scored = ScoredCluster {
            color,
            chroma,
            population: cluster.count,
            score,
        };

        all_clusters.push(scored.clone());

        // Filter: drop near-black, near-white and washed-out grey clusters
        let is_near_black = luma < 0.10;
        let is_near_white = hsl_light > 0.93;
        let is_low_chroma = chroma < 0.10;

        if !is_near_black && !is_near_white && !is_low_chroma {
            filtered_clusters.push(scored);
        }
    }

    if all_clusters.is_empty() {
        return PaletteInfo::fallback();
    }

    // Sort descending by score / population
    filtered_clusters.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    all_clusters.sort_by(|a, b| b.population.cmp(&a.population));

    // Choose primary, secondary, accent
    let primary: ColorRgb;
    let secondary: ColorRgb;
    let accent: ColorRgb;

    if !filtered_clusters.is_empty() {
        primary = filtered_clusters[0].color;

        // Secondary: the best-ranked cluster that is visibly different from the primary,
        // otherwise a hue-shifted version of the primary.
        secondary = filtered_clusters
            .iter()
            .skip(1)
            .find(|c| color_dist(c.color, primary) > 60.0)
            .map(|c| c.color)
            .unwrap_or_else(|| shift_hue(primary, 40.0));

        // Accent: the most colorful cluster that differs from both primary and secondary,
        // otherwise a lighter version of the primary (accent drives the bright core/comet/progress).
        accent = filtered_clusters
            .iter()
            .filter(|c| color_dist(c.color, primary) > 40.0 && color_dist(c.color, secondary) > 40.0)
            .max_by(|a, b| a.chroma.partial_cmp(&b.chroma).unwrap_or(std::cmp::Ordering::Equal))
            .map(|c| c.color)
            .unwrap_or_else(|| adjust_lightness(primary, 0.2));
    } else {
        // Fallback to dominant clusters from all (e.g. monochrome / black and white images)
        primary = all_clusters[0].color;
        secondary = all_clusters
            .get(1)
            .map(|c| c.color)
            .unwrap_or_else(|| adjust_lightness(primary, 0.2));
        accent = all_clusters
            .get(2)
            .map(|c| c.color)
            .unwrap_or_else(|| adjust_lightness(primary, -0.2));
    }

    // 4. Derive mood
    let mood = derive_mood(avg_chroma, avg_brightness, primary, secondary);

    PaletteInfo {
        primary,
        secondary,
        accent,
        avg_brightness,
        avg_saturation: avg_chroma,
        mood,
    }
}

#[derive(Debug, Clone)]
struct ScoredCluster {
    color: ColorRgb,
    chroma: f32,
    population: usize,
    score: f32,
}

struct Cluster {
    centroid: [f32; 3],
    count: usize,
}

/// Simple, robust k-means implementation for 3D color vectors.
fn run_kmeans(pixels: &[[f32; 3]], k: usize, max_iterations: usize) -> Vec<Cluster> {
    if pixels.is_empty() {
        return Vec::new();
    }

    let k = k.min(pixels.len());

    // Farthest-point initialization: start from the first pixel, then repeatedly pick the
    // pixel farthest from all chosen centroids. This spreads the starting centroids across
    // distinct colors (deterministic, no random seed needed).
    let mut centroids: Vec<[f32; 3]> = Vec::with_capacity(k);
    centroids.push(pixels[0]);
    while centroids.len() < k {
        let next = pixels
            .iter()
            .map(|p| {
                let d = centroids
                    .iter()
                    .map(|c| sq_dist(p, c))
                    .fold(f32::MAX, f32::min);
                (p, d)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(p, _)| *p)
            .unwrap_or(pixels[0]);
        centroids.push(next);
    }

    let mut assignments = vec![0usize; pixels.len()];

    for _ in 0..max_iterations {
        let mut changed = false;

        // Expectation step: assign pixels to closest centroid
        for (i, p) in pixels.iter().enumerate() {
            let mut min_dist = f32::MAX;
            let mut best_cluster = 0;

            for (c_idx, c) in centroids.iter().enumerate() {
                let dist = sq_dist(p, c);
                if dist < min_dist {
                    min_dist = dist;
                    best_cluster = c_idx;
                }
            }

            if assignments[i] != best_cluster {
                assignments[i] = best_cluster;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Maximization step: recompute centroids
        let mut sums = vec![[0.0f32; 3]; k];
        let mut counts = vec![0usize; k];

        for (i, p) in pixels.iter().enumerate() {
            let cluster_idx = assignments[i];
            sums[cluster_idx][0] += p[0];
            sums[cluster_idx][1] += p[1];
            sums[cluster_idx][2] += p[2];
            counts[cluster_idx] += 1;
        }

        for c_idx in 0..k {
            if counts[c_idx] > 0 {
                centroids[c_idx] = [
                    sums[c_idx][0] / counts[c_idx] as f32,
                    sums[c_idx][1] / counts[c_idx] as f32,
                    sums[c_idx][2] / counts[c_idx] as f32,
                ];
            }
        }
    }

    // Final cluster counts
    let mut counts = vec![0usize; k];
    for &cluster_idx in &assignments {
        counts[cluster_idx] += 1;
    }

    centroids
        .into_iter()
        .zip(counts.into_iter())
        .map(|(centroid, count)| Cluster { centroid, count })
        .collect()
}

#[inline]
fn sq_dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    dr * dr + dg * dg + db * db
}

/// Euclidean distance between two colors in RGB space (0..~441).
fn color_dist(a: ColorRgb, b: ColorRgb) -> f32 {
    sq_dist(
        &[a.r as f32, a.g as f32, a.b as f32],
        &[b.r as f32, b.g as f32, b.b as f32],
    )
    .sqrt()
}

/// Perceived brightness (Rec. 601 luma) in 0..1.
#[inline]
fn luma01(r: u8, g: u8, b: u8) -> f32 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// Colorfulness as RGB chroma (max - min) in 0..1.
/// Unlike HSL saturation, very dark pixels don't read as "highly saturated".
#[inline]
fn chroma01(r: u8, g: u8, b: u8) -> f32 {
    let max = r.max(g).max(b) as f32;
    let min = r.min(g).min(b) as f32;
    (max - min) / 255.0
}

/// Convert RGB [0..255] to HSL (Hue [0..360], Saturation [0..1], Lightness [0..1])
pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r_f = r as f32 / 255.0;
    let g_f = g as f32 / 255.0;
    let b_f = b as f32 / 255.0;

    let max = r_f.max(g_f).max(b_f);
    let min = r_f.min(g_f).min(b_f);
    let delta = max - min;

    let l = (max + min) / 2.0;

    if delta == 0.0 {
        return (0.0, 0.0, l);
    }

    let s = if l <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let mut h = if max == r_f {
        ((g_f - b_f) / delta).rem_euclid(6.0)
    } else if max == g_f {
        ((b_f - r_f) / delta) + 2.0
    } else {
        ((r_f - g_f) / delta) + 4.0
    };

    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }

    (h, s, l)
}

/// Convert HSL to RGB [0..255]
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0).round().clamp(0.0, 255.0) as u8;
        return (v, v, v);
    }

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0).rem_euclid(2.0)) - 1.0).abs());
    let m = l - c / 2.0;

    let (r_prime, g_prime, b_prime) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b_prime + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// Adjusts the lightness of a ColorRgb by delta [-1.0, 1.0].
fn adjust_lightness(color: ColorRgb, delta: f32) -> ColorRgb {
    let (h, s, l) = color.hsl();
    let new_l = (l + delta).clamp(0.05, 0.95);
    let (r, g, b) = hsl_to_rgb(h, s, new_l);
    ColorRgb::new(r, g, b)
}

/// Rotates the hue of a color by `degrees`, keeping saturation and lightness.
fn shift_hue(color: ColorRgb, degrees: f32) -> ColorRgb {
    let (h, s, l) = color.hsl();
    let (r, g, b) = hsl_to_rgb((h + degrees).rem_euclid(360.0), s, l);
    ColorRgb::new(r, g, b)
}

/// Derives the mood from average colorfulness (chroma), average brightness (luma),
/// and whether the two lead colors have distinct hues.
fn derive_mood(avg_chroma: f32, avg_brightness: f32, primary: ColorRgb, secondary: ColorRgb) -> Mood {
    // 1. Monochrome: almost no color
    if avg_chroma < 0.08 {
        return Mood::Monochrome;
    }

    // 2. DualTone: primary & secondary have distinct hues and are both strongly colored
    let (h1, _, _) = primary.hsl();
    let (h2, _, _) = secondary.hsl();
    let c1 = chroma01(primary.r, primary.g, primary.b);
    let c2 = chroma01(secondary.r, secondary.g, secondary.b);
    let hue_diff = (h1 - h2).abs();
    let circular_hue_diff = hue_diff.min(360.0 - hue_diff);

    if circular_hue_diff > 45.0 && c1 > 0.35 && c2 > 0.35 {
        return Mood::DualTone;
    }

    // 3. Vivid: colorful and bright
    if avg_chroma > 0.30 && avg_brightness > 0.40 {
        return Mood::Vivid;
    }

    // 4. Mellow: muted or dark
    if avg_chroma < 0.20 || avg_brightness < 0.28 {
        return Mood::Mellow;
    }

    Mood::Balanced
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_vivid_palette_extraction() {
        // Create 100x100 vivid image with neon pink (255, 0, 128) and bright cyan (0, 255, 255)
        let mut img = RgbImage::new(100, 100);
        for (x, _, pixel) in img.enumerate_pixels_mut() {
            if x < 50 {
                *pixel = Rgb([255, 0, 128]);
            } else {
                *pixel = Rgb([0, 255, 255]);
            }
        }

        let dyn_img = DynamicImage::ImageRgb8(img);
        let palette = extract_palette_from_image(&dyn_img);

        assert!(palette.avg_saturation > 0.6);
        assert!(palette.avg_brightness > 0.3);
        assert!(palette.mood == Mood::Vivid || palette.mood == Mood::DualTone);
    }

    #[test]
    fn test_dark_mellow_palette_extraction() {
        // Create 100x100 dark, muted purple image (not pure grey, so it isn't Monochrome)
        let mut img = RgbImage::new(100, 100);
        for (_, _, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgb([60, 20, 100]);
        }

        let dyn_img = DynamicImage::ImageRgb8(img);
        let palette = extract_palette_from_image(&dyn_img);

        assert!(palette.avg_brightness < 0.25);
        assert_eq!(palette.mood, Mood::Mellow);
    }

    #[test]
    fn test_monochrome_palette_extraction() {
        // Create pure grayscale image
        let mut img = RgbImage::new(100, 100);
        for (x, _, pixel) in img.enumerate_pixels_mut() {
            let val = (x * 2).min(255) as u8;
            *pixel = Rgb([val, val, val]);
        }

        let dyn_img = DynamicImage::ImageRgb8(img);
        let palette = extract_palette_from_image(&dyn_img);

        assert!(palette.avg_saturation < 0.1);
        assert_eq!(palette.mood, Mood::Monochrome);
    }

    #[test]
    fn test_dual_tone_palette_extraction() {
        // Create strong dual hues: deep blue (0, 80, 255) and fiery orange (255, 120, 0)
        let mut img = RgbImage::new(100, 100);
        for (x, _, pixel) in img.enumerate_pixels_mut() {
            if x < 50 {
                *pixel = Rgb([0, 80, 255]);
            } else {
                *pixel = Rgb([255, 120, 0]);
            }
        }

        let dyn_img = DynamicImage::ImageRgb8(img);
        let palette = extract_palette_from_image(&dyn_img);

        assert_eq!(palette.mood, Mood::DualTone);
    }

    #[test]
    fn test_secondary_differs_from_primary() {
        // Single-color art must still produce a visibly different secondary (hue shift fallback)
        let mut img = RgbImage::new(100, 100);
        for (_, _, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgb([220, 60, 60]);
        }

        let dyn_img = DynamicImage::ImageRgb8(img);
        let palette = extract_palette_from_image(&dyn_img);

        assert!(color_dist(palette.primary, palette.secondary) > 30.0);
    }

    // Timing target applies to optimized builds; debug builds are far slower.
    // Run with: cargo test --release -- test_performance_under_30ms
    #[test]
    #[cfg_attr(debug_assertions, ignore = "timing target is for release builds")]
    fn test_performance_under_30ms() {
        // Create 300x300 image (standard album art size)
        let mut img = RgbImage::new(300, 300);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgb([((x + y) % 256) as u8, (x % 256) as u8, (y % 256) as u8]);
        }

        let dyn_img = DynamicImage::ImageRgb8(img);

        let start = Instant::now();
        let palette = extract_palette_from_image(&dyn_img);
        let duration = start.elapsed();

        println!("Extraction time for 300x300: {:?}", duration);
        assert!(duration.as_millis() < 30, "Palette extraction took {:?}, target is < 30ms", duration);
        assert!(palette.avg_brightness > 0.0);
    }
}