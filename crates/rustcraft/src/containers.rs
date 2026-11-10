//! Inventory and container screens: survival inventory (2×2 crafting,
//! armour, offhand), crafting table, chests/barrels/shulker boxes, the
//! furnace family, and the tabbed creative inventory.

use rc_ui::{Key, Ui, WHITE};

use crate::blockentity::BlockEntity;
use crate::game::{Container, Game};
use crate::gfx::Gfx;
use crate::items::{self, Stack, Tab, click_slot, items};
use crate::recipes::recipes;
use crate::screens::{Action, draw_stack};

const LABEL: [f32; 4] = [0.25, 0.25, 0.25, 1.0];

fn slot_box(ui: &mut Ui<'_>, x: f32, y: f32) {
    ui.fill(x - 1.0, y - 1.0, 18.0, 18.0, [0.22, 0.22, 0.22, 1.0]);
    ui.fill(x, y, 17.0, 17.0, [1.0, 1.0, 1.0, 1.0]);
    ui.fill(x, y, 16.0, 16.0, [0.55, 0.55, 0.55, 1.0]);
}

/// Draw one slot; returns true when hovered.
fn inv_slot(ui: &mut Ui<'_>, gfx: &Gfx, s: Option<Stack>, x: f32, y: f32, draw_box: bool) -> bool {
    if draw_box {
        slot_box(ui, x, y);
    }
    if let Some(s) = s {
        draw_stack(ui, gfx, s, x, y);
    }
    let hover = ui.hover(x, y, 16.0, 16.0);
    if hover {
        ui.fill(x, y, 16.0, 16.0, [1.0, 1.0, 1.0, 0.5]);
    }
    hover
}

/// Draw a texture's top-left `w`×`h` region (container backgrounds are
/// 256×256 sheets).
fn panel(
    ui: &mut Ui<'_>,
    sprite: Option<rc_ui::Sprite>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    src_y: f32,
) {
    match sprite {
        Some(mut s) => {
            s.uv = [0.0, src_y / s.h, w / s.w, (src_y + h) / s.h];
            s.border = 0.0;
            ui.sprite(&s, x, y, w, h, WHITE);
        }
        None => {
            ui.fill(x, y, w, h, [0.78, 0.78, 0.78, 1.0]);
            ui.fill(x, y, w, 1.0, WHITE);
            ui.fill(x, y + h - 1.0, w, 1.0, [0.33, 0.33, 0.33, 1.0]);
        }
    }
}

/// A slot the player clicked.
#[derive(Clone, Copy, Debug)]
enum SlotRef {
    Inv(usize),
    Craft(usize),
    Result,
    Block(usize),
}

fn tooltip_for(ui: &mut Ui<'_>, s: Option<Stack>) {
    if let Some(s) = s {
        let it = items();
        let mut lines = vec![it.display_name(s.item)];
        let max = it.durability[s.item as usize];
        if max > 0 && s.damage > 0 {
            lines.push(format!("Durability: {} / {max}", max - s.damage));
        }
        ui.tooltip(&lines);
    }
}

fn cursor_stack(ui: &mut Ui<'_>, gfx: &Gfx, g: &Game) {
    if let Some(c) = g.inv.cursor {
        let (mx, my) = ui.mouse();
        draw_stack(ui, gfx, c, mx - 8.0, my - 8.0);
    }
}

/// Player inventory rows (main 3×9 then hotbar) at the given origin.
fn player_rows(
    ui: &mut Ui<'_>,
    gfx: &Gfx,
    g: &Game,
    x0: f32,
    y0: f32,
    draw_boxes: bool,
    hovered: &mut Option<SlotRef>,
) {
    for i in 0..36 {
        let (sx, sy) = if i < 9 {
            (x0 + i as f32 * 18.0, y0 + 58.0)
        } else {
            let k = i - 9;
            (x0 + (k % 9) as f32 * 18.0, y0 + (k / 9) as f32 * 18.0)
        };
        if inv_slot(ui, gfx, g.inv.slots[i], sx, sy, draw_boxes) {
            *hovered = Some(SlotRef::Inv(i));
        }
    }
}

fn craft_size(g: &Game) -> usize {
    if matches!(g.container, Some(Container::Crafting { .. })) {
        3
    } else {
        2
    }
}

fn craft_result(g: &Game) -> Option<Stack> {
    if let Some(Container::Station { kind, selected, .. }) = g.container {
        return station_outcome(g, kind, selected).map(|o| o.result);
    }
    let n = craft_size(g);
    recipes().craft(&g.craft[..n * n], n)
}

