// Game-side glue for Integrated Tunnels slice 1 (injected into `crate::game` as
// `integratedtunnels_state`; it needs the game, so it is not part of the
// standalone library).
//
// Upstream a tunnel part is a part on an Integrated Dynamics cable: the cable's
// block entity holds one part per face, an Item Importer moves items from the
// inventory on its face into the network's slotless item channel, and an Item
// Exporter moves them from that channel into the inventory on its face
// (`PartTypeImporterItem`, `TunnelAspectWriteBuilders.Item.PROP_IMPORT/EXPORT`).
// There is no cross-mod API at this pin and two packs cannot share a namespace,
// so this crate re-implements the cable it needs: a `integratedtunnels:tunnel_cable`
// carrier whose six `part_<face>` state properties hold the parts, whose block
// entity holds the network's item store, and whose level-tick end runs the
// network's transfers.
//
// A block entity's own tick must not write its NBT (the engine reverts it), so
// the tick only queues the cable; the store is read and written at
// `level_tick_post`. The item store lives on the network's root cable (the
// lowest position), so every cable of one network agrees without extra state.

use std::collections::BTreeSet;
use std::sync::Mutex;

use horizon_client::containers::{BlockEntity, ModBlockEntity};
use horizon_client::items::ItemStack;
use horizon_client::text::Text;
use horizon_core::geometry::Direction;
use horizon_world::nbt::{Compound, JavaString, Tag};

use crate::game::integratedtunnels::{
    FACE_COUNT, FACE_NAMES, ITEM_RATE, ImportMove, Part, Parts, SlotView, Store,
    collect_network, commit_export, commit_import, describe_store, face_of_normal, opposite,
    plan_export, plan_import, root, status_for,
};
use crate::game::native_hooks::flags;
use crate::game::native_server::{NativeCommand, NativeServerPlayer};

/// The tunnel cable carrier block (and its item).
pub(super) const CABLE: &str = "integratedtunnels:tunnel_cable";
/// The Item Importer part item.
pub(super) const IMPORTER: &str = "integratedtunnels:part_importer_item";
/// The Item Exporter part item.
pub(super) const EXPORTER: &str = "integratedtunnels:part_exporter_item";
/// The network item store, saved in the root cable's block entity: a list of
/// `{item, count}` compounds, in first-arrival order.
const STORE_TAG: &str = "store";
const ITEM_TAG: &str = "item";
const COUNT_TAG: &str = "count";
/// A safety cap on one network's cable count (upstream has no limit; a runaway
/// build must not stall the level tick).
const MAX_NETWORK: usize = 1024;

