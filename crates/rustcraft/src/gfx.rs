//! Everything GPU- and asset-facing for the game: renderers, GUI theme,
//! fonts and item icons.

use std::sync::Arc;

use ahash::AHashMap;
use anyhow::Result;
use rc_assets::model::{JavaRandom, pick};
use rc_assets::{Assets, BlockModels, Dir, Image, Tint};
use rc_core::BlockStateId;
use rc_render::gui::{DrawList, GuiRenderer, TexId};
use rc_render::terrain::TerrainRenderer;
use rc_ui::font::{PendingSheet, fallback_sheet, vanilla_sheets};
use rc_ui::{Font, Sprite, Theme};
use tracing::info;

use crate::items::items;

/// How an item is drawn in slots.
#[derive(Clone, Debug)]
pub enum Icon {
    /// Pre-projected quads in a unit square: (points, uvs, colour).
    Block(Vec<([[f32; 2]; 4], [[f32; 2]; 4], [f32; 4])>),
    Sprite(TexId, [f32; 4], [f32; 4]),
    None,
}

pub struct Gfx {
    pub terrain: TerrainRenderer,
    pub gui: GuiRenderer,
    pub assets: Assets,
    pub models: Arc<BlockModels>,
    pub theme: Theme,
    pub font: Font,
    pub atlas_tex: TexId,
    pub icons: Vec<Icon>,
    pub dl: DrawList,
    pub grass_map: Option<Image>,
    pub foliage_map: Option<Image>,
}

impl std::fmt::Debug for Gfx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gfx({:?})", self.assets.origin)
    }
}

fn read_sprite(
    gui: &mut GuiRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    assets: &Assets,
    path: &str,
) -> Option<Sprite> {
    let bytes = assets
        .stack
        .read(&format!("assets/minecraft/textures/{path}.png"))?;
    let img = Image::decode_png(&bytes)?;
    let border = assets
        .stack
        .read(&format!("assets/minecraft/textures/{path}.png.mcmeta"))
        .and_then(|m| serde_json::from_slice::<serde_json::Value>(&m).ok())
        .and_then(|m| m["gui"]["scaling"]["border"].as_u64())
        .unwrap_or(0) as u32;
    let tex = gui.register(device, queue, &img, false);
    Some(Sprite::new(tex, img.w, img.h, border))
}