/// A station's result, if the player can afford it.
fn station_outcome(
    g: &Game,
    kind: crate::stations::Station,
    selected: Option<usize>,
) -> Option<crate::stations::Outcome> {
    let o = crate::stations::outcome(kind, &g.craft[..kind.inputs()], selected)?;
    let creative = g.player.mode == crate::player::GameMode::Creative;
    (creative || o.levels <= g.player.xp_level).then_some(o)
}

/// Take a station result: pay levels, use up inputs, wear the anvil.
fn take_station(g: &mut Game, to_inventory: bool) -> bool {
    let Some(Container::Station {
        pos,
        kind,
        selected,
        ..
    }) = g.container
    else {
        return false;
    };
    let Some(o) = station_outcome(g, kind, selected) else {
        return false;
    };
    if to_inventory {
        if g.inv.add_stack(o.result).is_some() {
            return false;
        }
    } else {
        match g.inv.cursor {
            None => g.inv.cursor = Some(o.result),
            Some(c) if c.stacks_with(&o.result) && c.count + o.result.count <= c.max() => {
                g.inv.cursor = Some(Stack {
                    count: c.count + o.result.count,
                    ..c
                });
            }
            _ => return false,
        }
    }
    for (i, n) in o.consume.iter().enumerate().take(kind.inputs()) {
        if let Some(s) = &mut g.craft[i] {
            s.count = s.count.saturating_sub(*n);
            if s.count == 0 {
                g.craft[i] = None;
            }
        }
    }
    if g.player.mode != crate::player::GameMode::Creative {
        g.player.xp_level = g.player.xp_level.saturating_sub(o.levels);
    }
    g.station_used(pos, kind);
    true
}

/// Take the crafting result once; returns false if nothing was crafted.
fn take_result(g: &mut Game, to_inventory: bool) -> bool {
    if matches!(g.container, Some(Container::Station { .. })) {
        return take_station(g, to_inventory);
    }
    let Some(r) = craft_result(g) else {
        return false;
    };
    if to_inventory {
        if g.inv.add_stack(r).is_some() {
            return false;
        }
    } else {
        match g.inv.cursor {
            None => g.inv.cursor = Some(r),
            Some(c) if c.stacks_with(&r) && c.count + r.count <= c.max() => {
                g.inv.cursor = Some(Stack {
                    count: c.count + r.count,
                    ..c
                });
            }
            _ => return false,
        }
    }
    let n = craft_size(g);
    for i in 0..n * n {
        if let Some(s) = &mut g.craft[i] {
            // Buckets of milk etc. leave the bucket.
            let name = items().name(s.item);
            let remainder = match name {
                "milk_bucket" | "water_bucket" | "lava_bucket" => items().id("bucket"),
                "honey_bottle" => items().id("glass_bottle"),
                _ => None,
            };
            s.count -= 1;
            if s.count == 0 {
                g.craft[i] = remainder.map(|r| Stack::new(r, 1));
            }
        }
    }
    true
}

