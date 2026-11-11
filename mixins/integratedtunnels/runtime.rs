// Pure logic for Integrated Tunnels slice 1: the tunnel cable's six part slots,
// the connected network, and the item store an Item Importer fills and an Item
// Exporter empties.
//
// This module has no world access: `support.rs` reads the cable's state and
// block entity, reads the container behind a part's face through the pin's
// container helpers, and calls these rules. The face order is Horizon's
// `Direction` order (`down, up, north, south, west, east`), so a face index maps
// straight onto the engine's direction.
//
// Upstream (`PartTypeImporterItem`, `PartTypeExporterItem`,
// `TunnelAspectWriteBuilders.Item.PROP_IMPORT/PROP_EXPORT`) has an importer move
// up to `PROP_RATE` items from the inventory on its face into the network's
// slotless item channel, and an exporter move up to `PROP_RATE` items from that
// channel into the inventory on its face. `PROPERTIES_RATESLOT` defaults the rate
// to 64, the channel to 0, the slot to -1 (any) and round-robin to off.

/// The six faces of a tunnel cable, in `Direction` order.
pub const FACE_COUNT: usize = 6;

/// Face names, in `Direction` order (the blockstate property names).
pub const FACE_NAMES: [&str; FACE_COUNT] = ["down", "up", "north", "south", "west", "east"];

/// The items one part moves in one tick: upstream's `PROPERTIES_RATESLOT`
/// default for `PROP_RATE` (`TunnelAspectWriteBuilders.java`, "64").
pub const ITEM_RATE: u32 = 64;

/// How many stacks [`describe_store`] names before it stops listing with `…`.
const NAMED_STACKS: usize = 8;

/// A part installed on one face of a tunnel cable, by the value its blockstate
/// property holds. Upstream's cable block entity holds one part per face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    /// No part on this face.
    #[default]
    None,
    /// An Item Importer: pulls from the inventory behind its face into the store.
    ImporterItem,
    /// An Item Exporter: pushes the store into the inventory behind its face.
    ExporterItem,
}

impl Part {
    /// The part a blockstate property value names; `None` for an unknown value.
    pub fn from_name(name: &str) -> Option<Part> {
        match name {
            "none" => Some(Part::None),
            "importer_item" => Some(Part::ImporterItem),
            "exporter_item" => Some(Part::ExporterItem),
            _ => None,
        }
    }

    /// The blockstate property value for this part.
    pub fn name(self) -> &'static str {
        match self {
            Part::None => "none",
            Part::ImporterItem => "importer_item",
            Part::ExporterItem => "exporter_item",
        }
    }
}

/// The six part slots of one cable, in [`FACE_NAMES`] order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts([Part; FACE_COUNT]);

impl Parts {
    /// No parts on any face.
    pub fn new() -> Parts {
        Parts::default()
    }

    /// The part on `face`, or `None` for an out-of-range face.
    pub fn get(self, face: usize) -> Option<Part> {
        self.0.get(face).copied()
    }

    /// The same cable with `part` on `face` (out-of-range faces are ignored).
    pub fn with(self, face: usize, part: Part) -> Parts {
        let mut slots = self.0;
        if let Some(slot) = slots.get_mut(face) {
            *slot = part;
        }
        Parts(slots)
    }

    /// Whether any face carries any part.
    pub fn has_part(self) -> bool {
        self.0.iter().any(|part| *part != Part::None)
    }

    /// The faces carrying `part`, in face order.
    pub fn faces(self, part: Part) -> impl Iterator<Item = usize> {
        self.0
            .into_iter()
            .enumerate()
            .filter(move |(_, slot)| *slot == part)
            .map(|(face, _)| face)
    }
}

/// A part's status, as upstream's `PartStateWriterBase` reports it: active while
/// the part has work on the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Inactive,
    Active,
}

impl Status {
    /// The blockstate property value.
    pub fn name(self) -> &'static str {
        match self {
            Status::Inactive => "inactive",
            Status::Active => "active",
        }
    }
}

/// A block position in the level.
pub type Pos = [i32; 3];

/// The direction offset of `face` in [`FACE_NAMES`] order.
pub fn face_normal(face: usize) -> [i32; 3] {
    match face {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        3 => [0, 0, 1],
        4 => [-1, 0, 0],
        _ => [1, 0, 0],
    }
}