fn tint_rgb(t: Tint) -> [f32; 3] {
    let c = match t {
        Tint::None => return [1.0; 3],
        Tint::Grass => 0x91_bd_59,
        Tint::Foliage => 0x77_ab_2f,
        Tint::Water => 0x3f_76_e4,
        Tint::Fixed(c) => c,
    };
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

/// Project a block model into GUI space (vanilla "gui" display: rotate
/// 30° about X and 225° about Y, scale 0.625).
fn block_icon(models: &BlockModels, s: BlockStateId) -> Icon {
    let Some(sm) = models.states.get(s.0 as usize) else {
        return Icon::None;
    };
    let mut rng = JavaRandom::new(0);
    let tint = tint_rgb(models.tints[s.0 as usize]);
    let (ry, rx) = (225f32.to_radians(), 30f32.to_radians());
    let (sy, cy) = ry.sin_cos();
    let (sx, cx) = rx.sin_cos();
    let mut quads: Vec<(f32, ([[f32; 2]; 4], [[f32; 2]; 4], [f32; 4]))> = Vec::new();
    for part in &sm.parts {
        let m = &models.models[pick(part, &mut rng) as usize];
        for q in &m.quads {
            let mut pts = [[0.0f32; 2]; 4];
            let mut depth = 0.0;
            for (k, p) in q.pos.iter().enumerate() {
                let (x, y, z) = (p[0] - 0.5, p[1] - 0.5, p[2] - 0.5);
                // Y rotation then X rotation.
                let (x1, z1) = (x * cy + z * sy, -x * sy + z * cy);
                let (y2, z2) = (y * cx - z1 * sx, y * sx + z1 * cx);
                pts[k] = [0.5 + x1 * 0.625 * 1.6, 0.5 - y2 * 0.625 * 1.6];
                depth += z2;
            }
            let shade = match q.dir {
                Dir::Up => 1.0,
                Dir::Down => 0.4,
                Dir::North | Dir::South => 0.8,
                Dir::West | Dir::East => 0.6,
            };
            let c = if q.tint >= 0 { tint } else { [1.0; 3] };
            quads.push((
                depth,
                (pts, q.uv, [c[0] * shade, c[1] * shade, c[2] * shade, 1.0]),
            ));
        }
    }
    if quads.is_empty() {
        return Icon::None;
    }
    // Farthest first (larger z2 is farther from the viewer after rotation).
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    Icon::Block(quads.into_iter().map(|q| q.1).collect())
}

impl Gfx {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets: Assets,
        opts: &crate::options::Options,
    ) -> Result<Self> {
        let t = std::time::Instant::now();
        let models = Arc::new(assets.block_models(opts.mipmap_levels.clamp(1, 5)));
        info!(elapsed = ?t.elapsed(), models = models.models.len(), "baked block models");
        let sun = assets.png("minecraft:environment/sun");
        let moon = assets.png("minecraft:environment/moon_phases");
        let clouds = assets.png("minecraft:environment/clouds");
        let terrain = TerrainRenderer::new(
            device,
            queue,
            format,
            &models.atlas,
            sun.as_ref(),
            moon.as_ref(),
            clouds.as_ref(),
            opts.anisotropy,
        );
        let mut gui = GuiRenderer::new(device, queue, format);
        // Fonts.
        let mut sheets: Vec<PendingSheet> = if assets.is_vanilla() {
            vanilla_sheets(&assets.stack)
        } else {
            Vec::new()
        };
        sheets.push(fallback_sheet());
        let registered: Vec<(PendingSheet, TexId)> = sheets
            .into_iter()
            .map(|s| {
                let id = gui.register(device, queue, &s.image, false);
                (s, id)
            })
            .collect();
        let font = Font::build(&registered);
        // Theme.
        let mut sp = |p: &str| read_sprite(&mut gui, device, queue, &assets, p);
        let mut theme = Theme {
            button: sp("gui/sprites/widget/button"),
            button_hover: sp("gui/sprites/widget/button_highlighted"),
            button_disabled: sp("gui/sprites/widget/button_disabled"),
            slider: sp("gui/sprites/widget/slider"),
            slider_hover: sp("gui/sprites/widget/slider_highlighted"),
            slider_handle: sp("gui/sprites/widget/slider_handle"),
            slider_handle_hover: sp("gui/sprites/widget/slider_handle_highlighted"),
            text_field: sp("gui/sprites/widget/text_field"),
            text_field_hover: sp("gui/sprites/widget/text_field_highlighted"),
            menu_background: sp("gui/menu_background"),
            menu_list_background: sp("gui/menu_list_background"),
            header_separator: sp("gui/header_separator"),
            footer_separator: sp("gui/footer_separator"),
            panorama: Vec::new(),
            checkbox: sp("gui/sprites/widget/checkbox"),
            checkbox_selected: sp("gui/sprites/widget/checkbox_selected"),
            hotbar: sp("gui/sprites/hud/hotbar"),
            hotbar_selection: sp("gui/sprites/hud/hotbar_selection"),
            crosshair: sp("gui/sprites/hud/crosshair"),
            heart_container: sp("gui/sprites/hud/heart/container"),
            heart_full: sp("gui/sprites/hud/heart/full"),
            heart_half: sp("gui/sprites/hud/heart/half"),
            food_empty: sp("gui/sprites/hud/food_empty"),
            food_full: sp("gui/sprites/hud/food_full"),
            food_half: sp("gui/sprites/hud/food_half"),
            air: sp("gui/sprites/hud/air"),
            air_bursting: sp("gui/sprites/hud/air_bursting"),
            xp_background: sp("gui/sprites/hud/experience_bar_background"),
            xp_progress: sp("gui/sprites/hud/experience_bar_progress"),
            armor_empty: sp("gui/sprites/hud/armor_empty"),
            inventory: sp("gui/container/inventory"),
            creative_items: sp("gui/container/creative_inventory/tab_items"),
            creative_search: sp("gui/container/creative_inventory/tab_item_search"),
            creative_tab_top_selected: (1..=7)
                .filter_map(|i| {
                    sp(&format!(
                        "gui/sprites/container/creative_inventory/tab_top_selected_{i}"
                    ))
                })
                .collect(),
            creative_tab_top_unselected: (1..=7)
                .filter_map(|i| {
                    sp(&format!(
                        "gui/sprites/container/creative_inventory/tab_top_unselected_{i}"
                    ))
                })
                .collect(),
            scroller: sp("gui/sprites/container/creative_inventory/scroller"),
            slot_highlight: None,
        };
        for i in 0..4 {
            if let Some(s) = sp(&format!("gui/title/background/panorama_{i}")) {
                theme.panorama.push(s);
            }
        }
        // Block atlas as a GUI texture for item icons.
        let atlas_view = terrain.atlas_view();
        let a0 = &models.atlas.mips[0];
        let atlas_tex = gui.register_view(device, &atlas_view, (a0.w, a0.h), false);
        // Item icons.
        let it = items();
        let mut icons = vec![Icon::None; it.names.len()];
        let mut item_sprites: AHashMap<String, (TexId, [f32; 4])> = AHashMap::new();
        let mut item_imgs: Vec<(String, Image, Option<rc_assets::atlas::Animation>)> = Vec::new();
        let mut flat: Vec<(usize, String)> = Vec::new();
        for (i, name) in it.names.iter().enumerate() {
            if let Some(tex) = flat_item_texture(&assets, name) {
                flat.push((i, tex));
            }
        }
        for (_, tex) in &flat {
            if !tex.starts_with("minecraft:block/")
                && !item_sprites.contains_key(tex)
                && !item_imgs.iter().any(|x| &x.0 == tex)
                && let Some(img) = assets.png(tex)
            {
                item_imgs.push((tex.clone(), img, None));
            }
        }
        let item_atlas = rc_assets::atlas::build(item_imgs, 1);
        let item_atlas_tex = gui.register(device, queue, &item_atlas.mips[0], false);
        for (name, s) in &item_atlas.sprites {
            item_sprites.insert(name.clone(), (item_atlas_tex, s.uv));
        }
        let blocks = rc_data::blocks();
        for (i, tex) in &flat {
            let (t, uv) = if tex.starts_with("minecraft:block/") {
                (atlas_tex, models.atlas.sprite(tex).uv)
            } else if let Some(&(t, uv)) = item_sprites.get(tex) {
                (t, uv)
            } else {
                continue;
            };
            let tint = it.block[*i].map_or([1.0; 4], |b| {
                let c = tint_rgb(models.tints[b.0 as usize]);
                [c[0], c[1], c[2], 1.0]
            });
            icons[*i] = Icon::Sprite(t, uv, tint);
        }
        for (i, b) in it.block.iter().enumerate() {
            if !matches!(icons[i], Icon::None) {
                continue;
            }
            let Some(b) = *b else { continue };
            // Fallback pack: plants are flat sprites, the rest 3D.
            let layer = models.layers[b.0 as usize];
            let flat_shape = blocks.collision(b).is_empty() && layer == rc_assets::Layer::Cutout;
            icons[i] = if flat_shape && !assets.is_vanilla() {
                Icon::Sprite(atlas_tex, models.particle[b.0 as usize].uv, {
                    let c = tint_rgb(models.tints[b.0 as usize]);
                    [c[0], c[1], c[2], 1.0]
                })
            } else {
                block_icon(&models, b)
            };
        }
        let grass_map = assets.png("minecraft:colormap/grass");
        let foliage_map = assets.png("minecraft:colormap/foliage");
        Ok(Self {
            terrain,
            gui,
            assets,
            models,
            theme,
            font,
            atlas_tex,
            icons,
            dl: DrawList::default(),
            grass_map,
            foliage_map,
        })
    }

    /// Draw an item icon at GUI position (x, y), 16×16 units.
    pub fn draw_item(&self, dl: &mut DrawList, scale: f32, item: u16, x: f32, y: f32, size: f32) {
        match self.icons.get(item as usize) {
            Some(Icon::Block(quads)) => {
                for (p, uv, c) in quads {
                    let pts = p.map(|q| [(x + q[0] * size) * scale, (y + q[1] * size) * scale]);
                    dl.quad_pts(self.atlas_tex, pts, *uv, *c);
                }
            }
            Some(Icon::Sprite(t, uv, c)) => {
                dl.rect_uv(
                    *t,
                    [x * scale, y * scale, (x + size) * scale, (y + size) * scale],
                    *uv,
                    *c,
                );
            }
            _ => {
                dl.fill(
                    [x * scale, y * scale, (x + size) * scale, (y + size) * scale],
                    [0.8, 0.0, 0.8, 1.0],
                );
            }
        }
    }
}

/// `layer0` texture if the item model is a flat (generated) sprite.
fn flat_item_texture(assets: &Assets, name: &str) -> Option<String> {
    if !assets.is_vanilla() {
        return None;
    }
    let mut id = format!("minecraft:item/{name}");
    let mut layer0: Option<String> = None;
    for _ in 0..8 {
        let b = assets.stack.read_id("models", &id, ".json")?;
        let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
        if layer0.is_none() {
            layer0 = v["textures"]["layer0"].as_str().map(|s| {
                if s.contains(':') {
                    s.to_owned()
                } else {
                    format!("minecraft:{s}")
                }
            });
        }
        let parent = v["parent"].as_str()?;
        let p = parent.strip_prefix("minecraft:").unwrap_or(parent);
        if p == "item/generated" || p == "item/handheld" || p == "builtin/generated" {
            return layer0;
        }
        if p.starts_with("block/") || p.starts_with("builtin/") {
            return None;
        }
        id = format!("minecraft:{p}");
    }
    None
}
