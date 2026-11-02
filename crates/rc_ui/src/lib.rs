//! Immediate-mode GUI toolkit at vanilla scale: fonts, nine-slice widgets,
//! buttons, sliders, text fields. Screens live in the game crate and are
//! drawn with these widgets every frame.

#![forbid(unsafe_code)]
// Renderer/UI entry points take many plain parameters by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod font;

pub use font::Font;
use rc_render::gui::{DrawList, TexId};

/// A GUI sprite (whole texture) with optional nine-slice border.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sprite {
    pub tex: TexId,
    pub w: f32,
    pub h: f32,
    /// Nine-slice border in texels; 0 = stretch.
    pub border: f32,
    /// Sub-rectangle in UV space (whole texture by default).
    pub uv: [f32; 4],
}

impl Sprite {
    #[must_use]
    pub fn new(tex: TexId, w: u32, h: u32, border: u32) -> Self {
        Self {
            tex,
            w: w as f32,
            h: h as f32,
            border: border as f32,
            uv: [0.0, 0.0, 1.0, 1.0],
        }
    }

    /// A sub-region (in texels) of a larger sheet `sheet_w`×`sheet_h`.
    #[must_use]
    pub fn region(tex: TexId, sheet: (u32, u32), x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            tex,
            w: w as f32,
            h: h as f32,
            border: 0.0,
            uv: [
                x as f32 / sheet.0 as f32,
                y as f32 / sheet.1 as f32,
                (x + w) as f32 / sheet.0 as f32,
                (y + h) as f32 / sheet.1 as f32,
            ],
        }
    }
}

/// Textures used by widgets and the HUD. `None` falls back to flat colours.
#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub button: Option<Sprite>,
    pub button_hover: Option<Sprite>,
    pub button_disabled: Option<Sprite>,
    pub slider: Option<Sprite>,
    pub slider_hover: Option<Sprite>,
    pub slider_handle: Option<Sprite>,
    pub slider_handle_hover: Option<Sprite>,
    pub text_field: Option<Sprite>,
    pub text_field_hover: Option<Sprite>,
    pub menu_background: Option<Sprite>,
    pub menu_list_background: Option<Sprite>,
    pub header_separator: Option<Sprite>,
    pub footer_separator: Option<Sprite>,
    pub panorama: Vec<Sprite>,
    pub checkbox: Option<Sprite>,
    pub checkbox_selected: Option<Sprite>,
    // HUD
    pub hotbar: Option<Sprite>,
    pub hotbar_selection: Option<Sprite>,
    pub crosshair: Option<Sprite>,
    pub heart_container: Option<Sprite>,
    pub heart_full: Option<Sprite>,
    pub heart_half: Option<Sprite>,
    pub food_empty: Option<Sprite>,
    pub food_full: Option<Sprite>,
    pub food_half: Option<Sprite>,
    pub air: Option<Sprite>,
    pub air_bursting: Option<Sprite>,
    pub xp_background: Option<Sprite>,
    pub xp_progress: Option<Sprite>,
    pub armor_empty: Option<Sprite>,
    // Containers
    pub inventory: Option<Sprite>,
    pub creative_items: Option<Sprite>,
    pub creative_search: Option<Sprite>,
    pub creative_tab_top_selected: Vec<Sprite>,
    pub creative_tab_top_unselected: Vec<Sprite>,
    pub scroller: Option<Sprite>,
    pub slot_highlight: Option<Sprite>,
}

/// Keys the UI reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Delete,
    Enter,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Tab,
    PageUp,
    PageDown,
}

/// Per-frame input plus persistent focus.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Mouse in window pixels.
    pub mouse_px: (f32, f32),
    pub pressed: bool,
    pub released: bool,
    pub down: bool,
    pub right_pressed: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub scroll: f32,
    pub typed: String,
    pub keys: Vec<Key>,
    pub focus: Option<u64>,
    pub active: Option<u64>,
    /// Caret position per focused field.
    pub caret: usize,
    pub blink: u32,
    /// Set by widgets: a click sound should play.
    pub clicked_sound: bool,
}