#[derive(Default)]
struct State {
    /// Cables whose block entity ticked this tick, processed at its end.
    cables: BTreeSet<[i32; 3]>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn key(name: &str) -> JavaString {
    JavaString::from(name)
}

/// The block on the `face` side of `pos`.
fn neighbour(pos: [i32; 3], face: usize) -> [i32; 3] {
    let [dx, dy, dz] = Direction::ALL.get(face).copied().unwrap_or(Direction::Down).normal();
    [pos[0] + dx, pos[1] + dy, pos[2] + dz]
}

/// The part property of `face` (`part_down` … `part_east`).
fn face_property(face: usize) -> String {
    format!("part_{}", FACE_NAMES[face])
}

/// The parts a cable carries, or `None` for another block.
fn parts_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> Option<Parts> {
    if game.native_block(pos) != CABLE {
        return None;
    }
    let mut parts = Parts::new();
    for face in 0..FACE_COUNT {
        let value = game.native_block_property(pos, &face_property(face))?;
        parts = parts.with(face, Part::from_name(&value)?);
    }
    Some(parts)
}

/// The network's item store, as saved in the root cable's block entity.
fn store_at(world: &horizon_client::world::World, pos: [i32; 3]) -> Store {
    let Some(BlockEntity::Mod(entity)) = world.block_entities.get(&(pos[0], pos[1], pos[2]))
    else {
        return Store::new();
    };
    if entity.id != CABLE {
        return Store::new();
    }
    let Some(Tag::List(entries)) = entity.nbt.get(&key(STORE_TAG)) else {
        return Store::new();
    };
    let mut store = Store::new();
    for entry in entries {
        let Tag::Compound(entry) = entry else { continue };
        let Some(Tag::String(item)) = entry.get(&key(ITEM_TAG)) else {
            continue;
        };
        let Some(Tag::Int(count)) = entry.get(&key(COUNT_TAG)) else {
            continue;
        };
        if *count > 0 {
            store.add(&item.to_string_lossy(), *count as u32);
        }
    }
    store
}

/// The store as NBT (the shape [`store_at`] reads back).
fn store_tag(store: &Store) -> Tag {
    let entries = store
        .entries()
        .iter()
        .map(|stack| {
            let mut entry = Compound::new();
            entry.insert(key(ITEM_TAG), Tag::String(JavaString::from(&*stack.item)));
            entry.insert(key(COUNT_TAG), Tag::Int(stack.count as i32));
            Tag::Compound(entry)
        })
        .collect();
    Tag::List(entries)
}

/// Save a network's item store on its root cable; answers whether it changed.
fn set_store(game: &mut crate::game::Game<'_>, pos: [i32; 3], store: &Store) -> bool {
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
    else {
        return false;
    };
    if entity.id != CABLE {
        return false;
    }
    let tag = store_tag(store);
    if entity.nbt.get(&key(STORE_TAG)) == Some(&tag) {
        return false;
    }
    entity.nbt.insert(key(STORE_TAG), tag);
    game.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    true
}

/// Show the network's part status on a cable (the part models' active/inactive
/// texture); a no-op when it is already right.
fn show_status(game: &mut crate::game::Game<'_>, pos: [i32; 3], parts: Parts, store: &Store) {
    let status = status_for(parts, store).name();
    if game.native_block_property(pos, "status").as_deref() != Some(status) {
        game.native_set_block_property_with(pos, "status", status, flags::UPDATE_CLIENTS);
    }
}

/// One importer's move this tick: up to [`ITEM_RATE`] items from the first slot
/// the face may take from, into the network's store.
fn import_once(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    cable: [i32; 3],
    face: usize,
    store: &mut Store,
) {
    let target = neighbour(cable, face);
    // The neighbouring block's side that touches the part.
    let side = Direction::ALL[opposite(face)];
    let Some(slots) = game.native_container_slots(dimension, target, side) else {
        return;
    };
    let views: Vec<SlotView> = slots
        .iter()
        .filter_map(|slot| {
            let stack = slot.stack.as_ref()?;
            Some(SlotView {
                slot: slot.slot,
                item: stack.item.to_string(),
                count: stack.count,
                can_take: slot.can_take,
            })
        })
        .collect();
    let Some(plan) = plan_import(&views, ITEM_RATE) else {
        return;
    };
    let Some(taken) = game.native_extract_item(dimension, target, side, plan.slot, plan.count) else {
        return;
    };
    // The engine is the truth about what actually left the container.
    commit_import(
        store,
        &ImportMove {
            slot: plan.slot,
            item: taken.item.to_string(),
            count: taken.count,
        },
    );
}

/// One exporter's move this tick: up to [`ITEM_RATE`] items from the store into
/// the inventory on its face. What the destination refuses stays in the store.
fn export_once(
    game: &mut crate::game::Game<'_>,
    cable: [i32; 3],
    face: usize,
    store: &mut Store,
) {
    let Some(plan) = plan_export(store, ITEM_RATE) else {
        return;
    };
    let target = neighbour(cable, face);
    let side = Direction::ALL[opposite(face)];
    let stack = ItemStack {
        item: plan.item.as_str().into(),
        count: plan.count,
        damage: 0,
        extra: None,
    };
    let leftover = game
        .native_insert_item(target, side, stack)
        .map_or(0, |rest| rest.count);
    commit_export(store, &plan, plan.count.saturating_sub(leftover));
}

/// A placed tunnel cable gets a block entity for the network's item store
/// (`native_block_entity`, API 15).
#[allow(private_interfaces)]
pub(super) fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if block == CABLE && entity.is_none() {
        *entity = Some((CABLE.to_string(), Compound::new()));
    }
}

