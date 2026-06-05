#!/bin/bash
TESTRUNSTART=$(date +%T)

rm -f ./*OLD.*

for a in *out.*; do
  [ -f "$a" ] && mv "$a" "${a%.*}-OLD.${a##*.}"
done

pushd "$(dirname "$0")/.." > /dev/null || exit 1
for b in test-suite/??.*; do
  [ -f "$b" ] || continue
  base=$(basename "$b")
  name="${base%.*}"
  ext=".${base##*.}"
  ./ffcrt.sh "test-suite/${name}cfg.cfg" "$b" "test-suite/${name}-out${ext}"
done
popd > /dev/null || exit 1

echo "TOTAL FOR ALL TESTS -"
echo "Started:     $TESTRUNSTART"
echo "Finished:    $(date +%T)"
