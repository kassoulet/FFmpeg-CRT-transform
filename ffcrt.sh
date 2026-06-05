#!/bin/bash
# FFmpeg CRT transform script / VileR 2021
# parameter 1 = config file
# parameter 2 = input video/image
# parameter 3 = output video/image

LOGLVL=error

# Check cmdline arguments
if [ -z "$1" ] || [ -z "$2" ]; then
  echo
  echo "FFmpeg CRT transform script / VileR 2021"
  echo
  echo "USAGE:  $(basename "$0") <config_file> <input_file> [output_file]"
  echo
  echo "   input_file must be a valid image or video.  If output_file is omitted, the"
  echo "   output will be named \"(input_file)_(config_file).(input_ext)\""
  exit 1
fi

if [ -n "$3" ]; then
  OUTFILE=$(readlink -f "$3")
  OUTEXT=".${3##*.}"
else
  indir=$(dirname "$2")
  inbase=$(basename "${2%.*}")
  conbase=$(basename "${1%.*}")
  inext="${2##*.}"
  OUTFILE="${indir}/${inbase}_${conbase}.${inext}"
  OUTEXT=".$inext"
fi

if [ ! -f "$1" ]; then echo "File not found: $1"; exit 1; fi
if [ ! -f "$2" ]; then echo "File not found: $2"; exit 1; fi
if [ -z "$OUTEXT" ]; then echo "Output filename must have an extension: $OUTFILE"; exit 1; fi

# Find input dimensions and type (image/video)
IX=""; IY=""; FC=""
while IFS='=' read -r key value; do
  case "$key" in
    width)     IX="$value" ;;
    height)    IY="$value" ;;
    nb_frames) FC="$value" ;;
  esac
done < <(ffprobe -hide_banner -loglevel quiet -select_streams v:0 \
  -show_entries stream=width,height,nb_frames "$2" 2>/dev/null)

if [ -z "$IX" ] || [ -z "$IY" ] || [ -z "$FC" ]; then
  echo "Couldn't get media info for input file \"$2\" (invalid image/video?)"
  exit 1
fi

# matroska doesn't return nb_frames; assume video
ext_upper="${2##*.}"
ext_upper="${ext_upper^^}"
if [ "$ext_upper" = "MKV" ]; then FC="unknown"; fi

IS_VIDEO=""
if [ "$FC" != "N/A" ]; then IS_VIDEO=1; fi

# Read config file / check for required external files
while read -r key value rest; do
  [[ "$key" =~ ^\; ]] && continue
  [ -z "$key" ] && continue
  declare "$key=$value"
done < "$1"

if [ ! -f "_${OVL_TYPE}.png" ]; then
  echo "File not found: _${OVL_TYPE}.png"
  exit 1
fi

# Set temporary + final output parameters
if [ -n "$IS_VIDEO" ]; then
  if [ "$OFORMAT" = "0" ]; then
    FIN_OUTPARAMS="-pix_fmt rgb24 -c:a copy -c:v libx264rgb -crf 8"
    FIN_MATRIXSTR=" "
  fi
  if [ "$OFORMAT" = "1" ]; then
    FIN_OUTPARAMS="-pix_fmt yuv444p10le -color_primaries 1 -color_trc 1 -colorspace 1 -color_range 2 -c:v libx264 -crf 8 -c:a copy"
    FIN_MATRIXSTR=", scale=iw:ih:flags=neighbor+full_chroma_inp:in_range=full:out_range=full:out_color_matrix=bt709"
  fi
  if [ "$16BPC_PROCESSING" = "yes" ]; then
    TMP_EXT="mkv"
    TMP_OUTPARAMS="-pix_fmt gbrp16le -c:a copy -c:v ffv1"
  else
    TMP_EXT="avi"
    TMP_OUTPARAMS="-c:a copy -c:v libx264rgb -crf 0"
  fi