/// Every tunnel cable ticks on the server (`native_block_entity_ticks`, API 15).
#[allow(private_interfaces)]
pub(super) fn on_block_entity_ticks(block: &str, ticks: &mut bool) {
    if block == CABLE {
        *ticks = true;
    }
}

/// One cable tick: queue the cable for the level tick's end. The tick itself
/// writes nothing (the engine reverts block-entity writes made here).
#[allow(private_interfaces)]
pub(super) fn on_block_entity_tick(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    _entity: &ModBlockEntity,
) {
    if game.native_block(pos) == CABLE {
        state().get_or_insert_with(State::default).cables.insert(pos);
    }
}

/// After the level's tick: run every queued cable network's transfers. Every
/// importer takes from the inventory on its face into the network's store, then
/// every exporter hands the store on, so items imported this tick can leave the
/// same tick (as upstream's parts, each written once per tick).
#[allow(private_interfaces)]
pub(super) fn on_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    let queued = {
        let mut guard = state();
        guard
            .as_mut()
            .map(|state| std::mem::take(&mut state.cables))
            .unwrap_or_default()
    };
    if queued.is_empty() {
        return;
    }
    let mut done: BTreeSet<[i32; 3]> = BTreeSet::new();
    for start in queued {
        if done.contains(&start) || game.native_block(start) != CABLE {
            continue;
        }
        let network = collect_network(
            start,
            |pos| game.native_block(pos) == CABLE,
            MAX_NETWORK,
        );
        for &pos in &network {
            done.insert(pos);
        }
        let Some(store_pos) = root(&network) else {
            continue;
        };
        let mut store = store_at(&game.world, store_pos);
        for &pos in &network {
            let Some(parts) = parts_at(game, pos) else {
                continue;
            };
            for face in parts.faces(Part::ImporterItem) {
                import_once(game, dimension, pos, face, &mut store);
            }
        }
        for &pos in &network {
            let Some(parts) = parts_at(game, pos) else {
                continue;
            };
            for face in parts.faces(Part::ExporterItem) {
                export_once(game, pos, face, &mut store);
            }
        }
        set_store(game, store_pos, &store);
        for &pos in &network {
            let Some(parts) = parts_at(game, pos) else {
                continue;
            };
            show_status(game, pos, parts, &store);
        }
    }
}

/// Right-clicking a cable face with a part item places that part on the face
/// (upstream's `ItemPart.useOn` + `PartHelpers.addPart`); the item is consumed
/// outside creative. A face that already holds a part is left for the
/// (unported) screen.
#[allow(private_interfaces)]
pub(super) fn on_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    face_normal: [i32; 3],
    canceled: &mut bool,
) {
    if game.native_block(pos) != CABLE {
        return;
    }
    let Some(face) = face_of_normal(face_normal) else {
        return;
    };
    let Some((held, _, _)) = game.native_held() else {
        return;
    };
    let part = match held.as_str() {
        IMPORTER => Part::ImporterItem,
        EXPORTER => Part::ExporterItem,
        _ => return,
    };
    let property = face_property(face);
    if game.native_block_property(pos, &property).as_deref() != Some("none") {
        *canceled = true;
        game.native_feedback(Text::literal(format!(
            "Integrated Tunnels: this cable face already holds a part ({})",
            FACE_NAMES[face]
        )));
        return;
    }
    game.native_set_block_property_with(pos, &property, part.name(), flags::UPDATE_CLIENTS);
    if game
        .native_acting_player()
        .is_none_or(|player| player.game_mode != 1)
    {
        game.native_take_items(&held, 1);
    }
    *canceled = true;
    game.native_feedback(Text::literal(format!(
        "Integrated Tunnels: {} on the {} side",
        part.name(),
        FACE_NAMES[face]
    )));
}

