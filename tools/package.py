#!/usr/bin/env python3
"""Package the mod as a source-only `.hnmod`, the file Horizon's registry and launcher take.

The archive holds `horizon-native.json`, `mixins/<id>/` (`.rs` and `.toml`) and
`mixins/<id>/data/` — the built content mod, compiled into the game by
`native_data!` and loaded through the `content_root` injection (the AE2
pattern), so one `.hnmod` installs both halves. Fixed dates and modes make the
same sources always give the same bytes and the same SHA-256, and the deliverable
is copied to the port root by `tools/package.sh`.
"""
import hashlib
import json
from pathlib import Path
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
GAME = '26.3'


def package():
    compat = json.loads((ROOT / 'compatibility.json').read_text())
    manifests = sorted((ROOT / 'mixins').glob('*/horizon-mixin.toml'))
    if len(manifests) != 1:
        raise SystemExit('a .hnmod holds one bundle: keep one mixins/<id>/horizon-mixin.toml')
    bundle = manifests[0].parent
    mixin = tomllib.loads(manifests[0].read_text())
    mod = mixin['mod']
    if bundle.name != mod['id']:
        raise SystemExit(f"rename mixins/{bundle.name}/ to mixins/{mod['id']}/ (the registry finds it by id)")
    if not mod['id'].replace('_', '').isalnum() or not mod['id'].islower():
        raise SystemExit('the registry takes ids of lowercase letters, digits and _ only')
    if mixin.get('compatibility', {}).get('horizon_revision') != compat['horizon_revision']:
        raise SystemExit('horizon-mixin.toml and compatibility.json pin different Horizon revisions')
    manifest = dict(schema=1, id=mod['id'], version=mod['version'], game=GAME,
                    horizon=compat['horizon_revision'], mixins=compat['horizon_mixins_revision'])
    files = {'horizon-native.json': (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode()}
    # A port with content must carry the *built* one inside the bundle: the
    # registry keeps one file per project, so a code-only bundle would install
    # hooks that name blocks no pack provides.
    built = ROOT / 'content' / f"{mod['id']}.hmod"
    embedded = bundle / 'data' / f"{mod['id']}-content.hmod"
    if (ROOT / 'content' / 'horizon-mod.toml').is_file():
        if not embedded.is_file():
            raise SystemExit(f'{built} is the content half but {embedded} is missing: '
                             'copy the built content mod into data/ (one-file port)')
        if built.is_file() and embedded.read_bytes() != built.read_bytes():
            raise SystemExit(f'{embedded} is not the built {built}: copy the built content mod into data/')
    # The bundle's `data/` folder travels as `mixins/<id>/data/…` (`docs/native-mods.md`, "Data
    # files"): `native_data!` compiles a file of it into the game, so the content half ships
    # inside the one `.hnmod` the registry stores.
    for path in sorted(bundle.rglob('*')):
        if not path.is_file():
            continue
        relative = path.relative_to(bundle)
        packed = (len(relative.parts) == 1 and path.suffix in ('.rs', '.toml')) \
            or relative.parts[0] == 'data'
        if packed:
            files[path.relative_to(ROOT).as_posix()] = path.read_bytes()
    out = ROOT / 'target' / f"{mod['id']}-{mod['version']}.hnmod"
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, content)
    print(json.dumps(dict(file=str(out), size=out.stat().st_size,
                          sha256=hashlib.sha256(out.read_bytes()).hexdigest()), indent=2))


if __name__ == '__main__':
    package()
