#!/usr/bin/env python3
"""Assemble slice 1's `content/` folder from the pack jars.

The artefacts are the ATM10 pack files, kept as licence and inventory evidence:

- `ports/_shared/licence-cache/integratedtunnels-1.21.1-neoforge-1.13.0.jar`
  (SHA-256 `9b5bed5e…19e`, MIT) — this mod: the part item models, the part
  geometry, the four part textures and the part lang lines.
- `ports/_shared/licence-cache/integrateddynamics-1.21.1-neoforge-1.38.2.jar`
  (MIT, the parent mod) — the cable core texture, because this port's carrier
  block stands in for that cable, and the part model's `display` transforms.

Nothing is invented: every model, texture, rotation and lang line comes out of
those jars; re-spelled only where 26.3 differs (one `items/<name>.json` model
definition, a singular loot `condition`). The carrier block declaration, its
blockstate, its core model and its loot table are written here because the
carrier is this port's own block (see `FINDINGS.md` §3).

Rerun it to rebuild `content/` (it deletes its own generated folders first):

    python3 tools/assemble_content.py
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT.parent / "_shared/licence-cache"
JAR = CACHE / "integratedtunnels-1.21.1-neoforge-1.13.0.jar"
PARENT_JAR = CACHE / "integrateddynamics-1.21.1-neoforge-1.38.2.jar"
CONTENT = ROOT / "content"
MOD = "integratedtunnels"

JAR_SHA256 = "9b5bed5e69d071bf9266c0d9186bfaa952970493e93defda0cce35edb978e19e"

# The two parts this slice carries, and the status models each has in the jar.
PARTS = ["importer_item", "exporter_item"]
STATUSES = ["active", "inactive"]
# The jar's per-face rotations for a part model (`blockstates/part_<part>.json`):
# the base model sits against the south face, and every other face rotates it.
FACE_ROTATION = {
    "down": {"x": 270},
    "up": {"x": 90},
    "north": {"y": 180},
    "south": {},
    "west": {"y": 90},
    "east": {"y": 270},
}
FACES = ["down", "up", "north", "south", "west", "east"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read(jar: Path, name: str) -> bytes:
    with zipfile.ZipFile(jar) as archive:
        return archive.read(name)


def write(relative: str, data: bytes | str) -> None:
    path = CONTENT / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data.encode() if isinstance(data, str) else data)


def json_file(relative: str, value: object) -> None:
    write(relative, json.dumps(value, indent=2, sort_keys=True) + "\n")


def part_model(part: str, status: str, display: dict, elements: list) -> dict:
    texture = f"{MOD}:part/{part}_{status}"
    return {
        "parent": "minecraft:block/block",
        "display": display,
        "textures": {"particle": texture, "texture": texture},
        "elements": elements,
    }


def blockstate() -> dict:
    multipart = [{"apply": {"model": f"{MOD}:block/tunnel_cable_core"}}]
    for part in PARTS:
        for face in FACES:
            for status in STATUSES:
                apply = {"model": f"{MOD}:block/part_{part}_{status}"}
                apply.update(FACE_ROTATION[face])
                multipart.append(
                    {"when": {f"part_{face}": part, "status": status}, "apply": apply}
                )
    return {"multipart": multipart}


def block_declaration() -> dict:
    values = ["none", *PARTS]
    return {
        "default_state": {
            **{f"part_{face}": "none" for face in FACES},
            "status": "inactive",
        },
        "item": True,
        "properties": {
            "explosion_resistance": 3.0,
            "hardness": 3.0,
            "occludes": False,
            "render_shape": "MODEL",
            "requires_tool": False,
            "sounds": "minecraft:stone",
        },
        # The 4 px core of the cable this port's carrier stands in for.
        "shapes": {"default": [[0.375, 0.375, 0.375, 0.625, 0.625, 0.625]]},
        "state_properties": {
            **{
                f"part_{face}": {"type": "enum", "values": values}
                for face in FACES
            },
            "status": {"type": "enum", "values": ["inactive", "active"]},
        },
        "template": "minecraft:stone",
    }


def main() -> None:
    if sha256(JAR.read_bytes()) != JAR_SHA256:
        raise SystemExit(f"{JAR} is not the pack jar this port was built from")
    for folder in ("assets", "data"):
        shutil.rmtree(CONTENT / folder, ignore_errors=True)

    # Carried byte-for-byte: the four part textures and the cable core texture.
    for part in PARTS:
        for status in STATUSES:
            name = f"assets/{MOD}/textures/part/{part}_{status}.png"
            write(name, read(JAR, f"assets/{MOD}/textures/part/{part}_{status}.png"))
    write(
        f"assets/{MOD}/textures/block/tunnel_cable.png",
        read(PARENT_JAR, "assets/integrateddynamics/textures/block/cable.png"),
    )

    # The part geometry and the item-in-hand transforms, from the two jars.
    connector = json.loads(read(JAR, f"assets/{MOD}/models/block/connector.json"))
    elements = connector["elements"]
    display = json.loads(
        read(PARENT_JAR, "assets/integrateddynamics/models/block/part.json")
    )["display"]
    for part in PARTS:
        for status in STATUSES:
            json_file(
                f"assets/{MOD}/models/block/part_{part}_{status}.json",
                part_model(part, status, display, elements),
            )

    # 26.3 item definitions and item models (the jar's 1.21.1 item models point
    # at `block/connector` + an active part texture; ours point at the carried
    # active block model, so the item keeps the jar's look).
    for part in PARTS:
        json_file(
            f"assets/{MOD}/items/part_{part}.json",
            {"model": {"type": "minecraft:model", "model": f"{MOD}:item/part_{part}"}},
        )
        json_file(
            f"assets/{MOD}/models/item/part_{part}.json",
            {"parent": f"{MOD}:block/part_{part}_active"},
        )
    json_file(
        f"assets/{MOD}/items/tunnel_cable.json",
        {"model": {"type": "minecraft:model", "model": f"{MOD}:item/tunnel_cable"}},
    )
    json_file(
        f"assets/{MOD}/models/item/tunnel_cable.json",
        {"parent": f"{MOD}:block/tunnel_cable_core"},
    )
    json_file(
        f"assets/{MOD}/models/block/tunnel_cable_core.json",
        {
            "parent": "minecraft:block/block",
            "textures": {
                "all": f"{MOD}:block/tunnel_cable",
                "particle": f"{MOD}:block/tunnel_cable",
            },
            "elements": [
                {
                    "from": [6, 6, 6],
                    "to": [10, 10, 10],
                    "faces": {face: {"texture": "#all"} for face in FACES},
                }
            ],
        },
    )
    json_file(f"assets/{MOD}/blockstates/tunnel_cable.json", blockstate())

    upstream_lang = json.loads(read(JAR, f"assets/{MOD}/lang/en_us.json"))
    lang = {
        "block.integratedtunnels.tunnel_cable": "Tunnel Cable",
        "itemGroup.integratedtunnels.integratedtunnels": upstream_lang[
            "itemGroup.integratedtunnels"
        ],
    }
    for part in PARTS:
        key = f"parttype.{MOD}.{part}"
        lang[f"item.{MOD}.part_{part}"] = upstream_lang[key]
        lang[f"{key}.info"] = upstream_lang[f"{key}.info"]
    json_file(f"assets/{MOD}/lang/en_us.json", lang)

    json_file(f"data/{MOD}/horizon/block/tunnel_cable.json", block_declaration())
    for part in PARTS:
        json_file(f"data/{MOD}/horizon/item/part_{part}.json", {"template": "minecraft:paper"})
    json_file(
        f"data/{MOD}/horizon/creative_tab/{MOD}.json",
        {
            "icon": f"{MOD}:tunnel_cable",
            "items": [
                f"{MOD}:tunnel_cable",
                *[f"{MOD}:part_{part}" for part in PARTS],
            ],
            "title": "Integrated Tunnels",
        },
    )
    # 26.3's singular condition: the 1.21.1 `conditions` list silently fails.
    json_file(
        f"data/{MOD}/loot_table/blocks/tunnel_cable.json",
        {
            "type": "minecraft:block",
            "random_sequence": f"{MOD}:blocks/tunnel_cable",
            "pools": [
                {
                    "bonus_rolls": 0.0,
                    "condition": {"type": "minecraft:survives_explosion"},
                    "entries": [{"name": f"{MOD}:tunnel_cable", "type": "minecraft:item"}],
                    "rolls": 1.0,
                }
            ],
        },
    )
    write(
        "horizon-mod.toml",
        "\n".join(
            [
                "# Port-owned content mod for Integrated Tunnels 1.13.0 (MIT,",
                "# CyclopsMC/IntegratedTunnels): the tunnel cable carrier block, the Item",
                "# Importer and Item Exporter part items, their models, textures, lang and",
                "# loot. The network behaviour and the parts are the native half in",
                "# mixins/integratedtunnels.",
                f'id = "{MOD}"',
                'name = "Integrated Tunnels"',
                'version = "0.1.0"',
                'description = "Item tunnels for Horizon: an Item Importer and an Item Exporter part on a tunnel cable."',
                'authors = ["rubensworks (aka kroeser)"]',
                'api = "0.2"',
                'side = "both"',
                'capabilities = ["registries"]',
                "",
            ]
        ),
    )
    print(f"assembled {CONTENT} from {JAR.name} (+ {PARENT_JAR.name} for the core texture)")


if __name__ == "__main__":
    main()