impl Input {
    /// Call after each frame.
    pub fn end_frame(&mut self) {
        self.pressed = false;
        self.released = false;
        self.right_pressed = false;
        self.scroll = 0.0;
        self.typed.clear();
        self.keys.clear();
        if !self.down {
            self.active = None;
        }
        self.blink = self.blink.wrapping_add(1);
    }

    #[must_use]
    pub fn key(&self, k: Key) -> bool {
        self.keys.contains(&k)
    }
}

/// Stable widget id from a label/key.
#[must_use]
pub fn id(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub const GRAY: [f32; 4] = [0.627, 0.627, 0.627, 1.0];
pub const YELLOW: [f32; 4] = [1.0, 1.0, 0.627, 1.0];

/// Compute the vanilla "auto" GUI scale for a window size.
#[must_use]
pub fn auto_scale(w: u32, h: u32, max: u32) -> u32 {
    let mut s = 1;
    while s < max.max(1) && w / (s + 1) >= 320 && h / (s + 1) >= 240 {
        s += 1;
    }
    s
}

/// One frame of UI drawing.
pub struct Ui<'a> {
    pub dl: &'a mut DrawList,
    pub font: &'a Font,
    pub theme: &'a Theme,
    pub input: &'a mut Input,
    /// Pixels per GUI unit.
    pub scale: f32,
    /// Screen size in GUI units.
    pub w: f32,
    pub h: f32,
    /// Game time in ticks (for animations).
    pub time: f32,
}

impl std::fmt::Debug for Ui<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ui({}x{} @{})", self.w, self.h, self.scale)
    }
}

