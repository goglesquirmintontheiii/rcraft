//! Workstation blocks with temporary input slots: anvil (repair), grindstone
//! (combine), stonecutter and smithing table (netherite upgrades). Behaviour
//! from minecraft.wiki ("Anvil", "Grindstone", "Stonecutter", "Smithing
//! Table"); recipes from the vendored data-generator output (ADR-015).
//!
//! Inputs live in the game's crafting grid (`Game::craft`), which is handed
//! back to the inventory when the screen closes, exactly like vanilla.
//! Renaming, enchantment combining and armour trims need item components and
//! enchantments, which arrive with Phase 3.

use crate::items::{Stack, items};
use crate::recipes::recipes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Station {
    Anvil,
    Grindstone,
    Stonecutter,
    Smithing,
}

impl Station {
    #[must_use]
    pub fn for_block(name: &str) -> Option<Self> {
        match name {
            "anvil" | "chipped_anvil" | "damaged_anvil" => Some(Self::Anvil),
            "grindstone" => Some(Self::Grindstone),
            "stonecutter" => Some(Self::Stonecutter),
            "smithing_table" => Some(Self::Smithing),
            _ => None,
        }
    }

    /// Number of input slots.
    #[must_use]
    pub fn inputs(self) -> usize {
        match self {
            Self::Anvil | Self::Grindstone => 2,
            Self::Stonecutter => 1,
            Self::Smithing => 3,
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Anvil => "Repair & Name",
            Self::Grindstone => "Repair & Disenchant",
            Self::Stonecutter => "Stonecutter",
            Self::Smithing => "Upgrade Gear",
        }
    }
}

/// What taking the result costs and uses up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub result: Stack,
    /// Experience levels (anvil).
    pub levels: u32,
    /// How many of each input slot are consumed.
    pub consume: [u8; 3],
}

/// Repair material for a damageable item (minecraft.wiki "Anvil" → repair).
#[must_use]
pub fn repair_material(name: &str) -> &'static [&'static str] {
    let tier = name.split('_').next().unwrap_or("");
    match tier {
        "wooden" => &[
            "oak_planks",
            "spruce_planks",
            "birch_planks",
            "jungle_planks",
            "acacia_planks",
            "dark_oak_planks",
            "mangrove_planks",
            "cherry_planks",
            "bamboo_planks",
            "crimson_planks",
            "warped_planks",
        ],
        "stone" => &["cobblestone", "cobbled_deepslate", "blackstone"],
        "iron" | "chainmail" => &["iron_ingot"],
        "golden" => &["gold_ingot"],
        "diamond" => &["diamond"],
        "netherite" => &["netherite_ingot"],
        "leather" => &["leather"],
        "turtle" => &["turtle_scute"],
        _ => match name {
            "elytra" => &["phantom_membrane"],
            "shield" => &[
                "oak_planks",
                "spruce_planks",
                "birch_planks",
                "jungle_planks",
                "acacia_planks",
                "dark_oak_planks",
                "mangrove_planks",
                "cherry_planks",
                "bamboo_planks",
                "crimson_planks",
                "warped_planks",
            ],
            "mace" => &["breeze_rod"],
            "wolf_armor" => &["armadillo_scute"],
            _ => &[],
        },
    }
}

