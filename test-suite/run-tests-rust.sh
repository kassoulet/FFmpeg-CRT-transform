#!/bin/bash
TESTRUNSTART=$(date +%T)

BINDIR="$(dirname "$0")/.."
pushd "$BINDIR" > /dev/null || exit 1

OUTDIR="test-suite/output"
mkdir -p "$OUTDIR"

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

echo "Running Rust crt-transform on test cases ..."
echo "Output directory: $OUTDIR"
echo ""

PASS=0
FAIL=0

for b in test-suite/??.*; do
  [ -f "$b" ] || continue
  base=$(basename "$b")
  name="${base%.*}"
  ext="${base##*.}"

  cfg="test-suite/${name}cfg.cfg"
  [ -f "$cfg" ] || { echo "  $base  skipped (no ${name}cfg.cfg)"; continue; }

  case "${ext,,}" in
    png|jpg|jpeg|tif|tiff|bmp|mp4|mkv|avi|mov|webm|m4v)
      out="$OUTDIR/${name}-out.${ext}"
      printf "  %-12s" "$base"
      if "$FFCRT" "$cfg" "$b" "$out"; then
        echo "  ok  ->  $out"
        PASS=$((PASS + 1))
      else
        echo "  FAIL"
        FAIL=$((FAIL + 1))
      fi
      ;;
    *)
      echo "  $base  skipped (unsupported format)"
      ;;
  esac
done

echo ""
echo "TOTAL: $PASS passed, $FAIL failed"
echo "Started:  $TESTRUNSTART"
echo "Finished: $(date +%T)"
[ "$FAIL" -eq 0 ]