impl Ui<'_> {
    /// Mouse position in GUI units.
    #[must_use]
    pub fn mouse(&self) -> (f32, f32) {
        (
            self.input.mouse_px.0 / self.scale,
            self.input.mouse_px.1 / self.scale,
        )
    }

    #[must_use]
    pub fn hover(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (mx, my) = self.mouse();
        mx >= x && my >= y && mx < x + w && my < y + h
    }

    fn px(&self, r: [f32; 4]) -> [f32; 4] {
        [
            r[0] * self.scale,
            r[1] * self.scale,
            r[2] * self.scale,
            r[3] * self.scale,
        ]
    }

    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        let r = self.px([x, y, x + w, y + h]);
        self.dl.fill(r, c);
    }

    pub fn gradient(&mut self, x: f32, y: f32, w: f32, h: f32, top: [f32; 4], bottom: [f32; 4]) {
        let r = self.px([x, y, x + w, y + h]);
        self.dl.gradient(r, top, bottom);
    }

    pub fn text(&mut self, s: &str, x: f32, y: f32, c: [f32; 4]) {
        self.font.draw(self.dl, s, x, y, self.scale, c, true);
    }

    pub fn text_plain(&mut self, s: &str, x: f32, y: f32, c: [f32; 4]) {
        self.font.draw(self.dl, s, x, y, self.scale, c, false);
    }

    pub fn text_centered(&mut self, s: &str, cx: f32, y: f32, c: [f32; 4]) {
        let w = self.font.width(s);
        self.text(s, (cx - w / 2.0).round(), y, c);
    }

    pub fn text_right(&mut self, s: &str, right: f32, y: f32, c: [f32; 4]) {
        let w = self.font.width(s);
        self.text(s, right - w, y, c);
    }

    /// Draw a sprite stretched (or nine-sliced) to the rectangle.
    pub fn sprite(&mut self, s: &Sprite, x: f32, y: f32, w: f32, h: f32, tint: [f32; 4]) {
        if s.border <= 0.0 || (w == s.w && h == s.h) {
            let r = self.px([x, y, x + w, y + h]);
            self.dl.rect_uv(s.tex, r, s.uv, tint);
            return;
        }
        let b = s.border;
        let [u0, v0, u1, v1] = s.uv;
        let bu = (u1 - u0) * b / s.w;
        let bv = (v1 - v0) * b / s.h;
        let xs = [x, x + b, x + w - b, x + w];
        let ys = [y, y + b, y + h - b, y + h];
        let us = [u0, u0 + bu, u1 - bu, u1];
        let vs = [v0, v0 + bv, v1 - bv, v1];
        for j in 0..3 {
            for i in 0..3 {
                if xs[i + 1] <= xs[i] || ys[j + 1] <= ys[j] {
                    continue;
                }
                let r = self.px([xs[i], ys[j], xs[i + 1], ys[j + 1]]);
                self.dl
                    .rect_uv(s.tex, r, [us[i], vs[j], us[i + 1], vs[j + 1]], tint);
            }
        }
    }

    /// Tile a sprite across a rectangle (menu backgrounds).
    pub fn tiled(&mut self, s: &Sprite, x: f32, y: f32, w: f32, h: f32, tint: [f32; 4]) {
        let r = self.px([x, y, x + w, y + h]);
        self.dl.rect_uv(
            s.tex,
            r,
            [x / s.w, y / s.h, (x + w) / s.w, (y + h) / s.h],
            tint,
        );
    }

    /// Options-style background: panorama on the title, dimmed world in game.
    pub fn background(&mut self, in_world: bool) {
        let (w, h) = (self.w, self.h);
        if in_world {
            self.gradient(
                0.0,
                0.0,
                w,
                h,
                [0.06, 0.06, 0.06, 0.75],
                [0.06, 0.06, 0.06, 0.82],
            );
            return;
        }
        if let Some(p) = self.theme.panorama.first().copied() {
            // Slow horizontal pan across the first panorama face pair.
            let t = (self.time * 0.0004).rem_euclid(1.0);
            let scale = h / p.h;
            let pw = p.w * scale;
            let x0 = -t * pw;
            let n = self.theme.panorama.len().min(4);
            let mut x = x0;
            let mut i = 0;
            while x < w {
                let face = self.theme.panorama[i % n];
                self.sprite(&face, x, 0.0, pw + 0.5, h, WHITE);
                x += pw;
                i += 1;
            }
            self.gradient(0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.25], [0.0, 0.0, 0.0, 0.45]);
        } else {
            self.gradient(
                0.0,
                0.0,
                w,
                h,
                [0.11, 0.16, 0.24, 1.0],
                [0.05, 0.06, 0.09, 1.0],
            );
        }
    }

    /// Darkened list area used by option screens.
    pub fn list_background(&mut self, y0: f32, y1: f32, in_world: bool) {
        let w = self.w;
        match (self.theme.menu_list_background, in_world) {
            (Some(bg), false) => self.tiled(&bg, 0.0, y0, w, y1 - y0, [1.0, 1.0, 1.0, 1.0]),
            _ => self.fill(0.0, y0, w, y1 - y0, [0.0, 0.0, 0.0, 0.35]),
        }
        if let Some(s) = self.theme.header_separator {
            self.tiled(&s, 0.0, y0 - 2.0, w, 2.0, WHITE);
        } else {
            self.fill(0.0, y0 - 1.0, w, 1.0, [0.0, 0.0, 0.0, 0.6]);
        }
        if let Some(s) = self.theme.footer_separator {
            self.tiled(&s, 0.0, y1, w, 2.0, WHITE);
        } else {
            self.fill(0.0, y1, w, 1.0, [1.0, 1.0, 1.0, 0.25]);
        }
    }

    fn frame(&mut self, kind: Option<Sprite>, x: f32, y: f32, w: f32, h: f32, fallback: [f32; 4]) {
        match kind {
            Some(s) => self.sprite(&s, x, y, w, h, WHITE),
            None => {
                self.fill(x, y, w, h, [0.0, 0.0, 0.0, 1.0]);
                self.fill(x + 1.0, y + 1.0, w - 2.0, h - 2.0, fallback);
            }
        }
    }

    /// Vanilla-style button. Returns true when clicked.
    pub fn button(&mut self, label: &str, x: f32, y: f32, w: f32, h: f32, enabled: bool) -> bool {
        let hover = enabled && self.hover(x, y, w, h);
        let (sprite, fb) = if !enabled {
            (self.theme.button_disabled, [0.17, 0.17, 0.17, 1.0])
        } else if hover {
            (self.theme.button_hover, [0.45, 0.47, 0.62, 1.0])
        } else {
            (self.theme.button, [0.43, 0.43, 0.43, 1.0])
        };
        self.frame(sprite, x, y, w, h, fb);
        let c = if !enabled {
            GRAY
        } else if hover {
            YELLOW
        } else {
            WHITE
        };
        let max = w - 8.0;
        let mut text = label.to_owned();
        while self.font.width(&text) > max && text.chars().count() > 1 {
            text.pop();
        }
        self.text_centered(&text, x + w / 2.0, y + (h - 8.0) / 2.0, c);
        let clicked = hover && self.input.pressed;
        if clicked {
            self.input.pressed = false;
            self.input.clicked_sound = true;
        }
        clicked
    }

    /// Slider with the label drawn on it; `value` in 0..=1.
    pub fn slider(
        &mut self,
        key: &str,
        label: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        value: &mut f32,
    ) -> bool {
        let wid = id(key);
        let hover = self.hover(x, y, w, h);
        if hover && self.input.pressed {
            self.input.active = Some(wid);
            self.input.pressed = false;
        }
        let mut changed = false;
        if self.input.active == Some(wid) && self.input.down {
            let (mx, _) = self.mouse();
            let v = ((mx - x - 4.0) / (w - 8.0)).clamp(0.0, 1.0);
            if (v - *value).abs() > f32::EPSILON {
                *value = v;
                changed = true;
            }
        }
        if self.input.active == Some(wid) && self.input.released {
            self.input.clicked_sound = true;
        }
        let track = if hover {
            self.theme.slider_hover.or(self.theme.slider)
        } else {
            self.theme.slider
        };
        self.frame(track, x, y, w, h, [0.0, 0.0, 0.0, 1.0]);
        let hx = x + (*value * (w - 8.0)).round();
        let handle = if hover || self.input.active == Some(wid) {
            self.theme.slider_handle_hover.or(self.theme.slider_handle)
        } else {
            self.theme.slider_handle
        };
        self.frame(handle, hx, y, 8.0, h, [0.6, 0.6, 0.6, 1.0]);
        let c = if hover { YELLOW } else { WHITE };
        self.text_centered(label, x + w / 2.0, y + (h - 8.0) / 2.0, c);
        changed
    }

    /// Single-line text field. Returns true on Enter.
    pub fn text_field(
        &mut self,
        key: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        value: &mut String,
        max_len: usize,
        hint: &str,
    ) -> bool {
        let wid = id(key);
        let hover = self.hover(x, y, w, h);
        if self.input.pressed {
            if hover {
                self.input.focus = Some(wid);
                self.input.caret = value.chars().count();
                self.input.pressed = false;
            } else if self.input.focus == Some(wid) {
                self.input.focus = None;
            }
        }
        let focused = self.input.focus == Some(wid);
        let mut enter = false;
        if focused {
            let mut chars: Vec<char> = value.chars().collect();
            let mut caret = self.input.caret.min(chars.len());
            for c in self.input.typed.chars() {
                if !c.is_control() && chars.len() < max_len {
                    chars.insert(caret, c);
                    caret += 1;
                }
            }
            for k in self.input.keys.clone() {
                match k {
                    Key::Backspace if caret > 0 => {
                        caret -= 1;
                        chars.remove(caret);
                    }
                    Key::Delete if caret < chars.len() => {
                        chars.remove(caret);
                    }
                    Key::Left => caret = caret.saturating_sub(1),
                    Key::Right => caret = (caret + 1).min(chars.len()),
                    Key::Home => caret = 0,
                    Key::End => caret = chars.len(),
                    Key::Enter => enter = true,
                    _ => {}
                }
            }
            *value = chars.into_iter().collect();
            self.input.caret = caret;
        }
        let frame = if focused || hover {
            self.theme.text_field_hover.or(self.theme.text_field)
        } else {
            self.theme.text_field
        };
        match frame {
            Some(s) => self.sprite(&s, x, y, w, h, WHITE),
            None => {
                self.fill(x, y, w, h, if focused { WHITE } else { GRAY });
                self.fill(x + 1.0, y + 1.0, w - 2.0, h - 2.0, [0.0, 0.0, 0.0, 1.0]);
            }
        }
        let ty = y + (h - 8.0) / 2.0;
        if value.is_empty() && !focused {
            self.text(hint, x + 4.0, ty, [0.5, 0.5, 0.5, 1.0]);
        } else {
            // Scroll so the caret stays visible.
            let caret = self.input.caret.min(value.chars().count());
            let before: String = value.chars().take(caret).collect();
            let mut start = 0;
            while self
                .font
                .width(&before.chars().skip(start).collect::<String>())
                > w - 10.0
            {
                start += 1;
            }
            let visible: String = value.chars().skip(start).collect();
            let mut shown = visible.clone();
            while self.font.width(&shown) > w - 8.0 && !shown.is_empty() {
                shown.pop();
            }
            self.text(&shown, x + 4.0, ty, [0.88, 0.88, 0.88, 1.0]);
            if focused && (self.input.blink / 20).is_multiple_of(2) {
                let cx = x
                    + 4.0
                    + self
                        .font
                        .width(&before.chars().skip(start).collect::<String>())
                    + 1.0;
                self.fill(cx, ty - 1.0, 1.0, 10.0, [0.82, 0.82, 0.82, 1.0]);
            }
        }
        enter
    }

    /// A toggle shown as a button "Label: ON/OFF".
    pub fn toggle(&mut self, label: &str, x: f32, y: f32, w: f32, v: &mut bool) -> bool {
        let text = format!("{label}: {}", if *v { "ON" } else { "OFF" });
        if self.button(&text, x, y, w, 20.0, true) {
            *v = !*v;
            return true;
        }
        false
    }

    /// Tooltip box near the mouse.
    pub fn tooltip(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        let (mx, my) = self.mouse();
        let w = lines.iter().map(|l| self.font.width(l)).fold(0.0, f32::max);
        let h = lines.len() as f32 * 10.0 - 2.0;
        let mut x = mx + 12.0;
        let mut y = my - 12.0;
        if x + w + 4.0 > self.w {
            x = mx - 16.0 - w;
        }
        y = y.clamp(4.0, (self.h - h - 4.0).max(4.0));
        let bg = [0.063, 0.0, 0.063, 0.94];
        self.fill(x - 3.0, y - 4.0, w + 6.0, h + 8.0, bg);
        self.fill(x - 4.0, y - 3.0, w + 8.0, h + 6.0, bg);
        self.gradient(
            x - 3.0,
            y - 3.0,
            w + 6.0,
            h + 6.0,
            [0.31, 0.0, 1.0, 0.31],
            [0.16, 0.0, 0.5, 0.31],
        );
        self.fill(x - 2.0, y - 2.0, w + 4.0, h + 4.0, bg);
        for (i, l) in lines.iter().enumerate() {
            let c = if i == 0 { WHITE } else { GRAY };
            self.text(l, x, y + i as f32 * 10.0 + if i > 0 { 2.0 } else { 0.0 }, c);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auto_scale_matches_vanilla_rule() {
        assert_eq!(super::auto_scale(1280, 720, 99), 3);
        assert_eq!(super::auto_scale(1920, 1080, 99), 4);
        assert_eq!(super::auto_scale(854, 480, 99), 2);
        assert_eq!(super::auto_scale(1920, 1080, 2), 2);
    }

    #[test]
    fn fallback_font_widths() {
        let sheet = super::font::fallback_sheet();
        let f = super::Font::build(&[(sheet, rc_render::gui::TexId(1))]);
        assert!(f.width("Hello") > 20.0);
        assert_eq!(f.width(" "), 3.0);
    }
}