/// The face whose normal is `normal`, or `None` for a zero/unknown normal.
pub fn face_of_normal(normal: [i32; 3]) -> Option<usize> {
    (0..FACE_COUNT).find(|&face| face_normal(face) == normal)
}

/// The face opposite `face` (the side of the neighbouring block that faces back).
pub fn opposite(face: usize) -> usize {
    match face {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        4 => 5,
        _ => 4,
    }
}

/// The six positions touching `pos`, in [`FACE_NAMES`] order.
pub fn neighbours(pos: Pos) -> [Pos; FACE_COUNT] {
    std::array::from_fn(|face| {
        let [dx, dy, dz] = face_normal(face);
        [pos[0] + dx, pos[1] + dy, pos[2] + dz]
    })
}

/// The connected cable set starting at `start`: `is_cable` decides whether a
/// position joins it, and at most `limit` positions are collected (upstream's
/// network has no size limit; a runaway build must not stall the level tick).
pub fn collect_network(
    start: Pos,
    mut is_cable: impl FnMut(Pos) -> bool,
    limit: usize,
) -> Vec<Pos> {
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    seen.insert(start);
    queue.push_back(start);
    let mut out = Vec::new();
    while let Some(pos) = queue.pop_front() {
        if out.len() >= limit {
            break;
        }
        out.push(pos);
        for next in neighbours(pos) {
            if seen.len() >= limit {
                break;
            }
            if seen.insert(next) && is_cable(next) {
                queue.push_back(next);
            }
        }
    }
    out
}

/// The cable a network's item store lives on: the lowest position, so every
/// cable of one network agrees on it without extra state.
pub fn root(network: &[Pos]) -> Option<Pos> {
    network.iter().min().copied()
}

/// One stored stack: an item id and how many of it the network holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: u32,
}

/// A network's slotless item store (upstream's
/// `IPositionedAddonsNetworkIngredients` channel): one entry per item id, in the
/// order the items first arrived.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Store {
    entries: Vec<Stack>,
}

impl Store {
    /// An empty store.
    pub fn new() -> Store {
        Store::default()
    }

    /// The stored stacks, in the order the items first arrived.
    pub fn entries(&self) -> &[Stack] {
        &self.entries
    }

    /// How many items the store holds in total.
    pub fn total(&self) -> u32 {
        self.entries.iter().map(|stack| stack.count).sum()
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Adds `count` of `item`, merging into the entry that item already has (its
    /// place in the order stays); answers the item's new total.
    pub fn add(&mut self, item: &str, count: u32) -> u32 {
        if let Some(stack) = self.entries.iter_mut().find(|stack| stack.item == item) {
            stack.count = stack.count.saturating_add(count);
            return stack.count;
        }
        self.entries.push(Stack {
            item: item.to_owned(),
            count,
        });
        count
    }

    /// Removes up to `count` of `item`; answers how many were removed. An entry
    /// that reaches zero is dropped.
    pub fn remove(&mut self, item: &str, count: u32) -> u32 {
        let Some(index) = self.entries.iter().position(|stack| stack.item == item) else {
            return 0;
        };
        let taken = count.min(self.entries[index].count);
        self.entries[index].count -= taken;
        if self.entries[index].count == 0 {
            self.entries.remove(index);
        }
        taken
    }
}

/// A slot of the container behind a part's face, as the engine reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotView {
    /// The slot's number, for the engine's extract helper.
    pub slot: usize,
    /// The item in it.
    pub item: String,
    /// How many.
    pub count: u32,
    /// Whether this side may take from it.
    pub can_take: bool,
}

/// The move one importer makes this tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMove {
    pub slot: usize,
    pub item: String,
    pub count: u32,
}

/// The move one exporter makes this tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportMove {
    pub item: String,
    pub count: u32,
}

/// What an importer takes this tick: the first slot the side may take from, up
/// to `rate` items (upstream takes from the first matching slot, `PROP_SLOT`
/// -1, no round-robin).
pub fn plan_import(slots: &[SlotView], rate: u32) -> Option<ImportMove> {
    if rate == 0 {
        return None;
    }
    let slot = slots.iter().find(|slot| slot.can_take && slot.count > 0)?;
    Some(ImportMove {
        slot: slot.slot,
        item: slot.item.clone(),
        count: slot.count.min(rate),
    })
}

/// Records items the engine actually took from the source container; answers the
/// store's total for that item.
pub fn commit_import(store: &mut Store, moved: &ImportMove) -> u32 {
    store.add(&moved.item, moved.count)
}