else
  if [ "$OFORMAT" = "0" ]; then
    FIN_MATRIXSTR=" "
    FIN_OUTPARAMS="-frames:v 1 -pix_fmt rgb24"
  fi
  if [ "$OFORMAT" = "1" ]; then
    FIN_MATRIXSTR=" "
    FIN_OUTPARAMS="-frames:v 1 -pix_fmt rgb48be"
  fi
  if [ "$16BPC_PROCESSING" = "yes" ]; then
    TMP_EXT="mkv"
    TMP_OUTPARAMS="-pix_fmt gbrp16le -c:v ffv1"
  else
    TMP_EXT="png"
    TMP_OUTPARAMS=""
  fi
fi

# Bit depth-dependent vars
if [ "$16BPC_PROCESSING" = "yes" ]; then
  RNG=65536
  RGBFMT="gbrp16le"
  KLUDGEFMT="gbrpf32le"
else
  RNG=256
  RGBFMT="rgb24"
  KLUDGEFMT="rgb24"
fi

# Set some shorthand vars and calculate stuff
SXINT=$((IX * PRESCALE_BY))
PX=$((IX * PRESCALE_BY * PX_ASPECT))
PY=$((IY * PRESCALE_BY))
OX="round(${OY}*${OASPECT})"
SWSFLAGS="accurate_rnd+full_chroma_int+full_chroma_inp"
if [ "${V_PX_BLUR:-0}" = "0" ]; then
  VSIGMA="0.1"
else
  VSIGMA=$(echo "scale=4; $V_PX_BLUR/100*$PRESCALE_BY" | bc)
fi

if [ "$VIGNETTE_ON" = "yes" ]; then
  if [ "$16BPC_PROCESSING" = "yes" ]; then
    VIGNETTE_STR="[ref]; color=c=#FFFFFF:s=${PX}x${PY},format=rgb24[mkscale];\
[mkscale][ref]scale2ref=flags=neighbor[mkvig][novig];\
[mkvig]setsar=sar=1/1, vignette=PI*${VIGNETTE_POWER},format=gbrp16le[vig];\
[novig][vig]blend=all_mode='multiply':shortest=1,"
  else
    VIGNETTE_STR=", vignette=PI*${VIGNETTE_POWER}, "
  fi
else
  VIGNETTE_STR=","
fi

if [ "$FLAT_PANEL" = "yes" ]; then
  SCANLINES_ON="no"
  CRT_CURVATURE=0
  OVL_ALPHA=0
fi

# Curvature factors
if [ "$(echo "$BEZEL_CURVATURE < $CRT_CURVATURE" | bc -l)" = "1" ]; then
  BEZEL_CURVATURE=$CRT_CURVATURE
fi

LENSC=""; BZLENSC=""
if [ "$CRT_CURVATURE" != "0" ]; then
  LENSC=", pad=iw+8:ih+8:4:4:black, lenscorrection=k1=${CRT_CURVATURE}:k2=${CRT_CURVATURE}:i=bilinear, crop=iw-8:ih-8"
fi
if [ "$BEZEL_CURVATURE" != "0" ]; then
  BZLENSC=", scale=iw*2:ih*2:flags=gauss, pad=iw+8:ih+8:4:4:black, lenscorrection=k1=${BEZEL_CURVATURE}:k2=${BEZEL_CURVATURE}:i=bilinear, crop=iw-8:ih-8, scale=iw/2:ih/2:flags=gauss"
fi

# Scan factor
SCAN_FACTOR_LC="${SCAN_FACTOR,,}"
if [ "$SCAN_FACTOR_LC" = "half" ]; then
  SCAN_FACTOR="0.5"
  SL_COUNT=$((IY / 2))
elif [ "$SCAN_FACTOR_LC" = "double" ]; then
  SCAN_FACTOR="2"
  SL_COUNT=$((IY * 2))
else
  SCAN_FACTOR="1"
  SL_COUNT=$IY
fi

# Monochrome settings
MONOCURVES=""
TEXTURE_OVL=""
MONO_STR1=" "
MONO_STR2=" "
PXGRID_INVERT=0

