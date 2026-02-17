use eframe::egui;
use image::{DynamicImage, RgbaImage};
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
    Text,
    Select,
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

    fn draw_annotations(&self, painter: &egui::Painter, canvas_rect: egui::Rect) {
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
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let rect = egui::Rect::from_two_pos(s_min, s_max);
                    let c = color.to_egui();
                    let t = thickness * self.zoom;
                    painter.rect_stroke(rect, 0.0, egui::Stroke::new(t, c), egui::StrokeKind::Middle);
                    if is_selected {
                        self.draw_selection_indicator(painter, rect);
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
                    ..
                } => {
                    let s_min =
                        self.image_to_screen(canvas_rect, egui::pos2(min.0, min.1));
                    let s_max =
                        self.image_to_screen(canvas_rect, egui::pos2(max.0, max.1));
                    let rect = egui::Rect::from_two_pos(s_min, s_max);
                    let expanded = rect.expand(thickness * self.zoom + 8.0);
                    let shrunk = rect.shrink(thickness * self.zoom + 8.0);
                    expanded.contains(screen_pos) && !shrunk.contains(screen_pos)
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
                AnnotationKind::Rectangle { min, max, .. } => {
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
                } => {
                    let c = [
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ];
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
                ui.selectable_value(&mut self.tool, Tool::Text, "Text");
                ui.selectable_value(&mut self.tool, Tool::Select, "Select");
                ui.separator();
                ui.label("Color:");
                ui.color_edit_button_rgb(&mut self.color);
                ui.separator();
                ui.label("Thickness:");
                ui.add(egui::Slider::new(&mut self.thickness, 1.0..=20.0));
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
            self.draw_annotations(&painter, canvas_rect);

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
                            painter.rect_stroke(
                                rect,
                                0.0,
                                egui::Stroke::new(t, c),
                                egui::StrokeKind::Middle,
                            );
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
                            Tool::Arrow | Tool::Rectangle => {
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
    if args.len() < 2 {
        eprintln!("Usage: annotate-edit <image.png|jpg>");
        std::process::exit(1);
    }

    let image_path = PathBuf::from(&args[1]);
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