fn click(g: &mut Game, slot: SlotRef, right: bool, shift: bool) {
    fn block_slots(g: &mut Game) -> Option<&mut Vec<Option<Stack>>> {
        let pos = match g.container {
            Some(Container::Chest { pos } | Container::Furnace { pos }) => pos,
            _ => return None,
        };
        g.world
            .block_entities
            .get_mut(&pos)
            .map(BlockEntity::slots_mut)
    }
    match slot {
        SlotRef::Result => {
            if shift {
                for _ in 0..64 {
                    if !take_result(g, true) {
                        break;
                    }
                }
            } else {
                take_result(g, false);
            }
        }
        SlotRef::Inv(i) => {
            if shift {
                // Move to the open container if there is one, else hotbar <-> main.
                if let Some(st) = g.inv.slots[i]
                    && let Some(slots) = block_slots(g)
                {
                    let mut st = st;
                    let furnace_input = slots.len() == 3;
                    let range: Vec<usize> = if furnace_input {
                        let fuel = crate::recipes::fuel_ticks(st.item) > 0;
                        vec![if fuel
                            && recipes()
                                .cook(crate::recipes::Cooker::Furnace, st.item)
                                .is_none()
                        {
                            1
                        } else {
                            0
                        }]
                    } else {
                        (0..slots.len()).collect()
                    };
                    for &k in &range {
                        match &mut slots[k] {
                            Some(s) if s.stacks_with(&st) && s.count < s.max() => {
                                let n = (s.max() - s.count).min(st.count);
                                s.count += n;
                                st.count -= n;
                            }
                            None => {
                                slots[k] = Some(st);
                                st.count = 0;
                            }
                            _ => {}
                        }
                        if st.count == 0 {
                            break;
                        }
                    }
                    g.inv.slots[i] = (st.count > 0).then_some(st);
                } else if let Some(Container::Station { kind, .. }) = g.container {
                    // Into the station's inputs.
                    if let Some(mut st) = g.inv.slots[i] {
                        for k in 0..kind.inputs() {
                            match &mut g.craft[k] {
                                Some(c) if c.stacks_with(&st) && c.count < c.max() => {
                                    let n = (c.max() - c.count).min(st.count);
                                    c.count += n;
                                    st.count -= n;
                                }
                                None => {
                                    g.craft[k] = Some(st);
                                    st.count = 0;
                                }
                                _ => {}
                            }
                            if st.count == 0 {
                                break;
                            }
                        }
                        g.inv.slots[i] = (st.count > 0).then_some(st);
                    }
                } else {
                    g.inv.quick_move(i);
                }
            } else {
                let mut cursor = g.inv.cursor.take();
                click_slot(&mut g.inv.slots[i], &mut cursor, right);
                g.inv.cursor = cursor;
            }
        }
        SlotRef::Craft(i) => {
            if shift {
                if let Some(s) = g.craft[i].take()
                    && let Some(left) = g.inv.add_stack(s)
                {
                    g.craft[i] = Some(left);
                }
            } else {
                let mut cursor = g.inv.cursor.take();
                click_slot(&mut g.craft[i], &mut cursor, right);
                g.inv.cursor = cursor;
            }
        }
        SlotRef::Block(k) => {
            let mut cursor = g.inv.cursor.take();
            let mut moved: Option<Stack> = None;
            if let Some(slots) = block_slots(g) {
                if shift {
                    moved = slots[k].take();
                } else if !(slots.len() == 3 && k == 2 && cursor.is_some()) {
                    click_slot(&mut slots[k], &mut cursor, right);
                }
            }
            g.inv.cursor = cursor;
            if let Some(st) = moved
                && let Some(left) = g.inv.add_stack(st)
                && let Some(slots) = block_slots(g)
            {
                slots[k] = Some(left);
            }
            if let Some(Container::Chest { pos } | Container::Furnace { pos }) = g.container {
                g.world.mark_dirty(pos);
            }
        }
    }
}

fn handle_click(ui: &mut Ui<'_>, g: &mut Game, hovered: Option<SlotRef>) {
    if let Some(s) = hovered {
        if ui.input.pressed {
            ui.input.pressed = false;
            click(g, s, false, ui.input.shift);
        } else if ui.input.right_pressed {
            click(g, s, true, false);
        }
    }
}

fn hovered_stack(g: &Game, s: Option<SlotRef>) -> Option<Stack> {
    match s? {
        SlotRef::Inv(i) => g.inv.slots[i],
        SlotRef::Craft(i) => g.craft[i],
        SlotRef::Result => craft_result(g),
        SlotRef::Block(k) => {
            let pos = match g.container {
                Some(Container::Chest { pos } | Container::Furnace { pos }) => pos,
                _ => return None,
            };
            match g.world.block_entities.get(&pos)? {
                BlockEntity::Container { slots } | BlockEntity::Furnace { slots, .. } => {
                    slots.get(k).copied().flatten()
                }
            }
        }
    }
}

