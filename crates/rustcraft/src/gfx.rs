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

/// How an item is drawn in the world (dropped, held).
#[derive(Clone, Copy, Debug)]
pub enum WorldIcon {
    Block(BlockStateId),
    /// Batch texture index, UV rectangle, tint.
    Flat(usize, [f32; 4], [f32; 4]),
    None,
}

/// Batch texture indices registered with the terrain renderer.
#[derive(Clone, Copy, Debug, Default)]
pub struct EntityTextures {
    pub cow: usize,
    pub pig: usize,
    pub sheep: usize,
    pub sheep_fur: usize,
    pub chicken: usize,
    pub player: usize,
    pub xp: usize,
    pub rain: usize,
    pub snow: usize,
    pub white: usize,
    pub items: usize,
}

/// Prebuilt entity models (built once, posed per frame).
#[derive(Debug)]
pub struct MobModels {
    pub cow: crate::models::Model,
    pub pig: crate::models::Model,
    pub sheep: crate::models::Model,
    pub sheep_fur: crate::models::Model,
    pub chicken: crate::models::Model,
    pub player: crate::models::Model,
    /// Just the right arm (first-person hand).
    pub arm: crate::models::Model,
}

pub struct Gfx {
    pub item_world: Vec<WorldIcon>,
    pub tex: EntityTextures,
    pub mob_models: MobModels,
    /// Player skin as a GUI texture (inventory preview).
    pub skin_gui: TexId,
    model_scratch: std::cell::RefCell<Vec<crate::models::ModelQuad>>,
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

pub fn tint_rgb(t: Tint) -> [f32; 3] {
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
                pts[k] = [0.5 + x1 * 0.625 * 1.12, 0.5 - y2 * 0.625 * 1.12];
                depth += z2;
            }
            // Rotated face normal: cull faces turned away from the viewer
            // (at +Z) so only the three visible faces are drawn.
            let n = match q.dir {
                Dir::Up => [0.0, 1.0, 0.0],
                Dir::Down => [0.0, -1.0, 0.0],
                Dir::North => [0.0, 0.0, -1.0],
                Dir::South => [0.0, 0.0, 1.0],
                Dir::West => [-1.0, 0.0, 0.0],
                Dir::East => [1.0, 0.0, 0.0],
            };
            let (nx1, nz1) = (n[0] * cy + n[2] * sy, -n[0] * sy + n[2] * cy);
            let nz2 = n[1] * sx + nz1 * cx;
            if nz2 < -1e-3 {
                continue;
            }
            // Vanilla-like GUI lighting: top brightest, the left face a little
            // darker, the right face darkest.
            let shade = match q.dir {
                Dir::Up => 1.0,
                Dir::Down => 0.45,
                _ if nx1 < 0.0 => 0.8,
                _ => 0.6,
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
    // Farthest first (the viewer is at +Z, so smaller z2 is farther).
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
            dirt: sp("block/dirt"),
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
            extra: std::collections::HashMap::new(),
        };
        let mut extra = |key: String, path: String| {
            if let Some(sp) = read_sprite(&mut gui, device, queue, &assets, &path) {
                theme.extra.insert(key, sp);
            }
        };
        for c in [
            "crafting_table",
            "generic_54",
            "furnace",
            "blast_furnace",
            "smoker",
        ] {
            extra(c.into(), format!("gui/container/{c}"));
        }
        for c in ["furnace", "blast_furnace", "smoker"] {
            extra(
                format!("{c}_lit"),
                format!("gui/sprites/container/{c}/lit_progress"),
            );
            extra(
                format!("{c}_burn"),
                format!("gui/sprites/container/{c}/burn_progress"),
            );
        }
        for c in ["anvil", "grindstone", "stonecutter", "smithing"] {
            extra(c.into(), format!("gui/container/{c}"));
        }
        for k in ["recipe", "recipe_selected", "recipe_highlighted"] {
            extra(
                format!("stonecutter_{k}"),
                format!("gui/sprites/container/stonecutter/{k}"),
            );
        }
        extra(
            "anvil_error".into(),
            "gui/sprites/container/anvil/error".into(),
        );
        extra(
            "anvil_text_field_disabled".into(),
            "gui/sprites/container/anvil/text_field_disabled".into(),
        );
        extra(
            "hotbar_offhand_left".into(),
            "gui/sprites/hud/hotbar_offhand_left".into(),
        );
        extra(
            "tab_inventory".into(),
            "gui/container/creative_inventory/tab_inventory".into(),
        );
        for tb in ["top", "bottom"] {
            for sel in ["selected", "unselected"] {
                for i in 1..=7 {
                    extra(
                        format!("tab_{tb}_{sel}_{i}"),
                        format!("gui/sprites/container/creative_inventory/tab_{tb}_{sel}_{i}"),
                    );
                }
            }
        }
        let mut sp = |p: &str| read_sprite(&mut gui, device, queue, &assets, p);
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
        let mut item_world = vec![WorldIcon::None; it.names.len()];
        let mut item_sprites: AHashMap<String, (TexId, [f32; 4])> = AHashMap::new();
        let mut item_imgs: Vec<(String, Image, Option<rc_assets::atlas::Animation>)> = Vec::new();
        let mut flat: Vec<(usize, String)> = Vec::new();
        for (i, name) in it.names.iter().enumerate() {
            let Some(layers) = flat_item_layers(&assets, name) else {
                continue;
            };
            let tinted = (0..layers.len()).any(|l| {
                crate::item_tints::layer_tint(name, l) != crate::item_tints::LayerTint::None
            });
            if layers.len() > 1 || tinted {
                // Layered / tinted sprite (spawn eggs, leather, potions…):
                // composed once into a single icon.
                let imgs: Vec<Image> = layers.iter().filter_map(|t| assets.png(t)).collect();
                if imgs.len() == layers.len()
                    && let Some(img) = crate::item_tints::compose(name, &imgs)
                {
                    let id = format!("rustcraft:icon/{name}");
                    item_imgs.push((id.clone(), img, None));
                    flat.push((i, id));
                    continue;
                }
            }
            flat.push((i, layers[0].clone()));
        }
        if !assets.is_vanilla() {
            // Generated icons for items that are not blocks.
            for (i, name) in it.names.iter().enumerate() {
                if it.block[i].is_none() {
                    let id = format!("rustcraft:item/{name}");
                    item_imgs.push((id.clone(), crate::fallback_tex::item_icon(name), None));
                    flat.push((i, id));
                }
            }
        }
        if assets.is_vanilla() {
            // Items the game draws with a block-entity renderer get an icon
            // rendered from the entity texture; anything else still without
            // a picture gets a generated icon instead of a magenta square.
            let has_flat: Vec<bool> = {
                let mut v = vec![false; it.names.len()];
                for (i, _) in &flat {
                    v[*i] = true;
                }
                v
            };
            for (i, name) in it.names.iter().enumerate() {
                if i == 0 || has_flat[i] {
                    continue;
                }
                let block_ok = it.block[i].is_some_and(|b| match block_icon(&models, b) {
                    Icon::Block(q) => !q.is_empty(),
                    _ => false,
                });
                let entity = crate::entity_icons::icon(&assets, name);
                if entity.is_none() && block_ok {
                    continue;
                }
                let id = format!("rustcraft:icon/{name}");
                let img = entity.unwrap_or_else(|| crate::fallback_tex::item_icon(name));
                // Developer aid: RC_DUMP_ICONS=<dir> writes these icons out.
                if let Some(dir) = std::env::var_os("RC_DUMP_ICONS") {
                    let _ = image::save_buffer(
                        std::path::Path::new(&dir).join(format!("{name}.png")),
                        &img.px,
                        img.w,
                        img.h,
                        image::ExtendedColorType::Rgba8,
                    );
                }
                item_imgs.push((id.clone(), img, None));
                flat.push((i, id));
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
        let mut terrain = terrain;
        let mut ent = |name: &str, fallback: fn() -> Image, repeat: bool| {
            let img = assets
                .png(&format!("minecraft:{name}"))
                .unwrap_or_else(fallback);
            terrain.add_texture(device, queue, &img, name, repeat)
        };
        let etex = EntityTextures {
            cow: ent(
                "entity/cow/cow",
                || crate::fallback_tex::mob([0.35, 0.24, 0.16], [0.93, 0.93, 0.9]),
                false,
            ),
            pig: ent(
                "entity/pig/pig",
                || crate::fallback_tex::mob([0.94, 0.65, 0.63], [0.85, 0.5, 0.5]),
                false,
            ),
            sheep: ent(
                "entity/sheep/sheep",
                || crate::fallback_tex::mob([0.85, 0.75, 0.68], [0.8, 0.7, 0.62]),
                false,
            ),
            sheep_fur: ent(
                "entity/sheep/sheep_fur",
                || crate::fallback_tex::mob([0.95, 0.95, 0.95], [0.9, 0.9, 0.9]),
                false,
            ),
            chicken: ent(
                "entity/chicken",
                || crate::fallback_tex::mob([0.97, 0.97, 0.97], [0.95, 0.75, 0.2]),
                false,
            ),
            player: ent("entity/player/wide/steve", crate::fallback_tex::skin, false),
            xp: ent("entity/experience_orb", crate::fallback_tex::orbs, false),
            rain: ent("environment/rain", crate::fallback_tex::rain, true),
            snow: ent("environment/snow", crate::fallback_tex::snow, true),
            white: ent("rustcraft:white", crate::fallback_tex::white, false),
            items: terrain.add_texture(device, queue, &item_atlas.mips[0], "item-atlas", false),
        };
        let skin_img = assets
            .png("minecraft:entity/player/wide/steve")
            .unwrap_or_else(crate::fallback_tex::skin);
        let skin_gui = gui.register(device, queue, &skin_img, false);
        let player_model = crate::models::player();
        let arm = crate::models::Model {
            parts: vec![player_model.parts[2].clone()],
            tex: player_model.tex,
        };
        let mob_models = MobModels {
            cow: crate::models::cow(),
            pig: crate::models::pig(),
            sheep: crate::models::sheep(),
            sheep_fur: crate::models::sheep_fur(),
            chicken: crate::models::chicken(),
            player: player_model,
            arm,
        };
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
            let tint = it.block[*i]
                .filter(|_| !tex.starts_with("rustcraft:icon/"))
                .map_or([1.0; 4], |b| {
                    let c = tint_rgb(models.tints[b.0 as usize]);
                    [c[0], c[1], c[2], 1.0]
                });
            icons[*i] = Icon::Sprite(t, uv, tint);
            item_world[*i] = WorldIcon::Flat(if t == atlas_tex { 0 } else { etex.items }, uv, tint);
        }
        for (i, b) in it.block.iter().enumerate() {
            if !matches!(icons[i], Icon::None) {
                continue;
            }
            let Some(b) = *b else { continue };
            // Fallback pack: plants are flat sprites, the rest 3D.
            let layer = models.layers[b.0 as usize];
            let flat_shape = blocks.collision(b).is_empty() && layer == rc_assets::Layer::Cutout;
            item_world[i] = if flat_shape {
                let c = tint_rgb(models.tints[b.0 as usize]);
                WorldIcon::Flat(0, models.particle[b.0 as usize].uv, [c[0], c[1], c[2], 1.0])
            } else {
                WorldIcon::Block(b)
            };
            icons[i] = if flat_shape && !assets.is_vanilla() {
                Icon::Sprite(atlas_tex, models.particle[b.0 as usize].uv, {
                    let c = tint_rgb(models.tints[b.0 as usize]);
                    [c[0], c[1], c[2], 1.0]
                })
            } else {
                block_icon(&models, b)
            };
        }
        {
            let missing: Vec<&str> = icons
                .iter()
                .enumerate()
                .filter(|(i, ic)| {
                    *i > 0
                        && match ic {
                            Icon::None => true,
                            Icon::Block(q) => q.is_empty(),
                            Icon::Sprite(..) => false,
                        }
                })
                .map(|(i, _)| it.names[i].as_str())
                .collect();
            if !missing.is_empty() {
                tracing::info!(count = missing.len(), items = ?missing, "items without an icon");
            }
        }
        let grass_map = assets.png("minecraft:colormap/grass");
        let foliage_map = assets.png("minecraft:colormap/foliage");
        Ok(Self {
            item_world,
            tex: etex,
            mob_models,
            skin_gui,
            model_scratch: std::cell::RefCell::new(Vec::with_capacity(256)),
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

impl Gfx {
    /// Draw the player model in a GUI box (feet at `(x, y)`, `size` units
    /// per block-ish), turned to follow the mouse offset like vanilla.
    pub fn draw_player(
        &self,
        dl: &mut DrawList,
        scale: f32,
        x: f32,
        y: f32,
        size: f32,
        mdx: f32,
        mdy: f32,
    ) {
        let mut q = self.model_scratch.borrow_mut();
        q.clear();
        let body_yaw = (mdx / 40.0).atan() * 20f32.to_radians() / 0.785;
        let head_yaw = (mdx / 40.0).atan() * 40f32.to_radians() / 0.785;
        let head_pitch = -(mdy / 40.0).atan() * 20f32.to_radians() / 0.785;
        crate::models::build(
            &self.mob_models.player,
            |n| {
                if n == "head" {
                    [head_pitch, head_yaw - body_yaw, 0.0]
                } else {
                    [0.0; 3]
                }
            },
            &mut q,
        );
        // Rotate whole model by body yaw (turning towards the mouse) plus
        // vanilla's 180° so we see the front.
        let (sy, cy) = body_yaw.sin_cos();
        let k = size / 16.0 * 0.9;
        let mut tris: Vec<(f32, [[f32; 2]; 4], [[f32; 2]; 4], f32)> = Vec::with_capacity(q.len());
        for m in q.iter() {
            let mut depth = 0.0;
            let mut pts = [[0.0f32; 2]; 4];
            for (i, p) in m.pos.iter().enumerate() {
                let (px, pz) = (p[0] * cy + p[2] * sy, -p[0] * sy + p[2] * cy);
                pts[i] = [(x - px * k) * scale, (y + (p[1] - 24.0) * k) * scale];
                depth += pz;
            }
            let n = m.normal;
            let nz = -n[0] * sy + n[2] * cy;
            // Viewer looks along +Z in model space (faces with -Z normals are visible).
            if nz > 0.01 {
                continue;
            }
            let shade = if n[1] < -0.5 {
                1.0
            } else if n[0].abs() > 0.5 {
                0.75
            } else {
                0.9
            };
            tris.push((depth, pts, m.uv, shade));
        }
        // Painter's order: farthest (largest z) first.
        tris.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, pts, uv, s) in tris {
            dl.quad_pts(self.skin_gui, pts, uv, [s, s, s, 1.0]);
        }
    }
}

/// All `layerN` textures (bottom first) if the item model is a flat
/// (generated) sprite.
fn flat_item_layers(assets: &Assets, name: &str) -> Option<Vec<String>> {
    if !assets.is_vanilla() {
        return None;
    }
    let mut id = format!("minecraft:item/{name}");
    let mut layers: Option<Vec<String>> = None;
    for _ in 0..8 {
        let b = assets.stack.read_id("models", &id, ".json")?;
        let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
        if layers.is_none() && v["textures"]["layer0"].is_string() {
            let mut out = Vec::new();
            for i in 0..8 {
                let Some(s) = v["textures"][format!("layer{i}")].as_str() else {
                    break;
                };
                out.push(if s.contains(':') {
                    s.to_owned()
                } else {
                    format!("minecraft:{s}")
                });
            }
            layers = Some(out);
        }
        let parent = v["parent"].as_str()?;
        let p = parent.strip_prefix("minecraft:").unwrap_or(parent);
        if p == "item/generated" || p == "item/handheld" || p == "builtin/generated" {
            return layers;
        }
        if p.starts_with("block/") || p.starts_with("builtin/") {
            return None;
        }
        id = format!("minecraft:{p}");
    }
    None
}
