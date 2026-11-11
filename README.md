# integratedtunnels (Horizon port)

A Horizon port of [integratedtunnels](CyclopsMC/IntegratedTunnels) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- An **Item Importer** part on a tunnel cable face pulls items out of the inventory
- behind that face into the cable network's slotless item store at the jar's rate
- (64 items per part per tick), an **Item Exporter** part on another cable face
- hands them into the inventory behind *its* face, and the cable's
- `/integratedtunnels info` readout shows the parts, the store and the status. It
- is upstream's `import_all_items`/`exporter_item` pair without the variable-card
- `integratedtunnels:tunnel_cable`** — a 4 px non-occluding core (hardness
- ), six `part_<face>` enum properties (`none|importer_item|exporter_item`),

## Not yet ported

- the **variable/aspect layer**: variable cards, the aspect selection GUI, and
- the other six item aspects (integer count, slot index, itemstack, list,
- predicate, NBT) — the whole point of the mod's "advanced" behaviour;
- the **fluid and energy** parts and the fluid/energy network channels;
- the **world parts** (`importer/exporter_world_item|fluid|energy|block`) and
- the block break/place handlers (the `ExtendedFakePlayer`, the
- `dummy_pickaxe`);
- the **interface** parts (item/fluid/energy and their filtering variants) and

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: CyclopsMC/IntegratedTunnels. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