MC_LC="${MONITOR_COLOR,,}"
case "$MC_LC" in
  white)      MONOCURVES=" " ;;
  paperwhite) MONOCURVES=" "; TEXTURE_OVL="paper" ;;
  green1)     MONOCURVES="curves=r='0/0 .77/0 1/.45':g='0/0 .77/1 1/1':b='0/0 .77/.17 1/.73'," ;;
  green2)     MONOCURVES="curves=r='0/0 .43/.16 .72/.30 1/.56':g='0/0 .51/.53 .82/1 1/1':b='0/0 .43/.16 .72/.30 1/.56'," ;;
  bw-tv)      MONOCURVES="curves=r='0/0 .5/.49 1/1':g='0/0 .5/.49 1/1':b='0/0 .5/.62 1/1'," ;;
  amber)      MONOCURVES="curves=r='0/0 .25/.45 .8/1 1/1':g='0/0 .25/.14 .8/.55 1/.8':b='0/0 .8/0 1/.29'," ;;
  plasma)     MONOCURVES="curves=r='0/0 .13/.27 .52/.83 .8/1 1/1':g='0/0 .13/0 .52/.14 .8/.35 1/.54':b='0/0 1/0'," ;;
  eld)        MONOCURVES="curves=r='0/0 .46/.49 1/1':g='0/0 .46/.37 1/.94':b='0/0 .46/0 1/.29'," ;;
  lcd)        MONOCURVES="curves=r='0/.09 1/.48':g='0/.11 1/.56':b='0/.20 1/.35',"; PXGRID_INVERT=1 ;;
  lcd-lite)   MONOCURVES="curves=r='0/.06 1/.64':g='0/.15 1/.77':b='0/.35 1/.65',"; PXGRID_INVERT=1 ;;
  lcd-lwhite) MONOCURVES="curves=r='0/.09 1/.82':g='0/.18 1/.89':b='0/.29 1/.93',"; PXGRID_INVERT=1 ;;
  lcd-lblue)  MONOCURVES="curves=r='0/.00 1/.62':g='0/.22 1/.75':b='0/.73 1/.68',"; PXGRID_INVERT=1 ;;
esac

# lcd grain only for the appropriate monitor types
if [ "${MONITOR_COLOR:0:3}" = "lcd" ] || [ "${MONITOR_COLOR:0:3}" = "LCD" ]; then
  if [ "$LCD_GRAIN" -gt 0 ] 2>/dev/null; then TEXTURE_OVL="lcdgrain"; fi
fi

if [ "$MC_LC" != "rgb" ]; then
  OVL_ALPHA=0
  MONO_STR1="format=gray16le,format=gbrp16le,"
  MONO_STR2="${MONOCURVES}"
fi

if [ "$MC_LC" = "p7" ]; then
  MONOCURVES_LAT="curves=r='0/0 .6/.31 1/.75':g='0/0 .25/.16 .75/.83 1/.94':b='0/0 .5/.76 1/.97'"
  MONOCURVES_DEC="curves=r='0/0 .5/.36 1/.86':g='0/0 .5/.52 1/.89':b='0/0 .5/.08 1/.13'"
  DECAYDELAY=$((LATENCY / 2))
  if [ -n "$IS_VIDEO" ]; then
    MONO_STR2="\
split=4 [orig][a][b][c];\
[a] tmix=${LATENCY}, ${MONOCURVES_LAT} [lat];\
[b] lagfun=${P_DECAY_FACTOR} [dec1]; [c] lagfun=$(echo "$P_DECAY_FACTOR * 0.95" | bc -l) [dec2];\
[dec2][dec1] blend=all_mode='lighten':all_opacity=0.3, ${MONOCURVES_DEC}, setpts=PTS+(${DECAYDELAY}/FR)/TB [decay];\
[lat][decay] blend=all_mode='lighten':all_opacity=${P_DECAY_ALPHA} [p7];\
[orig][p7] blend=all_mode='screen',format=${RGBFMT},"
  else
    MONO_STR2="\
split=3 [orig][a][b];\
[a] ${MONOCURVES_LAT} [lat];\
[b] ${MONOCURVES_DEC} [decay];\
[lat][decay] blend=all_mode='lighten':all_opacity=${P_DECAY_ALPHA} [p7];\
[orig][p7] blend=all_mode='screen',format=${RGBFMT},"
  fi
