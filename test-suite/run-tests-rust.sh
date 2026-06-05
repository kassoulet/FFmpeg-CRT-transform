#!/bin/bash
TESTRUNSTART=$(date +%T)

rm -f ./*OLD.*

for a in *out.*; do
  [ -f "$a" ] && mv "$a" "${a%.*}-OLD.${a##*.}"
done

BINDIR="$(dirname "$0")/.."
pushd "$BINDIR" > /dev/null || exit 1

FFCRT="$(pwd)/target/release/ffcrt"
if [ "${CARGO_TARGET_DIR+set}" = set ]; then
  FFCRT="$CARGO_TARGET_DIR/release/ffcrt"
fi

if [ ! -x "$FFCRT" ]; then
  echo "Building ffcrt (release) ..."
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