/// Survival inventory, crafting table, chests and furnaces.
pub fn container(ui: &mut Ui<'_>, gfx: &Gfx, g: &mut Game, out: &mut Vec<Action>) {
    let (w, h) = (ui.w, ui.h);
    ui.fill(0.0, 0.0, w, h, [0.06, 0.06, 0.06, 0.75]);
    let mut hovered: Option<SlotRef> = None;
    let theme = &gfx.theme;
    let container = g.container.clone();
    match &container {
        None => {
            let (x0, y0) = ((w - 176.0) / 2.0, (h - 166.0) / 2.0);
            let tex = theme.inventory;
            panel(ui, tex, x0, y0, 176.0, 166.0, 0.0);
            ui.text_plain("Crafting", x0 + 97.0, y0 + 8.0, LABEL);
            // Player model.
            let (mx, my) = ui.mouse();
            gfx.draw_player(
                ui.dl,
                ui.scale,
                x0 + 51.0,
                y0 + 75.0,
                30.0,
                mx - (x0 + 51.0),
                my - (y0 + 25.0),
            );
            for i in 0..4 {
                let (sx, sy) = (
                    x0 + 98.0 + (i % 2) as f32 * 18.0,
                    y0 + 18.0 + (i / 2) as f32 * 18.0,
                );
                if inv_slot(ui, gfx, g.craft[i], sx, sy, tex.is_none()) {
                    hovered = Some(SlotRef::Craft(i));
                }
            }
            if inv_slot(
                ui,
                gfx,
                craft_result(g),
                x0 + 154.0,
                y0 + 28.0,
                tex.is_none(),
            ) {
                hovered = Some(SlotRef::Result);
            }
            // Armour (head..feet) and offhand.
            for k in 0..4 {
                let slot = 39 - k;
                if inv_slot(
                    ui,
                    gfx,
                    g.inv.slots[slot],
                    x0 + 8.0,
                    y0 + 8.0 + k as f32 * 18.0,
                    tex.is_none(),
                ) {
                    hovered = Some(SlotRef::Inv(slot));
                }
            }
            if inv_slot(
                ui,
                gfx,
                g.inv.slots[40],
                x0 + 77.0,
                y0 + 62.0,
                tex.is_none(),
            ) {
                hovered = Some(SlotRef::Inv(40));
            }
            player_rows(ui, gfx, g, x0 + 8.0, y0 + 84.0, tex.is_none(), &mut hovered);
        }
        Some(Container::Crafting { .. }) => {
            let (x0, y0) = ((w - 176.0) / 2.0, (h - 166.0) / 2.0);
            let tex = theme.extra.get("crafting_table").copied();
            panel(ui, tex, x0, y0, 176.0, 166.0, 0.0);
            ui.text_plain("Crafting", x0 + 28.0, y0 + 6.0, LABEL);
            for i in 0..9 {
                let (sx, sy) = (
                    x0 + 30.0 + (i % 3) as f32 * 18.0,
                    y0 + 17.0 + (i / 3) as f32 * 18.0,
                );
                if inv_slot(ui, gfx, g.craft[i], sx, sy, tex.is_none()) {
                    hovered = Some(SlotRef::Craft(i));
                }
            }
            if inv_slot(
                ui,
                gfx,
                craft_result(g),
                x0 + 124.0,
                y0 + 35.0,
                tex.is_none(),
            ) {
                hovered = Some(SlotRef::Result);
            }
            ui.text_plain("Inventory", x0 + 8.0, y0 + 72.0, LABEL);
            player_rows(ui, gfx, g, x0 + 8.0, y0 + 84.0, tex.is_none(), &mut hovered);
        }
        Some(Container::Station {
            kind,
            selected,
            scroll,
            ..
        }) => {
            use crate::stations::Station;
            let (kind, selected, scroll) = (*kind, *selected, *scroll);
            let (x0, y0) = ((w - 176.0) / 2.0, (h - 166.0) / 2.0);
            let key = match kind {
                Station::Anvil => "anvil",
                Station::Grindstone => "grindstone",
                Station::Stonecutter => "stonecutter",
                Station::Smithing => "smithing",
            };
            let tex = theme.extra.get(key).copied();
            panel(ui, tex, x0, y0, 176.0, 166.0, 0.0);
            let (inputs, result): (&[(f32, f32)], (f32, f32)) = match kind {
                Station::Anvil => (&[(27.0, 47.0), (76.0, 47.0)], (134.0, 47.0)),
                Station::Grindstone => (&[(49.0, 19.0), (49.0, 40.0)], (129.0, 34.0)),
                Station::Stonecutter => (&[(20.0, 33.0)], (143.0, 33.0)),
                Station::Smithing => (&[(8.0, 48.0), (26.0, 48.0), (44.0, 48.0)], (98.0, 48.0)),
            };
            let title_x = match kind {
                Station::Anvil => 60.0,
                Station::Smithing => 44.0,
                _ => 8.0,
            };
            ui.text_plain(kind.title(), x0 + title_x, y0 + 6.0, LABEL);
            if kind == Station::Anvil
                && let Some(sp) = theme.extra.get("anvil_text_field_disabled")
            {
                // Renaming needs item names (Phase 3): the field stays disabled.
                ui.sprite(sp, x0 + 59.0, y0 + 20.0, 110.0, 16.0, WHITE);
            }
            for (i, (sx, sy)) in inputs.iter().enumerate() {
                if inv_slot(ui, gfx, g.craft[i], x0 + sx, y0 + sy, tex.is_none()) {
                    hovered = Some(SlotRef::Craft(i));
                }
            }
            if inv_slot(
                ui,
                gfx,
                craft_result(g),
                x0 + result.0,
                y0 + result.1,
                tex.is_none(),
            ) {
                hovered = Some(SlotRef::Result);
            }
            if kind == Station::Anvil
                && let Some(o) = crate::stations::outcome(kind, &g.craft[..2], None)
                && o.levels > 0
            {
                let creative = g.player.mode == crate::player::GameMode::Creative;
                let ok = creative || o.levels <= g.player.xp_level;
                let msg = format!("Enchantment Cost: {}", o.levels);
                let c = if ok {
                    [0.5, 1.0, 0.13, 1.0]
                } else {
                    [1.0, 0.38, 0.38, 1.0]
                };
                let tw = ui.font.width(&msg);
                ui.fill(
                    x0 + 168.0 - tw - 4.0,
                    y0 + 67.0,
                    tw + 4.0,
                    12.0,
                    [0.0, 0.0, 0.0, 0.3],
                );
                ui.text(&msg, x0 + 168.0 - tw - 2.0, y0 + 69.0, c);
            }
            if kind == Station::Stonecutter {
                let opts = g.craft[0]
                    .map(crate::stations::stonecutter_options)
                    .unwrap_or_default();
                let rows = opts.len().div_ceil(4);
                let max_scroll = rows.saturating_sub(3);
                let mut new_scroll = scroll.min(max_scroll);
                if ui.hover(x0 + 52.0, y0 + 14.0, 75.0, 54.0) && ui.input.scroll != 0.0 {
                    new_scroll = if ui.input.scroll > 0.0 {
                        new_scroll.saturating_sub(1)
                    } else {
                        (new_scroll + 1).min(max_scroll)
                    };
                }
                let mut new_sel = selected.filter(|s| *s < opts.len());
                for (k, st) in opts.iter().enumerate().skip(new_scroll * 4).take(12) {
                    let v = k - new_scroll * 4;
                    let (bx, by) = (
                        x0 + 52.0 + (v % 4) as f32 * 16.0,
                        y0 + 14.0 + (v / 4) as f32 * 18.0,
                    );
                    let hover = ui.hover(bx, by, 16.0, 18.0);
                    let sprite = if new_sel == Some(k) {
                        "stonecutter_recipe_selected"
                    } else if hover {
                        "stonecutter_recipe_highlighted"
                    } else {
                        "stonecutter_recipe"
                    };
                    match theme.extra.get(sprite) {
                        Some(sp) => ui.sprite(sp, bx, by, 16.0, 18.0, WHITE),
                        None => ui.fill(
                            bx,
                            by,
                            16.0,
                            18.0,
                            if new_sel == Some(k) {
                                [0.4, 0.6, 0.4, 1.0]
                            } else {
                                [0.6, 0.6, 0.6, 1.0]
                            },
                        ),
                    }
                    draw_stack(ui, gfx, *st, bx, by + 1.0);
                    if hover {
                        if ui.input.pressed {
                            ui.input.pressed = false;
                            ui.input.clicked_sound = true;
                            new_sel = Some(k);
                        }
                        let it = items();
                        ui.tooltip(&[it.display_name(st.item)]);
                    }
                }
                if g.craft[0].is_none() {
                    new_sel = None;
                }
                if let Some(Container::Station {
                    selected, scroll, ..
                }) = &mut g.container
                {
                    *selected = new_sel;
                    *scroll = new_scroll;
                }
            }
            ui.text_plain("Inventory", x0 + 8.0, y0 + 72.0, LABEL);
            player_rows(ui, gfx, g, x0 + 8.0, y0 + 84.0, tex.is_none(), &mut hovered);
        }
        Some(Container::Chest { pos }) => {
            let rows = 3.0;
            let ih = 114.0 + rows * 18.0;
            let (x0, y0) = ((w - 176.0) / 2.0, (h - ih) / 2.0);
            let tex = theme.extra.get("generic_54").copied();
            panel(ui, tex, x0, y0, 176.0, rows * 18.0 + 17.0, 0.0);
            panel(ui, tex, x0, y0 + rows * 18.0 + 17.0, 176.0, 96.0, 126.0);
            let title =
                items().display_name(items().for_block(g.world.get_block(*pos)).unwrap_or(0));
            ui.text_plain(&title, x0 + 8.0, y0 + 6.0, LABEL);
            let slots: Vec<Option<Stack>> = match g.world.block_entities.get(pos) {
                Some(BlockEntity::Container { slots }) => slots.clone(),
                _ => vec![None; 27],
            };
            for (k, s) in slots.iter().enumerate().take(27) {
                let (sx, sy) = (
                    x0 + 8.0 + (k % 9) as f32 * 18.0,
                    y0 + 18.0 + (k / 9) as f32 * 18.0,
                );
                if inv_slot(ui, gfx, *s, sx, sy, tex.is_none()) {
                    hovered = Some(SlotRef::Block(k));
                }
            }
            ui.text_plain("Inventory", x0 + 8.0, y0 + rows * 18.0 + 20.0, LABEL);
            player_rows(
                ui,
                gfx,
                g,
                x0 + 8.0,
                y0 + rows * 18.0 + 31.0,
                tex.is_none(),
                &mut hovered,
            );
        }
        Some(Container::Furnace { pos }) => {
            let (x0, y0) = ((w - 176.0) / 2.0, (h - 166.0) / 2.0);
            let name = rc_data::blocks().def(g.world.get_block(*pos)).name.clone();
            let key = match name.as_str() {
                "blast_furnace" => "blast_furnace",
                "smoker" => "smoker",
                _ => "furnace",
            };
            let tex = theme.extra.get(key).copied();
            panel(ui, tex, x0, y0, 176.0, 166.0, 0.0);
            let title = items().display_name(items().id(&name).unwrap_or(0));
            ui.text_centered(&title, x0 + 88.0, y0 + 6.0, LABEL);
            if let Some(BlockEntity::Furnace {
                slots,
                burn,
                burn_total,
                cook,
                cook_total,
                ..
            }) = g.world.block_entities.get(pos).cloned()
            {
                let places = [(56.0, 17.0), (56.0, 53.0), (116.0, 35.0)];
                for (k, (sx, sy)) in places.iter().enumerate() {
                    if inv_slot(ui, gfx, slots[k], x0 + sx, y0 + sy, tex.is_none()) {
                        hovered = Some(SlotRef::Block(k));
                    }
                }
                if burn > 0 && burn_total > 0 {
                    let f = burn as f32 / burn_total as f32;
                    let hh = (14.0 * f).ceil();
                    if let Some(mut s) = theme.extra.get(&format!("{key}_lit")).copied() {
                        s.uv = [0.0, 1.0 - hh / 14.0, 1.0, 1.0];
                        ui.sprite(&s, x0 + 56.0, y0 + 36.0 + 14.0 - hh, 14.0, hh, WHITE);
                    } else {
                        ui.fill(
                            x0 + 57.0,
                            y0 + 37.0 + 13.0 - hh,
                            12.0,
                            hh,
                            [1.0, 0.6, 0.1, 1.0],
                        );
                    }
                }
                if cook > 0 && cook_total > 0 {
                    let wv = (24.0 * cook as f32 / cook_total as f32).ceil();
                    if let Some(mut s) = theme.extra.get(&format!("{key}_burn")).copied() {
                        s.uv = [0.0, 0.0, wv / 24.0, 1.0];
                        ui.sprite(&s, x0 + 79.0, y0 + 34.0, wv, 16.0, WHITE);
                    } else {
                        ui.fill(x0 + 80.0, y0 + 40.0, wv, 4.0, WHITE);
                    }
                }
            }
            ui.text_plain("Inventory", x0 + 8.0, y0 + 72.0, LABEL);
            player_rows(ui, gfx, g, x0 + 8.0, y0 + 84.0, tex.is_none(), &mut hovered);
        }
    }
    handle_click(ui, g, hovered);
    if g.inv.cursor.is_none() {
        let st = hovered_stack(g, hovered);
        tooltip_for(ui, st);
    }
    cursor_stack(ui, gfx, g);
    if ui.input.key(Key::Escape) {
        g.close_container();
        out.push(Action::CloseAll);
    }
}