fi

# Can skip some stuff where not needed
SKIP_OVL=""
SKIP_BRI=""
alpha_int="${OVL_ALPHA%.*}"
alpha_frac="${OVL_ALPHA#*.}"
bri_int="${BRIGHTEN%.*}"
bri_frac="${BRIGHTEN#*.}"
if [ "$OVL_ALPHA" = "0" ] || ([ "$alpha_int" = "0" ] && [ "$alpha_frac" = "0" ]); then SKIP_OVL=1; fi
if [ "$BRIGHTEN" = "1" ] || ([ "$bri_int" = "1" ] && [ "$bri_frac" = "0" ]); then SKIP_BRI=1; fi

FFSTART=$(date +%T)
if [ "$FC" != "N/A" ]; then
  echo
  echo "Input frame count: $FC"
  echo "---------------------------"
fi

# --------------------------------------------------
# Create bezel with rounded corners and curvature
# --------------------------------------------------
echo "Bezel:"
if [ "$CORNER_RADIUS" = "0" ]; then
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y \
    -f lavfi -i "color=c=#ffffff:s=${PX}x${PY}, format=rgb24 ${BZLENSC}" \
    -frames:v 1 TMPbezel.png
else
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y \
    -f lavfi -i "color=s=1024x1024, format=gray, geq='lum=if(lte((X-W)^2+(Y-H)^2, 1024*1024), 255, 0)', scale=${CORNER_RADIUS}:${CORNER_RADIUS}:flags=lanczos" \
    -filter_complex "\
color=c=#ffffff:s=${PX}x${PY}, format=rgb24[bg];\
[0] split=4 [tl][c2][c3][c4];\
[c2] transpose=1 [tr];\
[c3] transpose=3 [br];\
[c4] transpose=2 [bl];\
[bg][tl] overlay=0:0:format=rgb [p1];\
[p1][tr] overlay=$((PX - CORNER_RADIUS)):0:format=rgb [p2];\
[p2][br] overlay=$((PX - CORNER_RADIUS)):$((PY - CORNER_RADIUS)):format=rgb [p3];\
[p3][bl] overlay=x=0:y=$((PY - CORNER_RADIUS)):format=rgb ${BZLENSC}" \
    -frames:v 1 TMPbezel.png
fi
if [ $? -ne 0 ]; then exit 1; fi

# --------------------------------------------------
# Create scanlines, add curvature
# --------------------------------------------------
if [ "${SCANLINES_ON,,}" = "yes" ]; then
  echo
  echo "Scanlines:"
  SCANLINE_PERIOD=$(echo "$PRESCALE_BY / $SCAN_FACTOR" | bc)
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -f lavfi \
    -i nullsrc=s=1x100 \
    -vf "\
format=gray,\
geq=lum='if(lt(Y,${SCANLINE_PERIOD}), pow(sin(Y*PI/${SCANLINE_PERIOD}), 1/${SL_WEIGHT})*255, 0)',\
crop=1:${SCANLINE_PERIOD}:0:0,\
scale=${PX}:ih:flags=neighbor" \
    -frames:v 1 TMPscanline.png

  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -loop 1 -framerate 1 -t $SL_COUNT \
    -i TMPscanline.png \
    -vf "\
format=gray16le,\
tile=layout=1x${SL_COUNT},\
scale=iw*3:ih*3:flags=gauss ${LENSC}, scale=iw/3:ih/3:flags=gauss,\
format=gray16le, format=${RGBFMT}" \
    -frames:v 1 ${TMP_OUTPARAMS} TMPscanlines.${TMP_EXT}
  if [ $? -ne 0 ]; then exit 1; fi
fi

