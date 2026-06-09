#!/bin/bash
TESTRUNSTART=$(date +%T)

rm -f ./*OLD.*

for a in *out.*; do
  [ -f "$a" ] && mv "$a" "${a%.*}-OLD.${a##*.}"
done

BINDIR="$(dirname "$0")/.."
pushd "$BINDIR" > /dev/null || exit 1

# Resolve CARGO_TARGET_DIR from env, .cargo/config.toml, or default.
if [ -z "$CARGO_TARGET_DIR" ]; then
  CARGO_TARGET_DIR=$(cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c "import sys,json; print(json.load(sys.stdin).get('target_directory',''))" 2>/dev/null)
fi
FFCRT="${CARGO_TARGET_DIR:-$(pwd)/target}/release/crt-transform"

if [ ! -x "$FFCRT" ]; then
  echo "Building crt-transform (release) ..."
  cargo build --release || exit 1
fi

echo "Running Rust ffcrt on still-image test cases ..."
echo ""

for b in test-suite/??.*; do
  [ -f "$b" ] || continue
  base=$(basename "$b")
  name="${base%.*}"
  ext="${base##*.}"

  case "${ext,,}" in
    png|jpg|jpeg|tif|tiff|bmp)
      echo "  $base  (${name}cfg.cfg)"
      "$FFCRT" "test-suite/${name}cfg.cfg" "$b" "test-suite/${name}-out.${ext}"
      ;;
    *)
      echo "  $base  skipped (not a still image)"
      ;;
  esac
done

echo ""
echo "TOTAL FOR ALL TESTS -"
echo "Started:     $TESTRUNSTART"
echo "Finished:    $(date +%T)"