/// Which creative tab is open: one of the item tabs, search, or the
/// survival inventory view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreativeTab {
    Items(Tab),
    Search,
    Inventory,
}

/// Tab positions: top row 0..5 and 6 (search); bottom row 0..5 and 6 (inventory).
fn tab_slot(t: CreativeTab) -> (bool, usize) {
    match t {
        CreativeTab::Items(tab) => {
            if let Some(i) = Tab::TOP.iter().position(|x| *x == tab) {
                (true, i)
            } else {
                (
                    false,
                    Tab::BOTTOM.iter().position(|x| *x == tab).unwrap_or(0),
                )
            }
        }
        CreativeTab::Search => (true, 6),
        CreativeTab::Inventory => (false, 6),
    }
}

pub fn creative(
    ui: &mut Ui<'_>,
    gfx: &Gfx,
    g: &mut Game,
    tab: &mut CreativeTab,
    scroll: &mut f32,
    search: &mut String,
    out: &mut Vec<Action>,
) {
    let (w, h) = (ui.w, ui.h);
    ui.fill(0.0, 0.0, w, h, [0.06, 0.06, 0.06, 0.75]);
    let (x0, y0) = ((w - 195.0) / 2.0, (h - 136.0) / 2.0);
    let it = items();
    let theme = &gfx.theme;
    // Tabs: unselected ones behind the panel, the selected one in front.
    let all_tabs: Vec<CreativeTab> = Tab::TOP
        .iter()
        .chain(Tab::BOTTOM.iter())
        .filter(|t| t.available())
        .map(|t| CreativeTab::Items(*t))
        .chain([CreativeTab::Search, CreativeTab::Inventory])
        .collect();
    let tab_rect = |t: CreativeTab| -> (f32, f32) {
        let (top, i) = tab_slot(t);
        let x = if i == 6 {
            x0 + 195.0 - 28.0
        } else {
            x0 + i as f32 * 27.0
        };
        (x, if top { y0 - 28.0 } else { y0 + 136.0 - 4.0 })
    };
    let draw_tab = |ui: &mut Ui<'_>, t: CreativeTab, selected: bool| {
        let (top, i) = tab_slot(t);
        let (x, y) = tab_rect(t);
        let idx = i.min(6);
        let key = format!(
            "tab_{}_{}_{}",
            if top { "top" } else { "bottom" },
            if selected { "selected" } else { "unselected" },
            idx + 1
        );
        match theme.extra.get(&key) {
            Some(s) => ui.sprite(s, x, y, 26.0, 32.0, WHITE),
            None => ui.fill(
                x,
                y,
                26.0,
                30.0,
                if selected {
                    [0.78, 0.78, 0.78, 1.0]
                } else {
                    [0.55, 0.55, 0.55, 1.0]
                },
            ),
        }
        let icon = match t {
            CreativeTab::Items(tb) => tb.icon(),
            CreativeTab::Search => "compass",
            CreativeTab::Inventory => "chest",
        };
        if let Some(id) = it.id(icon) {
            let iy = if top { y + 9.0 } else { y + 7.0 };
            gfx.draw_item(ui.dl, ui.scale, id, x + 5.0, iy, 16.0);
        }
    };
    for t in &all_tabs {
        if t != tab {
            draw_tab(ui, *t, false);
        }
    }
    let bg = match tab {
        CreativeTab::Search => theme.creative_search.or(theme.creative_items),
        CreativeTab::Inventory => theme.extra.get("tab_inventory").copied(),
        CreativeTab::Items(_) => theme.creative_items,
    };
    let has_tex = bg.is_some();
    panel(ui, bg, x0, y0, 195.0, 136.0, 0.0);
    draw_tab(ui, *tab, true);
    // Tab clicks and hover labels.
    for t in &all_tabs {
        let (x, y) = tab_rect(*t);
        if ui.hover(x, y, 26.0, 30.0) {
            let label = match t {
                CreativeTab::Items(tb) => tb.label().to_owned(),
                CreativeTab::Search => "Search Items".into(),
                CreativeTab::Inventory => "Survival Inventory".into(),
            };
            if ui.input.pressed {
                ui.input.pressed = false;
                *tab = *t;
                *scroll = 0.0;
            }
            ui.tooltip(&[label]);
        }
    }
    let mut hovered_inv: Option<usize> = None;
    if *tab == CreativeTab::Inventory {
        // Armour, offhand and the full inventory.
        let (mx, my) = ui.mouse();
        gfx.draw_player(
            ui.dl,
            ui.scale,
            x0 + 88.0,
            y0 + 50.0,
            20.0,
            mx - (x0 + 88.0),
            my - (y0 + 15.0),
        );
        let armour = [
            (54.0, 6.0, 39),
            (54.0, 33.0, 38),
            (108.0, 6.0, 37),
            (108.0, 33.0, 36),
            (35.0, 20.0, 40),
        ];
        for (ax, ay, slot) in armour {
            if inv_slot(ui, gfx, g.inv.slots[slot], x0 + ax, y0 + ay, !has_tex) {
                hovered_inv = Some(slot);
            }
        }
        for i in 0..36 {
            let (sx, sy) = if i < 9 {
                (x0 + 9.0 + i as f32 * 18.0, y0 + 112.0)
            } else {
                let k = i - 9;
                (
                    x0 + 9.0 + (k % 9) as f32 * 18.0,
                    y0 + 54.0 + (k / 9) as f32 * 18.0,
                )
            };
            if inv_slot(ui, gfx, g.inv.slots[i], sx, sy, !has_tex) {
                hovered_inv = Some(i);
            }
        }
        // Destroy-item slot.
        let (dx, dy) = (x0 + 173.0, y0 + 112.0);
        if ui.hover(dx, dy, 16.0, 16.0) && ui.input.pressed && g.inv.cursor.is_some() {
            ui.input.pressed = false;
            g.inv.cursor = None;
        }
        if let Some(i) = hovered_inv {
            if ui.input.pressed {
                ui.input.pressed = false;
                if ui.input.shift {
                    g.inv.slots[i] = None;
                } else {
                    g.inv.click(i, false);
                }
            } else if ui.input.right_pressed {
                g.inv.click(i, true);
            }
            if g.inv.cursor.is_none() {
                tooltip_for(ui, g.inv.slots[i]);
            }
        }
        cursor_stack(ui, gfx, g);
        if ui.input.key(Key::Escape) {
            g.inv.cursor = None;
            out.push(Action::CloseAll);
        }
        return;
    }
    let label = match tab {
        CreativeTab::Items(t) => t.label(),
        CreativeTab::Search => "Search Items",
        CreativeTab::Inventory => "",
    };
    ui.text_plain(label, x0 + 8.0, y0 + 6.0, LABEL);
    if *tab == CreativeTab::Search {
        ui.input.focus = Some(rc_ui::id("creative-search"));
        ui.text_field(
            "creative-search",
            x0 + 81.0,
            y0 + 5.0,
            90.0,
            12.0,
            search,
            50,
            "",
        );
    }
    let q = search.to_lowercase();
    let list: Vec<u16> = match tab {
        CreativeTab::Items(t) => it.tabs.get(t).cloned().unwrap_or_default(),
        _ => it
            .all
            .iter()
            .copied()
            .filter(|&i| {
                q.is_empty()
                    || it.name(i).replace('_', " ").contains(&q)
                    || it.display_name(i).to_lowercase().contains(&q)
            })
            .collect(),
    };
    let rows = list.len().div_ceil(9);
    let max_scroll = rows.saturating_sub(5) as f32;
    if ui.input.scroll != 0.0 {
        *scroll = (*scroll - ui.input.scroll).clamp(0.0, max_scroll);
    }
    let start = (*scroll as usize) * 9;
    let mut hovered_palette = None;
    for r in 0..5 {
        for c in 0..9 {
            let i = start + r * 9 + c;
            let (sx, sy) = (x0 + 9.0 + c as f32 * 18.0, y0 + 18.0 + r as f32 * 18.0);
            let s = list.get(i).map(|&item| Stack::new(item, 1));
            if inv_slot(ui, gfx, s, sx, sy, !has_tex) && s.is_some() {
                hovered_palette = s;
            }
        }
    }
    let mut hovered_hotbar = None;
    for i in 0..9 {
        let (sx, sy) = (x0 + 9.0 + i as f32 * 18.0, y0 + 112.0);
        if inv_slot(ui, gfx, g.inv.slots[i], sx, sy, !has_tex) {
            hovered_hotbar = Some(i);
        }
    }
    let track = 112.0 - 17.0;
    let sy = y0
        + 18.0
        + if max_scroll > 0.0 {
            *scroll / max_scroll * track
        } else {
            0.0
        };
    match theme.scroller {
        Some(s) => ui.sprite(&s, x0 + 175.0, sy, 12.0, 15.0, WHITE),
        None => ui.fill(x0 + 175.0, sy, 12.0, 15.0, [0.8, 0.8, 0.8, 1.0]),
    }
    if ui.input.down && ui.hover(x0 + 174.0, y0 + 18.0, 14.0, 112.0) && max_scroll > 0.0 {
        let (_, my) = ui.mouse();
        *scroll = (((my - y0 - 18.0 - 7.0) / track) * max_scroll)
            .round()
            .clamp(0.0, max_scroll);
    }
    if let Some(s) = hovered_palette {
        let max = it.max_stack[s.item as usize];
        if ui.input.pressed {
            ui.input.pressed = false;
            if g.inv.cursor.is_some_and(|c| c.item == s.item)
                && g.inv.cursor.is_some_and(|c| c.count < max)
            {
                if let Some(c) = &mut g.inv.cursor {
                    c.count += 1;
                }
            } else if g.inv.cursor.is_some() {
                g.inv.cursor = None;
            } else if ui.input.shift {
                g.inv.add(s.item, max);
            } else {
                g.inv.cursor = Some(Stack::new(s.item, max));
            }
        } else if ui.input.right_pressed && g.inv.cursor.is_none() {
            g.inv.cursor = Some(Stack::new(s.item, 1));
        }
        if g.inv.cursor.is_none() {
            ui.tooltip(&[
                it.display_name(s.item),
                format!("§9minecraft:{}", it.name(s.item)),
            ]);
        }
    }
    if let Some(i) = hovered_hotbar {
        if ui.input.pressed {
            ui.input.pressed = false;
            if ui.input.shift {
                g.inv.slots[i] = None;
            } else {
                g.inv.click(i, false);
            }
        } else if ui.input.right_pressed {
            g.inv.click(i, true);
        }
        if g.inv.cursor.is_none() {
            tooltip_for(ui, g.inv.slots[i]);
        }
    }
    if ui.input.pressed && !ui.hover(x0, y0 - 28.0, 195.0, 196.0) {
        g.inv.cursor = None;
    }
    cursor_stack(ui, gfx, g);
    if ui.input.key(Key::Escape) {
        g.inv.cursor = None;
        out.push(Action::CloseAll);
    }
    let _ = items::Tab::Building;
}