# --------------------------------------------------
# Create shadowmask/texture overlay, add curvature
# --------------------------------------------------
echo
echo "Shadowmask overlay:"
if [ "$(echo "$OVL_ALPHA > 0" | bc -l)" = "1" ]; then
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i "_${OVL_TYPE}.png" -vf "\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
scale=round(iw*${OVL_SCALE}):round(ih*${OVL_SCALE}):flags=lanczos+${SWSFLAGS}" \
    TMPshadowmask1x.png

  OVL_X=""; OVL_Y=""
  while IFS='=' read -r key value; do
    case "$key" in
      width)  OVL_X="$value" ;;
      height) OVL_Y="$value" ;;
    esac
  done < <(ffprobe -hide_banner -loglevel quiet -show_entries stream=width,height TMPshadowmask1x.png 2>/dev/null)

  TILES_X=$((PX / OVL_X + 1))
  TILES_Y=$((PY / OVL_Y + 1))

  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -loop 1 -i TMPshadowmask1x.png -vf "\
tile=layout=${TILES_X}x${TILES_Y},\
crop=${PX}:${PY},\
scale=iw*2:ih*2:flags=gauss ${LENSC},\
scale=iw/2:ih/2:flags=bicubic,\
lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)'" \
    -frames:v 1 TMPshadowmask.png
  if [ $? -ne 0 ]; then exit 1; fi
else
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -f lavfi -i "color=c=#00000000:s=${PX}x${PY},format=rgba" -frames:v 1 TMPshadowmask.png
fi

# Texture overlay
if [ -n "$TEXTURE_OVL" ]; then
  if [ "$TEXTURE_OVL" = "paper" ]; then
    PAPERX=$((OY * OASPECT * 67 / 100))
    PAPERY=$((OY * 67 / 100))
    echo
    echo "Texture overlay:"
    ffmpeg -hide_banner -y -loglevel $LOGLVL -stats -f lavfi -i "color=c=#808080:s=${PAPERX}x${PAPERY}" \
      -filter_complex "\
noise=all_seed=5150:all_strength=100:all_flags=u, format=gray,\
lutrgb='r=(val-70)*255/115:g=(val-70)*255/115:b=(val-70)*255/115',\
format=rgb24,\
lutrgb='r=if(between(val,0,101),207,if(between(val,102,203),253,251)):g=if(between(val,0,101),238,if(between(val,102,203),225,204)):b=if(between(val,0,101),255,if(between(val,102,203),157,255))',\
format=gbrp16le,\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
scale=${OX}:${OY}:flags=bilinear,\
gblur=sigma=3:steps=6,\
lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)',\
format=gbrp16le,format=rgb24" \
      -frames:v 1 TMPtexture.png
  elif [ "$TEXTURE_OVL" = "lcdgrain" ]; then
    GRAINX=$((OY * OASPECT * 50 / 100))
    GRAINY=$((OY * 50 / 100))
    echo
    echo "Texture overlay:"
    ffmpeg -hide_banner -y -loglevel $LOGLVL -stats -filter_complex "\
color=#808080:s=${GRAINX}x${GRAINY},\
noise=all_seed=5150:all_strength=${LCD_GRAIN}, format=gray,\
scale=${OX}:${OY}:flags=lanczos, format=rgb24" \
      -frames:v 1 TMPtexture.png
  fi
fi

# --------------------------------------------------
# Create discrete pixel grid if set
# --------------------------------------------------
if [ "${FLAT_PANEL,,}" = "yes" ]; then
  if [ "$PXGRID_INVERT" = "1" ]; then
    LUM_GAP="255*${PXGRID_ALPHA}"
    LUM_PX=0
  else
    LUM_GAP="255-255*${PXGRID_ALPHA}"
    LUM_PX=255
  fi
  GX=$((PRESCALE_BY / PX_FACTOR_X))
  GY=$((PRESCALE_BY / PX_FACTOR_Y))

  echo
  echo "Grid:"
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -f lavfi \
    -i nullsrc=s=${SXINT}x${PY} -vf "\
format=gray,\
geq=lum='if(gte(mod(X,${GX}),$((GX - PX_X_GAP)))+gte(mod(Y,${GY}),$((GY - PX_Y_GAP))),${LUM_GAP},${LUM_PX})',\
format=gbrp16le,\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
scale=${PX}:ih:flags=bicubic,\
lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)',\
format=gbrp16le,format=rgb24" \
    -frames:v 1 TMPgrid.png
  if [ $? -ne 0 ]; then exit 1; fi