/// What an exporter pushes this tick: the store's first stack, up to `rate`.
pub fn plan_export(store: &Store, rate: u32) -> Option<ExportMove> {
    if rate == 0 {
        return None;
    }
    let stack = store.entries().first()?;
    Some(ExportMove {
        item: stack.item.clone(),
        count: stack.count.min(rate),
    })
}

/// Records what the destination accepted; answers how many items left the store.
pub fn commit_export(store: &mut Store, moved: &ExportMove, inserted: u32) -> u32 {
    store.remove(&moved.item, inserted.min(moved.count))
}

/// A cable's part status: active while it carries a part and the network holds
/// items to move, as upstream's part is active while its aspect has work.
pub fn status_for(parts: Parts, store: &Store) -> Status {
    if parts.has_part() && !store.is_empty() {
        Status::Active
    } else {
        Status::Inactive
    }
}

/// The store as the `/integratedtunnels info` readout prints it, e.g.
/// `2 stacks, 128 items (minecraft:cobblestone x128, minecraft:stick x64)`;
/// at most eight stacks are named, then `, …`.
pub fn describe_store(store: &Store) -> String {
    let stacks = store.entries().len();
    if stacks == 0 {
        return "0 items".to_owned();
    }
    let named = store
        .entries()
        .iter()
        .take(NAMED_STACKS)
        .map(|stack| format!("{} x{}", stack.item, stack.count))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if stacks > NAMED_STACKS { ", …" } else { "" };
    let stack_word = if stacks == 1 { "stack" } else { "stacks" };
    let total = store.total();
    let item_word = if total == 1 { "item" } else { "items" };
    format!("{stacks} {stack_word}, {total} {item_word} ({named}{more})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_round_trip() {
        for part in [Part::None, Part::ImporterItem, Part::ExporterItem] {
            assert_eq!(Part::from_name(part.name()), Some(part));
        }
        // Another mod's part value is not ours.
        assert_eq!(Part::from_name("reader"), None);
        assert_eq!(Part::from_name(""), None);
    }

    #[test]
    fn a_part_is_placed_on_one_face_only() {
        let parts = Parts::new()
            .with(5, Part::ImporterItem)
            .with(3, Part::ExporterItem);
        assert_eq!(parts.get(5), Some(Part::ImporterItem));
        assert_eq!(parts.get(3), Some(Part::ExporterItem));
        assert_eq!(parts.get(0), Some(Part::None));
        assert!(parts.has_part());
        assert!(!Parts::new().has_part());
        // Replacing a face keeps the others; out-of-range faces do nothing.
        let replaced = parts.with(5, Part::None);
        assert_eq!(replaced.get(5), Some(Part::None));
        assert_eq!(replaced.get(3), Some(Part::ExporterItem));
        assert_eq!(parts.with(9, Part::ExporterItem), parts);
        assert_eq!(parts.get(9), None);
    }

    #[test]
    fn each_part_kind_lists_its_faces_in_face_order() {
        let parts = Parts::new()
            .with(5, Part::ImporterItem)
            .with(1, Part::ExporterItem)
            .with(2, Part::ImporterItem);
        assert_eq!(
            parts.faces(Part::ImporterItem).collect::<Vec<_>>(),
            vec![2, 5]
        );
        assert_eq!(parts.faces(Part::ExporterItem).collect::<Vec<_>>(), vec![1]);
        assert_eq!(Parts::new().faces(Part::None).count(), FACE_COUNT);
        assert_eq!(Parts::new().faces(Part::ImporterItem).next(), None);
    }

    #[test]
    fn faces_are_the_engine_direction_order() {
        assert_eq!(FACE_NAMES, ["down", "up", "north", "south", "west", "east"]);
        assert_eq!(face_normal(0), [0, -1, 0]);
        assert_eq!(face_normal(1), [0, 1, 0]);
        assert_eq!(face_normal(2), [0, 0, -1]);
        assert_eq!(face_normal(3), [0, 0, 1]);
        assert_eq!(face_normal(4), [-1, 0, 0]);
        assert_eq!(face_normal(5), [1, 0, 0]);
        for face in 0..FACE_COUNT {
            assert_eq!(face_of_normal(face_normal(face)), Some(face));
        }
        assert_eq!(face_of_normal([0, 0, 0]), None);
    }

    #[test]
    fn the_opposite_face_is_the_side_facing_back() {
        for face in 0..FACE_COUNT {
            let back = opposite(face);
            assert_eq!(
                face_normal(back),
                face_normal(face).map(|step| -step),
                "face {face}"
            );
            assert_eq!(opposite(back), face);
        }
    }

    #[test]
    fn neighbours_are_the_six_touching_positions() {
        assert_eq!(
            neighbours([0, 0, 0]),
            [
                [0, -1, 0],
                [0, 1, 0],
                [0, 0, -1],
                [0, 0, 1],
                [-1, 0, 0],
                [1, 0, 0]
            ]
        );
    }

    #[test]
    fn the_network_is_the_connected_cable_set() {
        // A line of cables x = 0..=2, with a hole at x = 3.
        let cables = |pos: Pos| pos[1] == 0 && pos[2] == 0 && (0..=2).contains(&pos[0]);
        let mut network = collect_network([0, 0, 0], cables, 64);
        network.sort();
        assert_eq!(network, vec![[0, 0, 0], [1, 0, 0], [2, 0, 0]]);
        assert!(
            !collect_network([0, 0, 0], cables, 64).contains(&[5, 0, 0]),
            "a disconnected cable must not join the network"
        );
    }

    #[test]
    fn the_network_is_bounded_against_a_runaway_loop() {
        // An infinite plane of cables: the safety cap stops the walk.
        assert_eq!(collect_network([0, 0, 0], |_pos| true, 25).len(), 25);
    }

    #[test]
    fn the_root_is_the_lowest_position_whatever_the_order() {
        let network = vec![[2, 0, 0], [-1, 0, 3], [0, 0, 0], [0, -2, 0]];
        assert_eq!(root(&network), Some([-1, 0, 3]));
        assert_eq!(root(&[]), None);
        let mut reversed = network.clone();
        reversed.reverse();
        assert_eq!(root(&reversed), root(&network));
    }

    #[test]
    fn the_store_merges_a_repeated_item_and_keeps_first_seen_order() {
        let mut store = Store::new();
        assert!(store.is_empty());
        assert_eq!(store.add("minecraft:cobblestone", 64), 64);
        assert_eq!(store.add("minecraft:stick", 8), 8);
        assert_eq!(store.add("minecraft:cobblestone", 36), 100);
        assert_eq!(store.total(), 108);
        assert!(!store.is_empty());
        let items: Vec<&str> = store.entries().iter().map(|stack| &*stack.item).collect();
        assert_eq!(items, vec!["minecraft:cobblestone", "minecraft:stick"]);
        assert_eq!(store.entries()[0].count, 100);
    }

    #[test]
    fn removing_from_the_store_drops_an_empty_entry() {
        let mut store = Store::new();
        store.add("minecraft:cobblestone", 100);
        store.add("minecraft:stick", 8);
        assert_eq!(store.remove("minecraft:cobblestone", 40), 40);
        assert_eq!(store.entries()[0].count, 60);
        assert_eq!(store.remove("minecraft:cobblestone", 60), 60);
        assert_eq!(store.total(), 8);
        let items: Vec<&str> = store.entries().iter().map(|stack| &*stack.item).collect();
        assert_eq!(items, vec!["minecraft:stick"]);
        // Removing more than is held removes what is there, and an unknown item
        // removes nothing.
        assert_eq!(store.remove("minecraft:stick", 99), 8);
        assert!(store.is_empty());
        assert_eq!(store.remove("minecraft:stick", 1), 0);
    }

    #[test]
    fn an_importer_takes_the_first_takeable_slot_up_to_its_rate() {
        let slots = vec![
            SlotView {
                slot: 0,
                item: "minecraft:cobblestone".to_owned(),
                count: 100,
                can_take: true,
            },
            SlotView {
                slot: 1,
                item: "minecraft:stick".to_owned(),
                count: 8,
                can_take: true,
            },
        ];
        assert_eq!(
            plan_import(&slots, ITEM_RATE),
            Some(ImportMove {
                slot: 0,
                item: "minecraft:cobblestone".to_owned(),
                count: 64,
            })
        );
        // A slot with less than the rate moves what it holds.
        let small = vec![SlotView {
            slot: 3,
            item: "minecraft:stick".to_owned(),
            count: 5,
            can_take: true,
        }];
        assert_eq!(plan_import(&small, ITEM_RATE).map(|m| m.count), Some(5));
        // A slot the side may not take from is skipped, and empty slots too.
        let refused = vec![
            SlotView {
                slot: 0,
                item: "minecraft:stone".to_owned(),
                count: 9,
                can_take: false,
            },
            SlotView {
                slot: 1,
                item: "minecraft:cobblestone".to_owned(),
                count: 0,
                can_take: true,
            },
            SlotView {
                slot: 2,
                item: "minecraft:dirt".to_owned(),
                count: 2,
                can_take: true,
            },
        ];
        assert_eq!(
            plan_import(&refused, ITEM_RATE).map(|m| (m.slot, m.item, m.count)),
            Some((2, "minecraft:dirt".to_owned(), 2))
        );
        // Nothing to take at all.
        assert_eq!(plan_import(&[], ITEM_RATE), None);
        assert_eq!(plan_import(&refused[..2], ITEM_RATE), None);
    }

    #[test]
    fn a_committed_import_lands_in_the_store() {
        let mut store = Store::new();
        let moved = ImportMove {
            slot: 0,
            item: "minecraft:cobblestone".to_owned(),
            count: 64,
        };
        assert_eq!(commit_import(&mut store, &moved), 64);
        assert_eq!(store.total(), 64);
        assert_eq!(commit_import(&mut store, &moved), 128);
        assert_eq!(store.entries().len(), 1);
    }

    #[test]
    fn an_exporter_pushes_the_first_stored_stack_up_to_its_rate() {
        let mut store = Store::new();
        assert_eq!(plan_export(&store, ITEM_RATE), None);
        store.add("minecraft:cobblestone", 100);
        store.add("minecraft:stick", 8);
        assert_eq!(
            plan_export(&store, ITEM_RATE),
            Some(ExportMove {
                item: "minecraft:cobblestone".to_owned(),
                count: 64,
            })
        );
        // After the first stack is gone the next one is chosen.
        store.remove("minecraft:cobblestone", 100);
        assert_eq!(plan_export(&store, ITEM_RATE).map(|m| m.count), Some(8));
        // A rate above what is stored moves what is there.
        store.remove("minecraft:stick", 8);
        store.add("minecraft:dirt", 5);
        assert_eq!(plan_export(&store, 1000).map(|m| m.count), Some(5));
    }

    #[test]
    fn a_committed_export_takes_only_what_the_target_accepted() {
        let mut store = Store::new();
        store.add("minecraft:cobblestone", 100);
        let moved = ExportMove {
            item: "minecraft:cobblestone".to_owned(),
            count: 64,
        };
        // The destination had room for 10: the rest stays in the network.
        assert_eq!(commit_export(&mut store, &moved, 10), 10);
        assert_eq!(store.total(), 90);
        assert_eq!(commit_export(&mut store, &moved, 64), 64);
        assert_eq!(store.total(), 26);
        // Nothing inserted leaves the store untouched.
        assert_eq!(commit_export(&mut store, &moved, 0), 0);
        assert_eq!(store.total(), 26);
    }

    #[test]
    fn the_status_follows_the_parts_and_the_store() {
        let mut store = Store::new();
        let parts = Parts::new().with(2, Part::ImporterItem);
        assert_eq!(status_for(parts, &store), Status::Inactive);
        store.add("minecraft:cobblestone", 1);
        assert_eq!(status_for(parts, &store), Status::Active);
        // A cable with no part is never active, however full the network is.
        assert_eq!(status_for(Parts::new(), &store), Status::Inactive);
        assert_eq!(Status::Active.name(), "active");
        assert_eq!(Status::Inactive.name(), "inactive");
    }

    #[test]
    fn the_store_line_names_each_stack_and_caps_the_list() {
        let mut store = Store::new();
        assert_eq!(describe_store(&store), "0 items");
        store.add("minecraft:cobblestone", 128);
        store.add("minecraft:stick", 64);
        assert_eq!(
            describe_store(&store),
            "2 stacks, 192 items (minecraft:cobblestone x128, minecraft:stick x64)"
        );
        let mut many = Store::new();
        for index in 0..10 {
            many.add(&format!("minecraft:item_{index}"), 1);
        }
        let line = describe_store(&many);
        assert!(line.starts_with("10 stacks, 10 items (minecraft:item_0 x1"));
        assert!(line.ends_with(", …)"), "{line}");
        assert!(!line.contains("minecraft:item_9"), "{line}");
    }
}
