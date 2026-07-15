use eframe::egui;
use image::{imageops, DynamicImage, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Data Model ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Color4 {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Color4 {
    fn to_egui(&self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8,
            (self.a * 255.0) as u8,
        )
    }

    #[allow(dead_code)]
    fn from_egui(c: egui::Color32) -> Self {
        Self {
            r: c.r() as f32 / 255.0,
            g: c.g() as f32 / 255.0,
            b: c.b() as f32 / 255.0,
            a: c.a() as f32 / 255.0,
        }
    }
}

impl Default for Color4 {
    fn default() -> Self {
        Self {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
enum Fill {
    #[default]
    None,
    Color(Color4),
    Blur(f32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AnnotationKind {
    Arrow {
        start: (f32, f32),
        end: (f32, f32),
        color: Color4,
        thickness: f32,
    },
    Rectangle {
        min: (f32, f32),
        max: (f32, f32),
        color: Color4,
        thickness: f32,
        #[serde(default)]
        fill: Fill,
    },
    Oval {
        min: (f32, f32),
        max: (f32, f32),
        color: Color4,
        thickness: f32,
        #[serde(default)]
        fill: Fill,
    },
    Text {
        pos: (f32, f32),
        content: String,
        font_size: f32,
        color: Color4,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Annotation {
    kind: AnnotationKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AnnotationFile {
    annotations: Vec<Annotation>,
}

fn annotz_path(image_path: &Path) -> PathBuf {
    image_path.with_extension(format!(
        "{}.annotz",
        image_path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("")
    ))
}

fn load_annotations(image_path: &Path) -> Vec<Annotation> {
    let path = annotz_path(image_path);
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(file) = serde_json::from_str::<AnnotationFile>(&data) {
                return file.annotations;
            }
        }
    }
    Vec::new()
}

fn save_annotations(image_path: &Path, annotations: &[Annotation]) {
    let path = annotz_path(image_path);
    let file = AnnotationFile {
        annotations: annotations.to_vec(),
    };
    if let Ok(data) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(&path, data);
    }
}

// ── Tool / Interaction State ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Arrow,
    Rectangle,
    Oval,
    Text,
    Select,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FillMode {
    None,
    Color,
    Blur,
}

#[derive(Clone, Debug)]
enum DragState {
    None,
    Drawing { start: egui::Pos2 },
    Moving { index: usize },
}

// ── App ─────────────────────────────────────────────────────────────────────

struct AnnotateApp {
    image_path: PathBuf,
    texture: Option<egui::TextureHandle>,
    image_size: (f32, f32),
    raw_image: Option<DynamicImage>,

    annotations: Vec<Annotation>,
    undo_stack: Vec<Vec<Annotation>>,
    redo_stack: Vec<Vec<Annotation>>,

    tool: Tool,
    color: [f32; 3],
    thickness: f32,
    font_size: f32,
    fill_mode: FillMode,
    fill_color: [f32; 3],
    blur_sigma: f32,

    drag: DragState,
    selected: Option<usize>,

    // text input state
    text_input_pos: Option<(f32, f32)>,
    text_input_buf: String,

    // pan & zoom
    pan: egui::Vec2,
    zoom: f32,
    panning: bool,
}

impl AnnotateApp {
    fn new(image_path: PathBuf) -> Self {
        let annotations = load_annotations(&image_path);
        let raw_image = image::open(&image_path).ok();
        let image_size = raw_image
            .as_ref()
            .map(|img| (img.width() as f32, img.height() as f32))
            .unwrap_or((800.0, 600.0));

        Self {
            image_path,
            texture: None,
            image_size,
            raw_image,
            annotations,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tool: Tool::Arrow,
            color: [1.0, 0.0, 0.0],
            thickness: 3.0,
            font_size: 20.0,
            fill_mode: FillMode::None,
            fill_color: [1.0, 1.0, 0.0],
            blur_sigma: 8.0,
            drag: DragState::None,
            selected: None,
            text_input_pos: None,
            text_input_buf: String::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            panning: false,
        }
    }

    fn current_color4(&self) -> Color4 {
        Color4 {
            r: self.color[0],
            g: self.color[1],
            b: self.color[2],
            a: 1.0,
        }
    }

    fn current_fill(&self) -> Fill {
        match self.fill_mode {
            FillMode::None => Fill::None,
            FillMode::Color => Fill::Color(Color4 {
                r: self.fill_color[0],
                g: self.fill_color[1],
                b: self.fill_color[2],
                a: 1.0,
            }),
            FillMode::Blur => Fill::Blur(self.blur_sigma),
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.annotations.clone());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.annotations.clone());
            self.annotations = prev;
            self.auto_save();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.annotations.clone());
            self.annotations = next;
            self.auto_save();
        }
    }

    fn auto_save(&self) {
        save_annotations(&self.image_path, &self.annotations);
    }

    /// Convert image-space coords to screen-space
    fn image_to_screen(&self, canvas_rect: egui::Rect, img_pos: egui::Pos2) -> egui::Pos2 {
        let center = canvas_rect.center();
        center
            + self.pan
            + (img_pos.to_vec2() - egui::vec2(self.image_size.0, self.image_size.1) * 0.5)
                * self.zoom
    }

    /// Convert screen-space coords to image-space
    fn screen_to_image(&self, canvas_rect: egui::Rect, screen_pos: egui::Pos2) -> egui::Pos2 {
        let center = canvas_rect.center();
        let rel = screen_pos - center - self.pan;
        egui::pos2(
            rel.x / self.zoom + self.image_size.0 * 0.5,
            rel.y / self.zoom + self.image_size.1 * 0.5,
        )
    }

    fn image_rect_on_screen(&self, canvas_rect: egui::Rect) -> egui::Rect {
        let top_left = self.image_to_screen(canvas_rect, egui::Pos2::ZERO);
        let bot_right = self.image_to_screen(
            canvas_rect,
            egui::pos2(self.image_size.0, self.image_size.1),
        );
        egui::Rect::from_min_max(top_left, bot_right)
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_some() {
            return;
        }
        if let Some(ref img) = self.raw_image {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.as_flat_samples();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
            self.texture = Some(ctx.load_texture(
                "image",
                color_image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    /// Draws a live-blurred patch of the source image within the given
    /// image-space bounds. The blur is computed from the original image only
    /// (not from other annotations drawn on top), same as text annotations,
    /// this is a GUI-only approximation of the exported result.
    fn draw_blur_fill(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        canvas_rect: egui::Rect,
        img_bounds: ((f32, f32), (f32, f32)),
        sigma: f32,
        oval: bool,
    ) {
        let Some(ref raw) = self.raw_image else {
            return;
        };
        let (min, max) = img_bounds;
        let Some((ox, oy, patch)) = blurred_patch(raw, min.0, min.1, max.0, max.1, sigma, oval)
        else {
            return;
        };
        let size = [patch.width() as usize, patch.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, patch.as_flat_samples().as_slice());
        let tex = ctx.load_texture("blur_patch", color_image, egui::TextureOptions::LINEAR);
        let img_min = egui::pos2(ox as f32, oy as f32);
        let img_max = img_min + egui::vec2(patch.width() as f32, patch.height() as f32);
        let s_min = self.image_to_screen(canvas_rect, img_min);
        let s_max = self.image_to_screen(canvas_rect, img_max);
        painter.image(
            tex.id(),
            egui::Rect::from_two_pos(s_min, s_max),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    fn draw_annotations(&self, ctx: &egui::Context, painter: &egui::Painter, canvas_rect: egui::Rect) {
        for (i, ann) in self.annotations.iter().enumerate() {
            let is_selected = self.selected == Some(i);
            match &ann.kind {
                AnnotationKind::Arrow {
                    start,
                    end,
                    color,
                    thickness,
                } => {
                    let s =
                        self.image_to_screen(canvas_rect, egui::pos2(start.0, start.1));
                    let e = self.image_to_screen(canvas_rect, egui::pos2(end.0, end.1));
                    let c = color.to_egui();
                    let t = thickness * self.zoom;
                    painter.line_segment([s, e], egui::Stroke::new(t, c));
                    // arrowhead
                    let dir = (e - s).normalized();
                    let head_len = (t * 4.0).max(10.0);
                    let perp = egui::vec2(-dir.y, dir.x);
                    let p1 = e - dir * head_len + perp * head_len * 0.4;
                    let p2 = e - dir * head_len - perp * head_len * 0.4;
                    painter.add(egui::Shape::convex_polygon(
                        vec![e, p1, p2],
                        c,
                        egui::Stroke::NONE,
                    ));
                    if is_selected {
                        self.draw_selection_indicator(
                            painter,
                            egui::Rect::from_two_pos(s, e),
                        );
                    }
                }
                AnnotationKind::Rectangle {
                    min,
                    max,
                    color,
                    thickness,
                    fill,
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let rect = egui::Rect::from_two_pos(s_min, s_max);
                    let c = color.to_egui();
                    let t = thickness * self.zoom;
                    match fill {
                        Fill::Blur(sigma) => {
                            self.draw_blur_fill(ctx, painter, canvas_rect, (*min, *max), *sigma, false);
                            painter.rect_stroke(rect, 0.0, egui::Stroke::new(t, c), egui::StrokeKind::Middle);
                        }
                        _ => {
                            let fill_c = match fill {
                                Fill::Color(fc) => fc.to_egui(),
                                _ => egui::Color32::TRANSPARENT,
                            };
                            painter.rect(rect, 0.0, fill_c, egui::Stroke::new(t, c), egui::StrokeKind::Middle);
                        }
                    }
                    if is_selected {
                        self.draw_selection_indicator(painter, rect);
                    }
                }
                AnnotationKind::Oval {
                    min,
                    max,
                    color,
                    thickness,
                    fill,
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let center = egui::pos2((s_min.x + s_max.x) * 0.5, (s_min.y + s_max.y) * 0.5);
                    let radii = egui::vec2((s_max.x - s_min.x).abs() * 0.5, (s_max.y - s_min.y).abs() * 0.5);
                    let c = color.to_egui();
                    let t = thickness * self.zoom;
                    match fill {
                        Fill::Blur(sigma) => {
                            self.draw_blur_fill(ctx, painter, canvas_rect, (*min, *max), *sigma, true);
                            painter.add(egui::epaint::EllipseShape { center, radius: radii, fill: egui::Color32::TRANSPARENT, stroke: egui::Stroke::new(t, c) });
                        }
                        _ => {
                            let fill_c = match fill {
                                Fill::Color(fc) => fc.to_egui(),
                                _ => egui::Color32::TRANSPARENT,
                            };
                            painter.add(egui::epaint::EllipseShape { center, radius: radii, fill: fill_c, stroke: egui::Stroke::new(t, c) });
                        }
                    }
                    let bounding = egui::Rect::from_two_pos(s_min, s_max);
                    if is_selected {
                        self.draw_selection_indicator(painter, bounding);
                    }
                }
                AnnotationKind::Text {
                    pos,
                    content,
                    font_size,
                    color,
                } => {
                    let s = self.image_to_screen(canvas_rect, egui::pos2(pos.0, pos.1));
                    let c = color.to_egui();
                    let fs = font_size * self.zoom;
                    let galley = painter.layout_no_wrap(
                        content.clone(),
                        egui::FontId::proportional(fs),
                        c,
                    );
                    let text_rect = egui::Rect::from_min_size(s, galley.size());
                    painter.galley(s, galley, c);
                    if is_selected {
                        self.draw_selection_indicator(painter, text_rect);
                    }
                }
            }
        }
    }

    fn draw_selection_indicator(&self, painter: &egui::Painter, rect: egui::Rect) {
        let expanded = rect.expand(4.0);
        painter.rect_stroke(
            expanded,
            2.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 120, 255)),
            egui::StrokeKind::Middle,
        );
    }

    fn hit_test(
        &self,
        canvas_rect: egui::Rect,
        screen_pos: egui::Pos2,
    ) -> Option<usize> {
        for (i, ann) in self.annotations.iter().enumerate().rev() {
            let hit = match &ann.kind {
                AnnotationKind::Arrow {
                    start,
                    end,
                    thickness,
                    ..
                } => {
                    let s =
                        self.image_to_screen(canvas_rect, egui::pos2(start.0, start.1));
                    let e =
                        self.image_to_screen(canvas_rect, egui::pos2(end.0, end.1));
                    point_to_segment_dist(screen_pos, s, e)
                        < (thickness * self.zoom + 8.0)
                }
                AnnotationKind::Rectangle {
                    min,
                    max,
                    thickness,
                    fill,
                    ..
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let rect = egui::Rect::from_two_pos(s_min, s_max);
                    if !matches!(fill, Fill::None) {
                        rect.expand(thickness * self.zoom + 4.0).contains(screen_pos)
                    } else {
                        let expanded = rect.expand(thickness * self.zoom + 8.0);
                        let shrunk = rect.shrink(thickness * self.zoom + 8.0);
                        expanded.contains(screen_pos) && !shrunk.contains(screen_pos)
                    }
                }
                AnnotationKind::Oval {
                    min,
                    max,
                    thickness,
                    fill,
                    ..
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let cx = (s_min.x + s_max.x) * 0.5;
                    let cy = (s_min.y + s_max.y) * 0.5;
                    let slack = thickness * self.zoom + 8.0;
                    let rx = (s_max.x - s_min.x).abs() * 0.5;
                    let ry = (s_max.y - s_min.y).abs() * 0.5;
                    let dx = screen_pos.x - cx;
                    let dy = screen_pos.y - cy;
                    if !matches!(fill, Fill::None) {
                        let rx = (rx + slack).max(1.0);
                        let ry = (ry + slack).max(1.0);
                        (dx / rx).powi(2) + (dy / ry).powi(2) <= 1.0
                    } else {
                        let outer_rx = (rx + slack).max(1.0);
                        let outer_ry = (ry + slack).max(1.0);
                        let inner_rx = (rx - slack).max(0.001);
                        let inner_ry = (ry - slack).max(0.001);
                        let outside_inner = (dx / inner_rx).powi(2) + (dy / inner_ry).powi(2) >= 1.0;
                        let inside_outer = (dx / outer_rx).powi(2) + (dy / outer_ry).powi(2) <= 1.0;
                        outside_inner && inside_outer
                    }
                }
                AnnotationKind::Text {
                    pos,
                    content,
                    font_size,
                    ..
                } => {
                    let s =
                        self.image_to_screen(canvas_rect, egui::pos2(pos.0, pos.1));
                    let fs = font_size * self.zoom;
                    let approx_width = content.len() as f32 * fs * 0.6;
                    let rect = egui::Rect::from_min_size(
                        s,
                        egui::vec2(approx_width, fs * 1.2),
                    );
                    rect.expand(4.0).contains(screen_pos)
                }
            };
            if hit {
                return Some(i);
            }
        }
        None
    }

    fn move_annotation(&mut self, index: usize, delta_img: egui::Vec2) {
        if let Some(ann) = self.annotations.get_mut(index) {
            match &mut ann.kind {
                AnnotationKind::Arrow { start, end, .. } => {
                    start.0 += delta_img.x;
                    start.1 += delta_img.y;
                    end.0 += delta_img.x;
                    end.1 += delta_img.y;
                }
                AnnotationKind::Rectangle { min, max, .. }
                | AnnotationKind::Oval { min, max, .. } => {
                    min.0 += delta_img.x;
                    min.1 += delta_img.y;
                    max.0 += delta_img.x;
                    max.1 += delta_img.y;
                }
                AnnotationKind::Text { pos, .. } => {
                    pos.0 += delta_img.x;
                    pos.1 += delta_img.y;
                }
            }
        }
    }

    fn export_annotated(&self) {
        let Some(ref raw) = self.raw_image else {
            return;
        };
        let mut img: RgbaImage = raw.to_rgba8();

        for ann in &self.annotations {
            match &ann.kind {
                AnnotationKind::Arrow {
                    start,
                    end,
                    color,
                    thickness,
                } => {
                    let c = [
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ];
                    draw_line_on_image(
                        &mut img, start.0, start.1, end.0, end.1, *thickness, c,
                    );
                    let dx = end.0 - start.0;
                    let dy = end.1 - start.1;
                    let len = (dx * dx + dy * dy).sqrt();
                    if len > 0.0 {
                        let dir = (dx / len, dy / len);
                        let perp = (-dir.1, dir.0);
                        let head_len = (thickness * 4.0).max(10.0);
                        let p1 = (
                            end.0 - dir.0 * head_len + perp.0 * head_len * 0.4,
                            end.1 - dir.1 * head_len + perp.1 * head_len * 0.4,
                        );
                        let p2 = (
                            end.0 - dir.0 * head_len - perp.0 * head_len * 0.4,
                            end.1 - dir.1 * head_len - perp.1 * head_len * 0.4,
                        );
                        draw_line_on_image(
                            &mut img, end.0, end.1, p1.0, p1.1, *thickness, c,
                        );
                        draw_line_on_image(
                            &mut img, end.0, end.1, p2.0, p2.1, *thickness, c,
                        );
                        draw_line_on_image(
                            &mut img, p1.0, p1.1, p2.0, p2.1, *thickness, c,
                        );
                    }
                }
                AnnotationKind::Rectangle {
                    min,
                    max,
                    color,
                    thickness,
                    fill,
                } => {
                    let c = [
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ];
                    match fill {
                        Fill::Color(fc) => {
                            let fc = [
                                (fc.r * 255.0) as u8,
                                (fc.g * 255.0) as u8,
                                (fc.b * 255.0) as u8,
                                (fc.a * 255.0) as u8,
                            ];
                            fill_rect_on_image(&mut img, min.0, min.1, max.0, max.1, fc);
                        }
                        Fill::Blur(sigma) => {
                            if let Some((ox, oy, patch)) =
                                blurred_patch(&img, min.0, min.1, max.0, max.1, *sigma, false)
                            {
                                imageops::overlay(&mut img, &patch, ox as i64, oy as i64);
                            }
                        }
                        Fill::None => {}
                    }
                    draw_line_on_image(
                        &mut img, min.0, min.1, max.0, min.1, *thickness, c,
                    );
                    draw_line_on_image(
                        &mut img, max.0, min.1, max.0, max.1, *thickness, c,
                    );
                    draw_line_on_image(
                        &mut img, max.0, max.1, min.0, max.1, *thickness, c,
                    );
                    draw_line_on_image(
                        &mut img, min.0, max.1, min.0, min.1, *thickness, c,
                    );
                }
                AnnotationKind::Oval {
                    min,
                    max,
                    color,
                    thickness,
                    fill,
                } => {
                    let c = [
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ];
                    let cx = (min.0 + max.0) * 0.5;
                    let cy = (min.1 + max.1) * 0.5;
                    let rx = (max.0 - min.0).abs() * 0.5;
                    let ry = (max.1 - min.1).abs() * 0.5;
                    match fill {
                        Fill::Color(fc) => {
                            let fc = [
                                (fc.r * 255.0) as u8,
                                (fc.g * 255.0) as u8,
                                (fc.b * 255.0) as u8,
                                (fc.a * 255.0) as u8,
                            ];
                            fill_oval_on_image(&mut img, cx, cy, rx, ry, fc);
                        }
                        Fill::Blur(sigma) => {
                            if let Some((ox, oy, patch)) =
                                blurred_patch(&img, min.0, min.1, max.0, max.1, *sigma, true)
                            {
                                imageops::overlay(&mut img, &patch, ox as i64, oy as i64);
                            }
                        }
                        Fill::None => {}
                    }
                    draw_oval_on_image(&mut img, cx, cy, rx, ry, *thickness, c);
                }
                AnnotationKind::Text { .. } => {
                    // Text rendering to image requires a font rasterizer;
                    // text annotations only appear in the GUI for now.
                }
            }
        }

        let out_path = self.image_path.with_file_name(format!(
            "{}_annotated.png",
            self.image_path
                .file_stem()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("out")
        ));
        let _ = img.save(&out_path);
        eprintln!("Exported to {}", out_path.display());
    }
}

fn point_to_segment_dist(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let t = ap.dot(ab) / ab.dot(ab);
    let t = t.clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).length()
}

fn draw_line_on_image(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    color: [u8; 4],
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    let steps = (len * 2.0) as i32;
    let half_t = (thickness / 2.0).max(0.5) as i32;
    let (w, h) = (img.width() as i32, img.height() as i32);

    for i in 0..=steps {
        let t = i as f32 / steps.max(1) as f32;
        let cx = (x0 + dx * t) as i32;
        let cy = (y0 + dy * t) as i32;
        for oy in -half_t..=half_t {
            for ox in -half_t..=half_t {
                let px = cx + ox;
                let py = cy + oy;
                if px >= 0 && px < w && py >= 0 && py < h {
                    img.put_pixel(px as u32, py as u32, image::Rgba(color));
                }
            }
        }
    }
}

fn draw_oval_on_image(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    thickness: f32,
    color: [u8; 4],
) {
    let steps = ((rx.max(ry) * std::f32::consts::TAU) as usize).max(64);
    let half_t = (thickness / 2.0).max(0.5) as i32;
    let (w, h) = (img.width() as i32, img.height() as i32);
    for i in 0..steps {
        let angle = i as f32 / steps as f32 * std::f32::consts::TAU;
        let px = (cx + rx * angle.cos()) as i32;
        let py = (cy + ry * angle.sin()) as i32;
        for oy in -half_t..=half_t {
            for ox in -half_t..=half_t {
                let x = px + ox;
                let y = py + oy;
                if x >= 0 && x < w && y >= 0 && y < h {
                    img.put_pixel(x as u32, y as u32, image::Rgba(color));
                }
            }
        }
    }
}

fn fill_oval_on_image(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    color: [u8; 4],
) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let x0 = ((cx - rx) as i32).max(0);
    let x1 = ((cx + rx) as i32 + 1).min(w);
    let y0 = ((cy - ry) as i32).max(0);
    let y1 = ((cy + ry) as i32 + 1).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = (x as f32 - cx) / rx;
            let dy = (y as f32 - cy) / ry;
            if dx * dx + dy * dy <= 1.0 {
                img.put_pixel(x as u32, y as u32, image::Rgba(color));
            }
        }
    }
}

/// Crops `base` to the given image-space bounds (clamped to the image),
/// applies a gaussian blur with the given sigma, and, if `oval` is set,
/// zeroes the alpha of pixels outside the inscribed ellipse so the caller
/// can composite the patch back with alpha blending. Returns the patch
/// together with its top-left origin in image space.
fn blurred_patch<V>(
    base: &V,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    sigma: f32,
    oval: bool,
) -> Option<(u32, u32, RgbaImage)>
where
    V: image::GenericImageView<Pixel = image::Rgba<u8>> + 'static,
{
    let (w, h) = (base.width() as i32, base.height() as i32);
    let lx = (x0.min(x1) as i32).max(0);
    let rx = (x0.max(x1) as i32 + 1).min(w);
    let ty = (y0.min(y1) as i32).max(0);
    let by = (y0.max(y1) as i32 + 1).min(h);
    if rx <= lx || by <= ty {
        return None;
    }
    let (lx, ty, rx, by) = (lx as u32, ty as u32, rx as u32, by as u32);
    let cropped = imageops::crop_imm(base, lx, ty, rx - lx, by - ty).to_image();
    let mut blurred = imageops::blur(&cropped, sigma.max(0.01));
    if oval {
        let cx = (rx - lx) as f32 * 0.5;
        let cy = (by - ty) as f32 * 0.5;
        let rrx = cx.max(1.0);
        let rry = cy.max(1.0);
        for (px, py, pixel) in blurred.enumerate_pixels_mut() {
            let dx = (px as f32 + 0.5 - cx) / rrx;
            let dy = (py as f32 + 0.5 - cy) / rry;
            if dx * dx + dy * dy > 1.0 {
                pixel[3] = 0;
            }
        }
    }
    Some((lx, ty, blurred))
}

fn fill_rect_on_image(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [u8; 4],
) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let lx = (x0.min(x1) as i32).max(0);
    let rx = (x0.max(x1) as i32 + 1).min(w);
    let ty = (y0.min(y1) as i32).max(0);
    let by = (y0.max(y1) as i32 + 1).min(h);
    for y in ty..by {
        for x in lx..rx {
            img.put_pixel(x as u32, y as u32, image::Rgba(color));
        }
    }
}

// ── eframe App impl ────────────────────────────────────────────────────────

impl eframe::App for AnnotateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_texture(ctx);

        // Keyboard shortcuts
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Z) {
                if i.modifiers.shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::S) {
                self.auto_save();
                self.export_annotated();
            }
            if i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace) {
                if self.text_input_pos.is_none() {
                    if let Some(idx) = self.selected {
                        if idx < self.annotations.len() {
                            self.push_undo();
                            self.annotations.remove(idx);
                            self.selected = None;
                            self.auto_save();
                        }
                    }
                }
            }
        });

        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tool, Tool::Arrow, "Arrow");
                ui.selectable_value(&mut self.tool, Tool::Rectangle, "Rectangle");
                ui.selectable_value(&mut self.tool, Tool::Oval, "Oval");
                ui.selectable_value(&mut self.tool, Tool::Text, "Text");
                ui.selectable_value(&mut self.tool, Tool::Select, "Select");
                ui.separator();
                ui.label("Color:");
                ui.color_edit_button_rgb(&mut self.color);
                ui.separator();
                ui.label("Thickness:");
                ui.add(egui::Slider::new(&mut self.thickness, 1.0..=20.0));
                // If a Rectangle/Oval annotation is currently selected, the fill
                // controls edit that annotation directly instead of just setting
                // the defaults for the next shape drawn.
                let selected_fillable = if self.tool == Tool::Select {
                    self.selected.filter(|&i| {
                        matches!(
                            self.annotations.get(i).map(|a| &a.kind),
                            Some(AnnotationKind::Rectangle { .. }) | Some(AnnotationKind::Oval { .. })
                        )
                    })
                } else {
                    None
                };

                if matches!(self.tool, Tool::Rectangle | Tool::Oval) || selected_fillable.is_some() {
                    ui.separator();
                    ui.label("Fill:");
                    if let Some(idx) = selected_fillable {
                        let current_fill = match &self.annotations[idx].kind {
                            AnnotationKind::Rectangle { fill, .. }
                            | AnnotationKind::Oval { fill, .. } => fill.clone(),
                            _ => unreachable!(),
                        };
                        let mut mode = match current_fill {
                            Fill::None => FillMode::None,
                            Fill::Color(_) => FillMode::Color,
                            Fill::Blur(_) => FillMode::Blur,
                        };
                        let mut color = match &current_fill {
                            Fill::Color(c) => [c.r, c.g, c.b],
                            _ => self.fill_color,
                        };
                        let mut sigma = match current_fill {
                            Fill::Blur(s) => s,
                            _ => self.blur_sigma,
                        };

                        let mut should_push_undo = false;
                        let mut changed = false;
                        if ui.selectable_value(&mut mode, FillMode::None, "None").clicked() {
                            should_push_undo = true;
                            changed = true;
                        }
                        if ui.selectable_value(&mut mode, FillMode::Color, "Color").clicked() {
                            should_push_undo = true;
                            changed = true;
                        }
                        if ui.selectable_value(&mut mode, FillMode::Blur, "Blur").clicked() {
                            should_push_undo = true;
                            changed = true;
                        }
                        match mode {
                            FillMode::Color => {
                                let resp = ui.color_edit_button_rgb(&mut color);
                                should_push_undo |= resp.drag_started();
                                changed |= resp.changed();
                            }
                            FillMode::Blur => {
                                ui.label("Amount:");
                                let resp = ui.add(egui::Slider::new(&mut sigma, 1.0..=40.0));
                                should_push_undo |= resp.drag_started();
                                changed |= resp.changed();
                            }
                            FillMode::None => {}
                        }

                        if changed {
                            let new_fill = match mode {
                                FillMode::None => Fill::None,
                                FillMode::Color => Fill::Color(Color4 {
                                    r: color[0],
                                    g: color[1],
                                    b: color[2],
                                    a: 1.0,
                                }),
                                FillMode::Blur => Fill::Blur(sigma),
                            };
                            if should_push_undo {
                                self.push_undo();
                            }
                            if let Some(ann) = self.annotations.get_mut(idx) {
                                match &mut ann.kind {
                                    AnnotationKind::Rectangle { fill, .. }
                                    | AnnotationKind::Oval { fill, .. } => *fill = new_fill,
                                    _ => {}
                                }
                            }
                            self.auto_save();
                            self.fill_mode = mode;
                            self.fill_color = color;
                            self.blur_sigma = sigma;
                        }
                    } else {
                        ui.selectable_value(&mut self.fill_mode, FillMode::None, "None");
                        ui.selectable_value(&mut self.fill_mode, FillMode::Color, "Color");
                        ui.selectable_value(&mut self.fill_mode, FillMode::Blur, "Blur");
                        match self.fill_mode {
                            FillMode::Color => {
                                ui.color_edit_button_rgb(&mut self.fill_color);
                            }
                            FillMode::Blur => {
                                ui.label("Amount:");
                                ui.add(egui::Slider::new(&mut self.blur_sigma, 1.0..=40.0));
                            }
                            FillMode::None => {}
                        }
                    }
                }
                if self.tool == Tool::Text {
                    ui.separator();
                    ui.label("Font:");
                    ui.add(egui::Slider::new(&mut self.font_size, 8.0..=72.0));
                }
                ui.separator();
                if ui.button("Undo").clicked() {
                    self.undo();
                }
                if ui.button("Redo").clicked() {
                    self.redo();
                }
                ui.separator();
                ui.label(format!("Zoom: {:.0}%", self.zoom * 100.0));
            });
        });

        // Canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(
                ui.available_size(),
                egui::Sense::click_and_drag(),
            );
            let canvas_rect = response.rect;

            // Draw background
            painter.rect_filled(canvas_rect, 0.0, egui::Color32::from_gray(40));

            // Draw image
            if let Some(ref tex) = self.texture {
                let img_rect = self.image_rect_on_screen(canvas_rect);
                painter.image(
                    tex.id(),
                    img_rect,
                    egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    ),
                    egui::Color32::WHITE,
                );
            }

            // Draw annotations
            self.draw_annotations(ctx, &painter, canvas_rect);

            // Draw in-progress annotation preview
            if let DragState::Drawing { start } = self.drag {
                if let Some(current) = response.hover_pos() {
                    let c = self.current_color4().to_egui();
                    let t = self.thickness * self.zoom;
                    match self.tool {
                        Tool::Arrow => {
                            painter.line_segment(
                                [start, current],
                                egui::Stroke::new(t, c),
                            );
                            let dir = (current - start).normalized();
                            let head_len = (t * 4.0).max(10.0);
                            let perp = egui::vec2(-dir.y, dir.x);
                            let p1 =
                                current - dir * head_len + perp * head_len * 0.4;
                            let p2 =
                                current - dir * head_len - perp * head_len * 0.4;
                            painter.add(egui::Shape::convex_polygon(
                                vec![current, p1, p2],
                                c,
                                egui::Stroke::NONE,
                            ));
                        }
                        Tool::Rectangle => {
                            let rect = egui::Rect::from_two_pos(start, current);
                            if self.fill_mode == FillMode::Blur {
                                let img_start = self.screen_to_image(canvas_rect, start);
                                let img_end = self.screen_to_image(canvas_rect, current);
                                self.draw_blur_fill(
                                    ctx,
                                    &painter,
                                    canvas_rect,
                                    ((img_start.x, img_start.y), (img_end.x, img_end.y)),
                                    self.blur_sigma,
                                    false,
                                );
                                painter.rect_stroke(rect, 0.0, egui::Stroke::new(t, c), egui::StrokeKind::Middle);
                            } else {
                                let fill = if self.fill_mode == FillMode::Color {
                                    egui::Color32::from_rgba_unmultiplied(
                                        (self.fill_color[0] * 255.0) as u8,
                                        (self.fill_color[1] * 255.0) as u8,
                                        (self.fill_color[2] * 255.0) as u8,
                                        255,
                                    )
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                painter.rect(rect, 0.0, fill, egui::Stroke::new(t, c), egui::StrokeKind::Middle);
                            }
                        }
                        Tool::Oval => {
                            let rect = egui::Rect::from_two_pos(start, current);
                            let center = rect.center();
                            let radii = rect.size() * 0.5;
                            if self.fill_mode == FillMode::Blur {
                                let img_start = self.screen_to_image(canvas_rect, start);
                                let img_end = self.screen_to_image(canvas_rect, current);
                                self.draw_blur_fill(
                                    ctx,
                                    &painter,
                                    canvas_rect,
                                    ((img_start.x, img_start.y), (img_end.x, img_end.y)),
                                    self.blur_sigma,
                                    true,
                                );
                                painter.add(egui::epaint::EllipseShape { center, radius: radii, fill: egui::Color32::TRANSPARENT, stroke: egui::Stroke::new(t, c) });
                            } else {
                                let fill = if self.fill_mode == FillMode::Color {
                                    egui::Color32::from_rgba_unmultiplied(
                                        (self.fill_color[0] * 255.0) as u8,
                                        (self.fill_color[1] * 255.0) as u8,
                                        (self.fill_color[2] * 255.0) as u8,
                                        255,
                                    )
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                painter.add(egui::epaint::EllipseShape { center, radius: radii, fill, stroke: egui::Stroke::new(t, c) });
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Text input overlay
            if let Some(img_pos) = self.text_input_pos {
                let screen_pos = self.image_to_screen(
                    canvas_rect,
                    egui::pos2(img_pos.0, img_pos.1),
                );
                let text_area = egui::Area::new(egui::Id::new("text_input"))
                    .fixed_pos(screen_pos)
                    .order(egui::Order::Foreground);
                text_area.show(ctx, |ui| {
                    ui.set_max_width(300.0);
                    let te = ui.text_edit_singleline(&mut self.text_input_buf);
                    if te.lost_focus() {
                        if !self.text_input_buf.is_empty() {
                            self.push_undo();
                            self.annotations.push(Annotation {
                                kind: AnnotationKind::Text {
                                    pos: img_pos,
                                    content: self.text_input_buf.clone(),
                                    font_size: self.font_size,
                                    color: self.current_color4(),
                                },
                            });
                            self.auto_save();
                        }
                        self.text_input_buf.clear();
                        self.text_input_pos = None;
                    } else {
                        te.request_focus();
                    }
                });
            }

            // Handle pan (middle mouse button)
            let middle_down = ctx.input(|i| i.pointer.middle_down());
            if middle_down {
                let delta = ctx.input(|i| i.pointer.delta());
                self.pan += delta;
                self.panning = true;
            } else {
                self.panning = false;
            }

            // Handle zoom (scroll wheel)
            let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll_delta != 0.0 && response.hovered() {
                let zoom_factor = 1.0 + scroll_delta * 0.002;
                let new_zoom = (self.zoom * zoom_factor).clamp(0.1, 10.0);
                if let Some(cursor) = response.hover_pos() {
                    let center = canvas_rect.center();
                    let cursor_rel = cursor - center - self.pan;
                    self.pan -= cursor_rel * (new_zoom / self.zoom - 1.0);
                }
                self.zoom = new_zoom;
            }

            // Handle tool interactions (primary button only, not while panning)
            if !self.panning {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = response.hover_pos() {
                        match self.tool {
                            Tool::Arrow | Tool::Rectangle | Tool::Oval => {
                                self.drag = DragState::Drawing { start: pos };
                            }
                            Tool::Text => {
                                let img_pos =
                                    self.screen_to_image(canvas_rect, pos);
                                self.text_input_pos =
                                    Some((img_pos.x, img_pos.y));
                                self.text_input_buf.clear();
                            }
                            Tool::Select => {
                                if let Some(idx) =
                                    self.hit_test(canvas_rect, pos)
                                {
                                    self.selected = Some(idx);
                                    self.push_undo();
                                    self.drag = DragState::Moving {
                                        index: idx,
                                    };
                                } else {
                                    self.selected = None;
                                }
                            }
                        }
                    }
                }

                if response.dragged_by(egui::PointerButton::Primary) {
                    if let DragState::Moving { index, .. } = &self.drag {
                        let delta_screen = response.drag_delta();
                        let delta_img = delta_screen / self.zoom;
                        self.move_annotation(*index, delta_img);
                    }
                }

                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    match self.drag.clone() {
                        DragState::Drawing { start } => {
                            if let Some(end) = response
                                .hover_pos()
                                .or(ctx.input(|i| i.pointer.latest_pos()))
                            {
                                let img_start =
                                    self.screen_to_image(canvas_rect, start);
                                let img_end =
                                    self.screen_to_image(canvas_rect, end);

                                if (end - start).length() > 5.0 {
                                    self.push_undo();
                                    let ann = match self.tool {
                                        Tool::Arrow => Annotation {
                                            kind: AnnotationKind::Arrow {
                                                start: (
                                                    img_start.x,
                                                    img_start.y,
                                                ),
                                                end: (img_end.x, img_end.y),
                                                color: self.current_color4(),
                                                thickness: self.thickness,
                                            },
                                        },
                                        Tool::Rectangle => Annotation {
                                            kind: AnnotationKind::Rectangle {
                                                min: (
                                                    img_start.x,
                                                    img_start.y,
                                                ),
                                                max: (img_end.x, img_end.y),
                                                color: self.current_color4(),
                                                thickness: self.thickness,
                                                fill: self.current_fill(),
                                            },
                                        },
                                        Tool::Oval => Annotation {
                                            kind: AnnotationKind::Oval {
                                                min: (
                                                    img_start.x,
                                                    img_start.y,
                                                ),
                                                max: (img_end.x, img_end.y),
                                                color: self.current_color4(),
                                                thickness: self.thickness,
                                                fill: self.current_fill(),
                                            },
                                        },
                                        _ => unreachable!(),
                                    };
                                    self.annotations.push(ann);
                                    self.auto_save();
                                }
                            }
                        }
                        DragState::Moving { .. } => {
                            self.auto_save();
                        }
                        DragState::None => {}
                    }
                    self.drag = DragState::None;
                }
            }
        });
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("describe") => {
            println!(
                "{}",
                r#"{
  "slug": "annotate_edit",
  "description": "Open an image file for interactive annotation with arrows, rectangles, ovals, and text. Annotations are saved as a JSON sidecar file and exported as an annotated PNG.",
  "args": [
    {
      "name": "path",
      "description": "Path to the image file to annotate (PNG or JPEG)",
      "type": "string",
      "backing_type": "string",
      "arity": "single",
      "mode": "dashdashspace"
    }
  ]
}"#
            );
            return;
        }
        Some("run") => {}
        _ => {
            eprintln!("Usage: annotate-edit <describe|run --path <image>>");
            std::process::exit(1);
        }
    }

    // Parse --path <value> from the remaining args after "run"
    let run_args = &args[2..];
    let path_value = run_args
        .windows(2)
        .find(|w| w[0] == "--path")
        .map(|w| &w[1]);

    let Some(path_str) = path_value else {
        eprintln!("Usage: annotate-edit run --path <image>");
        std::process::exit(1);
    };

    let image_path = PathBuf::from(path_str);
    if !image_path.exists() {
        eprintln!("File not found: {}", image_path.display());
        std::process::exit(1);
    }

    let title = format!(
        "annotate-edit — {}",
        image_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("")
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title(&title),
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(move |_cc| Ok(Box::new(AnnotateApp::new(image_path)))),
    )
    .expect("Failed to run eframe");
}