fi

# --------------------------------------------------
# Pre-process if needed: phosphor decay (video only), invert, pixel latency (video only)
# --------------------------------------------------
SCALESRC="$2"
VF_PRE=""
PREPROCESS=""

if [ "${INVERT_INPUT,,}" = "yes" ]; then
  PREPROCESS=1
  VF_PRE="negate"
fi

if [ -n "$IS_VIDEO" ] && [ "$LATENCY" -gt 0 ] && [ "$MC_LC" != "p7" ]; then
  PREPROCESS=1
  VF_PRE_OLD="$VF_PRE"
  VF_PRE="split [o][2lat];
[2lat] tmix=${LATENCY}, setpts=PTS+((${LATENCY}/2)/FR)/TB [lat];
[lat][o] blend=all_opacity=${LATENCY_ALPHA}"
  if [ -n "$VF_PRE_OLD" ]; then
    VF_PRE="${VF_PRE}, ${VF_PRE_OLD}"
  fi
fi

if [ -n "$IS_VIDEO" ] && [ "$P_DECAY_FACTOR" != "0" ] && [ "$(echo "$P_DECAY_FACTOR > 0" | bc -l)" = "1" ] && [ "$MC_LC" != "p7" ]; then
  PREPROCESS=1
  VF_PRE_OLD="$VF_PRE"
  VF_PRE="[0] split [orig][2lag];
[2lag] lagfun=${P_DECAY_FACTOR} [lag];
[orig][lag] blend=all_mode='lighten':all_opacity=${P_DECAY_ALPHA}"
  if [ -n "$VF_PRE_OLD" ]; then
    VF_PRE="${VF_PRE}, ${VF_PRE_OLD}"
  fi
fi

if [ -n "$PREPROCESS" ]; then
  echo
  echo "Step00 (preprocess):"
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i "$2" -filter_complex "${VF_PRE}" ${TMP_OUTPARAMS} TMPstep00.${TMP_EXT}
  if [ $? -ne 0 ]; then exit 1; fi
  SCALESRC="TMPstep00.${TMP_EXT}"
fi

# --------------------------------------------------
# Scale nearest neighbor, go 16bit/channel, apply grid, gamma & pixel blur
# --------------------------------------------------
GRIDBLENDMODE="multiply"
if [ "$PXGRID_INVERT" = "1" ]; then
  GRIDBLENDMODE="screen"
fi

echo
echo "Step01:"

if [ "${FLAT_PANEL,,}" = "yes" ]; then
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i "${SCALESRC}" -filter_complex "\
scale=iw*${PRESCALE_BY}:ih:flags=neighbor,\
format=gbrp16le,\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
scale=iw*${PX_ASPECT}:ih:flags=fast_bilinear,\
scale=iw:ih*${PRESCALE_BY}:flags=neighbor[scaled];\
movie=TMPgrid.png[grid];\
[scaled][grid]blend=all_mode=${GRIDBLENDMODE},\
gblur=sigma=${H_PX_BLUR}/100*${PRESCALE_BY}*${PX_ASPECT}:sigmaV=${VSIGMA}:steps=3" \
    -c:v ffv1 -c:a copy TMPstep01.mkv
else
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i "${SCALESRC}" -filter_complex "\
scale=iw*${PRESCALE_BY}:ih:flags=neighbor,\
format=gbrp16le,\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
scale=iw*${PX_ASPECT}:ih:flags=fast_bilinear,\
scale=iw:ih*${PRESCALE_BY}:flags=neighbor,\
gblur=sigma=${H_PX_BLUR}/100*${PRESCALE_BY}*${PX_ASPECT}:sigmaV=${VSIGMA}:steps=3" \
    -c:v ffv1 -c:a copy TMPstep01.mkv
fi
if [ $? -ne 0 ]; then exit 1; fi