/// Declare `/integratedtunnels info|part`.
#[allow(private_interfaces)]
pub(super) fn on_register_commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::new("integratedtunnels info <pos:block_pos>"));
    commands.push(NativeCommand::new(
        "integratedtunnels part <pos:block_pos> <face:word> <part:word>",
    ));
}

/// The parts of a cable as `down=none,north=importer_item`.
fn parts_line(parts: Parts) -> String {
    (0..FACE_COUNT)
        .map(|face| {
            format!(
                "{}={}",
                FACE_NAMES[face],
                parts.get(face).unwrap_or(Part::None).name()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_pos(words: &[&str; 3]) -> Option<[i32; 3]> {
    Some([
        words[0].parse().ok()?,
        words[1].parse().ok()?,
        words[2].parse().ok()?,
    ])
}

/// Run `/integratedtunnels info <x> <y> <z>` (the harness readout: parts, the
/// network's item store and its status) or `/integratedtunnels part <x> <y> <z>
/// <face> <none|importer_item|exporter_item>` (the harness path for
/// placing/removing a part, since `ctl interact` only reaches a visible face).
#[allow(private_interfaces)]
pub(super) fn on_perform_command(
    game: &mut crate::game::Game<'_>,
    _player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    match words.as_slice() {
        ["integratedtunnels", "info", x, y, z] => {
            *ran = true;
            let Some(pos) = parse_pos(&[*x, *y, *z]) else {
                game.native_feedback(Text::literal(
                    "Usage: /integratedtunnels info <x> <y> <z>",
                ));
                return;
            };
            let Some(parts) = parts_at(game, pos) else {
                game.native_feedback(Text::literal("Integrated Tunnels: no tunnel cable there."));
                return;
            };
            game.native_feedback(Text::literal(format!(
                "Tunnel Cable at {x},{y},{z}: parts {}",
                parts_line(parts)
            )));
            let network = collect_network(pos, |other| game.native_block(other) == CABLE, MAX_NETWORK);
            let store = root(&network)
                .map(|store_pos| store_at(&game.world, store_pos))
                .unwrap_or_default();
            let status = status_for(parts, &store);
            game.native_feedback(Text::literal(format!(
                "Tunnel Cable at {x},{y},{z}: store {}; status {}",
                describe_store(&store),
                status.name()
            )));
        }
        ["integratedtunnels", "part", x, y, z, face, part] => {
            *ran = true;
            let Some(pos) = parse_pos(&[*x, *y, *z]) else {
                game.native_feedback(Text::literal(
                    "Usage: /integratedtunnels part <x> <y> <z> <face> <none|importer_item|exporter_item>",
                ));
                return;
            };
            let Some(face) = FACE_NAMES.iter().position(|name| name == face) else {
                game.native_feedback(Text::literal(format!(
                    "Integrated Tunnels: unknown face '{face}' (down|up|north|south|west|east)"
                )));
                return;
            };
            if game.native_block(pos) != CABLE {
                game.native_feedback(Text::literal("Integrated Tunnels: no tunnel cable there."));
                return;
            }
            let Some(part) = Part::from_name(part) else {
                game.native_feedback(Text::literal(format!(
                    "Integrated Tunnels: unknown part '{part}' (none|importer_item|exporter_item)"
                )));
                return;
            };
            game.native_set_block_property_with(pos, &face_property(face), part.name(), flags::UPDATE_CLIENTS);
            game.native_feedback(Text::literal(format!(
                "Tunnel Cable at {x},{y},{z}: {} on the {} side",
                part.name(),
                FACE_NAMES[face]
            )));
        }
        ["integratedtunnels", ..] => {
            *ran = true;
            game.native_feedback(Text::literal(
                "Usage: /integratedtunnels info <x> <y> <z> | part <x> <y> <z> <face> <part>",
            ));
        }
        _ => {}
    }
}
