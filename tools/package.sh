#!/usr/bin/env bash
# Package both artifacts:
#  - the source-only .hnmod (tools/package.py) with the built content embedded
#    in data/ (the AE2 pattern), in the port root
#  - the .hmod via the pin's own cargo-horizon-mod (build + check), from content/
# Run inside a build slot (it compiles the packaging tool):
#   ports/tools/build-slot.sh run --job id-pack --port ports/integratedtunnels \
#       --step package -- env CARGO_INCREMENTAL=0 tools/package.sh
set -euo pipefail

PORT=$(cd "$(dirname "$0")/.." && pwd)
HORIZON=${HORIZON:-${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}/ports/_shared/pins/horizon-1d63e0a}
LOGS=$PORT/target/ladder
mkdir -p "$LOGS"
cd "$PORT"

# Rebuild content/ from the verified pack jar, then the content package.
python3 tools/assemble_content.py

CARGO_TARGET_DIR=$PORT/target/carry-tools
export CARGO_TARGET_DIR
(cd content && cargo run --locked --offline --quiet \
    --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod build) \
    2>&1 | tee "$LOGS/package-hmod.log"
cp content/target/horizon-mod/integratedtunnels.hmod content/integratedtunnels.hmod
(cd content && cargo run --locked --offline --quiet \
    --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod check integratedtunnels.hmod) \
    2>&1 | tee -a "$LOGS/package-hmod.log"

# The one-file port: the built content travels inside the native bundle.
mkdir -p mixins/integratedtunnels/data
cp content/integratedtunnels.hmod mixins/integratedtunnels/data/integratedtunnels-content.hmod

# tools/package.py writes target/<id>-<version>.hnmod; the delivered name is
# <id>.hnmod in the port root, one file per project.
python3 tools/package.py | tee "$LOGS/package-hnmod.log"
cp target/integratedtunnels-*.hnmod integratedtunnels.hnmod

# The registry's own reader must accept it: one native bundle carrying its content.
(cd content && cargo run --locked --offline --quiet \
    --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod check \
    ../integratedtunnels.hnmod --kind native-mod) \
    2>&1 | tee "$LOGS/check-native.log"

echo "== artifacts =="
for f in content/integratedtunnels.hmod integratedtunnels.hnmod; do
    [ -f "$f" ] || { echo "missing $f"; exit 1; }
    size=$(stat -c %s "$f")
    sha=$(sha256sum "$f" | cut -d' ' -f1)
    entries=$(python3 -c "import zipfile,sys; print(len(zipfile.ZipFile(sys.argv[1]).namelist()))" "$f")
    echo "$f size=$size sha256=$sha entries=$entries"
done