# --------------------------------------------------
# Add halation, revert gamma, normalize blackpoint, revert bit depth, add curvature
# --------------------------------------------------
echo
echo "Step02:"
if [ "${HALATION_ON,,}" = "yes" ]; then
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i TMPstep01.mkv -filter_complex "\
[0]split[a][b],\
[a]gblur=sigma=${HALATION_RADIUS}:steps=6[h],\
[b][h]blend=all_mode='lighten':all_opacity=${HALATION_ALPHA},\
lutrgb='r=clip(gammaval(0.454545)*(258/256)-2*256,minval,maxval):g=clip(gammaval(0.454545)*(258/256)-2*256,minval,maxval):b=clip(gammaval(0.454545)*(258/256)-2*256,minval,maxval)',\
lutrgb='r=val+(${BLACKPOINT}*256*(maxval-val)/maxval):g=val+(${BLACKPOINT}*256*(maxval-val)/maxval):b=val+(${BLACKPOINT}*256*(maxval-val)/maxval)',\
format=${RGBFMT}\
${LENSC}" \
    ${TMP_OUTPARAMS} TMPstep02.${TMP_EXT}
else
  ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i TMPstep01.mkv -vf "\
lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)',\
lutrgb='r=val+(${BLACKPOINT}*256*(maxval-val)/maxval):g=val+(${BLACKPOINT}*256*(maxval-val)/maxval):b=val+(${BLACKPOINT}*256*(maxval-val)/maxval)',\
format=${RGBFMT}\
${LENSC}" \
    ${TMP_OUTPARAMS} TMPstep02.${TMP_EXT}
fi
if [ $? -ne 0 ]; then exit 1; fi

# --------------------------------------------------
# Add bloom, scanlines, shadowmask, rounded corners + brightness fix
# --------------------------------------------------
if [ "${SCANLINES_ON,,}" = "no" ] && [ "$BEZEL_CURVATURE" = "$CRT_CURVATURE" ] && [ "$CORNER_RADIUS" = "0" ] && [ -n "$SKIP_OVL" ] && [ -n "$SKIP_BRI" ]; then
  [ -f "TMPstep03.${TMP_EXT}" ] && rm -f "TMPstep03.${TMP_EXT}"
  mv "TMPstep02.${TMP_EXT}" "TMPstep03.${TMP_EXT}"
else
  if [ "${SCANLINES_ON,,}" = "yes" ]; then
    SL_INPUT="TMPscanlines.${TMP_EXT}"
    if [ "${BLOOM_ON,,}" = "yes" ]; then
      SL_INPUT="TMPbloom.${TMP_EXT}"
      echo
      echo "Step02-bloom:"
      ffmpeg -hide_banner -loglevel $LOGLVL -stats -y \
        -i TMPscanlines.${TMP_EXT} -i TMPstep02.${TMP_EXT} -filter_complex "\
[1]lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)', hue=s=0, lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)'[g],\
[g][0]blend=all_expr='if(gte(A,$((RNG/2))), (B+$((RNG-1-B))*${BLOOM_POWER}*(A-$((RNG/2)))/$((RNG/2))), B)',\
setsar=sar=1/1" \
        ${TMP_OUTPARAMS} "${SL_INPUT}"
    fi
    echo
    echo "Step03:"
    ffmpeg -hide_banner -loglevel $LOGLVL -stats -y \
      -i TMPstep02.${TMP_EXT} -i "${SL_INPUT}" -i TMPshadowmask.png -i TMPbezel.png -filter_complex "\
[0][1]blend=all_mode='multiply':all_opacity=${SL_ALPHA}[a],\
[a][2]blend=all_mode='multiply':all_opacity=${OVL_ALPHA}[b],\
[b][3]blend=all_mode='multiply',\
lutrgb='r=clip(val*${BRIGHTEN},0,$((RNG-1))):g=clip(val*${BRIGHTEN},0,$((RNG-1))):b=clip(val*${BRIGHTEN},0,$((RNG-1)))'" \
      ${TMP_OUTPARAMS} TMPstep03.${TMP_EXT}
  else
    echo
    echo "Step03:"
    ffmpeg -hide_banner -loglevel $LOGLVL -stats -y \
      -i TMPstep02.${TMP_EXT} -i TMPshadowmask.png -i TMPbezel.png -filter_complex "\
[0][1]blend=all_mode='multiply':all_opacity=${OVL_ALPHA}[b],\
[b][2]blend=all_mode='multiply',\
lutrgb='r=clip(val*${BRIGHTEN},0,$((RNG-1))):g=clip(val*${BRIGHTEN},0,$((RNG-1))):b=clip(val*${BRIGHTEN},0,$((RNG-1)))'" \
      ${TMP_OUTPARAMS} TMPstep03.${TMP_EXT}
  fi
  if [ $? -ne 0 ]; then exit 1; fi
