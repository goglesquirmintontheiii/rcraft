//! Block entities: containers (chests, barrels, shulker boxes) and the
//! furnace family. Persisted inside chunk records.

use serde::{Deserialize, Serialize};

use crate::items::Stack;
use crate::recipes::{Cooker, fuel_ticks, recipes};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BlockEntity {
    Container {
        slots: Vec<Option<Stack>>,
    },
    Furnace {
        cooker: Cooker,
        /// input, fuel, output
        slots: Vec<Option<Stack>>,
        burn: u32,
        burn_total: u32,
        cook: u32,
        cook_total: u32,
        #[serde(default)]
        xp: f32,
    },
}

impl BlockEntity {
    /// Block entity for a newly placed block, if it needs one.
    #[must_use]
    pub fn for_block(name: &str) -> Option<Self> {
        if matches!(name, "chest" | "trapped_chest" | "barrel") || name.ends_with("shulker_box") {
            return Some(Self::Container {
                slots: vec![None; 27],
            });
        }
        let cooker = match name {
            "furnace" => Cooker::Furnace,
            "blast_furnace" => Cooker::Blast,
            "smoker" => Cooker::Smoker,
            _ => return None,
        };
        Some(Self::Furnace {
            cooker,
            slots: vec![None; 3],
            burn: 0,
            burn_total: 0,
            cook: 0,
            cook_total: 200,
            xp: 0.0,
        })
    }

    #[must_use]
    pub fn slots_mut(&mut self) -> &mut Vec<Option<Stack>> {
        match self {
            Self::Container { slots } | Self::Furnace { slots, .. } => slots,
        }
    }

    /// Items to drop when the block is broken.
    #[must_use]
    pub fn contents(&self) -> Vec<Stack> {
        match self {
            Self::Container { slots } | Self::Furnace { slots, .. } => {
                slots.iter().flatten().copied().collect()
            }
        }
    }

    /// Furnace tick; returns whether the "lit" state changed.
    pub fn tick(&mut self) -> Option<bool> {
        let Self::Furnace {
            cooker,
            slots,
            burn,
            burn_total,
            cook,
            cook_total,
            xp,
        } = self
        else {
            return None;
        };
        let was_lit = *burn > 0;
        if *burn > 0 {
            *burn -= 1;
        }
        let recipe = slots[0]
            .and_then(|s| recipes().cook(*cooker, s.item))
            .cloned();
        let can_output = recipe.as_ref().is_some_and(|r| match slots[2] {
            None => true,
            Some(o) => o.stacks_with(&r.result) && o.count + r.result.count <= o.max(),
        });
        if *burn == 0
            && can_output
            && let Some(f) = slots[1]
        {
            let t = fuel_ticks(f.item);
            if t > 0 {
                *burn = t;
                *burn_total = t;
                let n = f.count - 1;
                slots[1] = if n == 0 {
                    // Lava buckets leave the bucket behind.
                    if crate::items::items().name(f.item) == "lava_bucket" {
                        crate::items::items().id("bucket").map(|b| Stack::new(b, 1))
                    } else {
                        None
                    }
                } else {
                    Some(Stack { count: n, ..f })
                };
            }
        }
        if *burn > 0 && can_output {
            let r = recipe.expect("checked");
            let speed = if *cooker == Cooker::Furnace { 1 } else { 2 };
            *cook_total = r.ticks / speed.max(1);
            *cook += 1;
            if *cook >= *cook_total {
                *cook = 0;
                if let Some(i) = &mut slots[0] {
                    i.count -= 1;
                    if i.count == 0 {
                        slots[0] = None;
                    }
                }
                slots[2] = Some(match slots[2] {
                    Some(o) => Stack {
                        count: o.count + r.result.count,
                        ..o
                    },
                    None => r.result,
                });
                *xp += r.xp;
            }
        } else if *cook > 0 {
            *cook = cook.saturating_sub(2);
        }
        let lit = *burn > 0;
        (lit != was_lit).then_some(lit)
    }
}