/// The result of the station for `inputs` (and the chosen stonecutter
/// recipe), if any.
#[must_use]
pub fn outcome(st: Station, inputs: &[Option<Stack>], selected: Option<usize>) -> Option<Outcome> {
    let it = items();
    match st {
        Station::Anvil => {
            let a = inputs.first().copied().flatten()?;
            let b = inputs.get(1).copied().flatten()?;
            let max = it.durability[a.item as usize];
            if max == 0 || a.count != 1 {
                return None;
            }
            if b.item == a.item {
                // Combine: both durabilities plus 12 % of the maximum.
                if a.damage == 0 {
                    return None;
                }
                let left = |s: Stack| u32::from(max - s.damage.min(max));
                let dur = (left(a) + left(b) + u32::from(max) * 12 / 100).min(u32::from(max));
                return Some(Outcome {
                    result: Stack {
                        damage: max - dur as u16,
                        ..a
                    },
                    levels: 2,
                    consume: [1, 1, 0],
                });
            }
            let mats = repair_material(it.name(a.item));
            if !mats.contains(&it.name(b.item)) || a.damage == 0 {
                return None;
            }
            // Each unit restores a quarter of the maximum durability.
            let quarter = (max / 4).max(1);
            let units = a.damage.div_ceil(quarter).min(u16::from(b.count)) as u8;
            Some(Outcome {
                result: Stack {
                    damage: a.damage.saturating_sub(quarter * u16::from(units)),
                    ..a
                },
                levels: u32::from(units),
                consume: [1, units, 0],
            })
        }
        Station::Grindstone => {
            let (a, b) = (
                inputs.first().copied().flatten(),
                inputs.get(1).copied().flatten(),
            );
            match (a, b) {
                (Some(a), Some(b)) if a.item == b.item && it.durability[a.item as usize] > 0 => {
                    // Combine: both durabilities plus 5 % of the maximum.
                    let max = u32::from(it.durability[a.item as usize]);
                    let left = |s: Stack| max - u32::from(s.damage).min(max);
                    let dur = (left(a) + left(b) + max * 5 / 100).min(max);
                    Some(Outcome {
                        result: Stack {
                            damage: (max - dur) as u16,
                            count: 1,
                            ..a
                        },
                        levels: 0,
                        consume: [1, 1, 0],
                    })
                }
                _ => None,
            }
        }
        Station::Stonecutter => {
            let a = inputs.first().copied().flatten()?;
            let opts = stonecutter_options(a);
            let r = *opts.get(selected?)?;
            Some(Outcome {
                result: r,
                levels: 0,
                consume: [1, 0, 0],
            })
        }
        Station::Smithing => {
            let t = inputs.first().copied().flatten()?;
            let base = inputs.get(1).copied().flatten()?;
            let add = inputs.get(2).copied().flatten()?;
            let r = recipes().smithing.iter().find(|r| {
                r.template.contains(&t.item)
                    && r.base.contains(&base.item)
                    && r.addition.contains(&add.item)
            })?;
            Some(Outcome {
                result: Stack {
                    item: r.result.item,
                    count: r.result.count,
                    // The upgrade keeps the item's wear.
                    damage: base.damage,
                },
                levels: 0,
                consume: [1, 1, 1],
            })
        }
    }
}

/// Stonecutter results for `input`, in recipe-file order (vanilla sorts by
/// result id).
#[must_use]
pub fn stonecutter_options(input: Stack) -> Vec<Stack> {
    let mut v: Vec<Stack> = recipes()
        .stonecutting
        .iter()
        .filter(|(ing, _)| ing.contains(&input.item))
        .map(|(_, r)| *r)
        .collect();
    v.sort_by_key(|s| items().name(s.item).to_owned());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str, count: u8, damage: u16) -> Option<Stack> {
        Some(Stack {
            item: items().id(name).unwrap(),
            count,
            damage,
        })
    }

    #[test]
    fn anvil_repairs_with_material_and_combines() {
        // Iron pickaxe: 250 durability, a quarter is 62.
        let o = outcome(
            Station::Anvil,
            &[stack("iron_pickaxe", 1, 200), stack("iron_ingot", 5, 0)],
            None,
        )
        .unwrap();
        assert_eq!(o.consume[1], 4);
        assert_eq!(o.result.damage, 0);
        assert_eq!(o.levels, 4);
        let o = outcome(
            Station::Anvil,
            &[stack("iron_pickaxe", 1, 100), stack("iron_pickaxe", 1, 100)],
            None,
        )
        .unwrap();
        // 150 + 150 + 30 capped at 250.
        assert_eq!(o.result.damage, 0);
        assert!(
            outcome(
                Station::Anvil,
                &[stack("iron_pickaxe", 1, 10), stack("gold_ingot", 1, 0)],
                None
            )
            .is_none()
        );
    }

    #[test]
    fn stonecutter_and_smithing() {
        let opts = stonecutter_options(stack("andesite", 1, 0).unwrap());
        assert!(
            opts.iter()
                .any(|s| items().name(s.item) == "andesite_slab" && s.count == 2)
        );
        let o = outcome(
            Station::Smithing,
            &[
                stack("netherite_upgrade_smithing_template", 1, 0),
                stack("diamond_sword", 1, 30),
                stack("netherite_ingot", 1, 0),
            ],
            None,
        )
        .unwrap();
        assert_eq!(items().name(o.result.item), "netherite_sword");
        assert_eq!(o.result.damage, 30);
    }
}