fi

# --------------------------------------------------
# Detect crop area
# --------------------------------------------------
ffmpeg -hide_banner -y \
  -f lavfi -i "color=c=#ffffff:s=${PX}x${PY}" -i TMPbezel.png \
  -filter_complex "[0]format=rgb24 ${LENSC}[crt]; [crt][1]overlay, cropdetect=limit=0:round=2" \
  -frames:v 3 -f null - 2>&1 | grep "crop" > TMPcrop
if [ $? -ne 0 ]; then exit 1; fi

CROPTEMP=$(tail -1 TMPcrop)
CROP_STR=""
for w in $CROPTEMP; do CROP_STR="$w"; done

# Texture string for output
TEXTURE_STR=""
if [ -n "$TEXTURE_OVL" ]; then
  if [ "$TEXTURE_OVL" = "paper" ]; then
    TEXTURE_STR="[nop];movie=TMPtexture.png,format=${RGBFMT}[paper];[nop][paper]blend=all_mode='multiply':eof_action='repeat'"
  elif [ "$TEXTURE_OVL" = "lcdgrain" ]; then
    TEXTURE_STR="\
,format=${KLUDGEFMT},split[og1][og2];\
movie=TMPtexture.png,format=${KLUDGEFMT}[lcd];\
[lcd][og1]blend=all_mode='vividlight':eof_action='repeat'[notquite];\
[og2]limiter=0:$((110 * RNG / 256))[fix];\
[fix][notquite]blend=all_mode='lighten':eof_action='repeat', format=${RGBFMT}"
  fi
fi

# --------------------------------------------------
# Final output
# --------------------------------------------------
echo
echo "Output:"
ffmpeg -hide_banner -loglevel $LOGLVL -stats -y -i "TMPstep03.${TMP_EXT}" -filter_complex "\
crop=${CROP_STR},\
format=gbrp16le,\
lutrgb='r=gammaval(2.2):g=gammaval(2.2):b=gammaval(2.2)',\
${MONO_STR1}\
scale=w=round(${OY}*${OASPECT})-$((OMARGIN * 2)):h=$((OY - OMARGIN * 2)):force_original_aspect_ratio=decrease:flags=${OFILTER}+${SWSFLAGS},\
lutrgb='r=gammaval(0.454545):g=gammaval(0.454545):b=gammaval(0.454545)',\
format=gbrp16le,\
format=${RGBFMT},\
${MONO_STR2}\
setsar=sar=1/1\
${VIGNETTE_STR}\
pad=${OX}:${OY}:-1:-1:black\
${TEXTURE_STR}\
${FIN_MATRIXSTR}" \
  ${FIN_OUTPARAMS} "${OUTFILE}"
if [ $? -ne 0 ]; then exit 1; fi

# --------------------------------------------------
# Clean up
# --------------------------------------------------
rm -f TMPbezel.png TMPscanline?.png TMPscanlines.* TMPshadow*.png TMPtexture.png
rm -f TMPgrid.png TMPstep0?.* TMPbloom.* TMPcrop TMPshadowmask1x.png

echo
echo "------------------------"
echo "Output file: $OUTFILE"
echo "Started:     $FFSTART"
echo "Finished:    $(date +%T)"
echo
