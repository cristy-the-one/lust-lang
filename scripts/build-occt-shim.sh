#!/bin/sh
# Build the Open CASCADE shim once (docs/occt-shim.md).
# Not called from scripts/build.sh or from CI. A miss in the cache
# does not run this script; lust prints a diagnostic that names it.
#
# Linux (this VM, OCCT from apt libocct-*-dev):
#   scripts/build-occt-shim.sh
# Writes $LUST_OCCT_CACHE/<key>/liblust_occt.so and a one-line
# $LUST_OCCT_CACHE/current with that library's absolute path.
# Default cache: $HOME/.cache/lust/occt
#
# Windows was not run here. Marius's machine is the place to verify it.
# From a developer prompt whose cl matches the OCCT build (same CRT):
#   set OCCT_INC=C:\OpenCASCADE\inc
#   set OCCT_LIB=C:\OpenCASCADE\win64\vc14\lib
#   cl /nologo /LD /EHsc /O2 /std:c++17 /I"%OCCT_INC%" shims\occt\shim.cc ^
#      /Fe:lust_occt.dll /link /LIBPATH:"%OCCT_LIB%" ^
#      TKPrim.lib TKTopAlgo.lib TKMesh.lib TKBRep.lib TKG2d.lib TKG3d.lib ^
#      TKGeomBase.lib TKGeomAlgo.lib TKMath.lib TKernel.lib
# Copy lust_occt.dll and the OCCT DLLs it needs into one directory
#   %LUST_OCCT_CACHE%\<key>\
# and write that lust_occt.dll's absolute path into
#   %LUST_OCCT_CACHE%\current
# LoadLibraryW looks in the DLL's directory first, so the OCCT DLLs
# stay next to lust_occt.dll. The rustc oracle links the import lib
# and runs with that directory on PATH.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
src_h="$root/shims/occt/lust_occt.h"
src_cc="$root/shims/occt/shim.cc"
cache=${LUST_OCCT_CACHE:-${HOME:-/tmp}/.cache/lust/occt}

os=$(uname -s 2>/dev/null || echo unknown)
case "$os" in
MINGW*|MSYS*|CYGWIN*|Windows*)
  echo "occt shim: this script's Windows build was not run in the Linux VM." >&2
  echo "occt shim: follow the cl steps at the top of scripts/build-occt-shim.sh on a machine with OCCT." >&2
  exit 2
  ;;
esac

if [ ! -f "$src_cc" ]; then
  echo "occt shim: missing $src_cc" >&2
  exit 1
fi

cxx=${CXX:-g++}
inc=${OCCT_INC:-}
if [ -z "$inc" ]; then
  if [ -f /usr/include/opencascade/BRepPrimAPI_MakeBox.hxx ]; then
    inc=/usr/include/opencascade
  else
    echo "occt shim: no OCCT headers. Install libocct-foundation-dev, libocct-modeling-data-dev, and libocct-modeling-algorithms-dev, or set OCCT_INC." >&2
    exit 1
  fi
fi

libdir=${OCCT_LIB:-}
if [ -z "$libdir" ]; then
  for d in /usr/lib/x86_64-linux-gnu /usr/lib64 /usr/lib; do
    if [ -e "$d/libTKPrim.so" ] || [ -e "$d/libTKPrim.dylib" ]; then
      libdir=$d
      break
    fi
  done
fi
if [ -z "$libdir" ]; then
  echo "occt shim: no OCCT libraries. Set OCCT_LIB to the directory that holds libTKPrim.so." >&2
  exit 1
fi

occt_ver=${OCCT_VERSION:-unknown}
if command -v dpkg-query >/dev/null 2>&1; then
  occt_ver=$(dpkg-query -W -f='${Version}' libocct-foundation-dev 2>/dev/null || echo "$occt_ver")
fi
triple=$("$cxx" -dumpmachine)
cxx_id=$("$cxx" -dumpversion)
src_hash=$(sha256sum "$src_h" "$src_cc" | sha256sum | awk '{print $1}')

# Ubuntu 24.04's libocct-foundation-dev omits NCollection_AliasedArray.hxx
# (Poly_Triangulation includes it). Not vendored: download the matching
# OCCT 7.6.3 header into the cache when the system tree does not have it.
extra_inc=""
extra_note="system"
if [ ! -f "$inc/NCollection_AliasedArray.hxx" ]; then
  extra_inc="$cache/extra-inc"
  mkdir -p "$extra_inc"
  if [ ! -f "$extra_inc/NCollection_AliasedArray.hxx" ]; then
    url="https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_6_3/src/NCollection/NCollection_AliasedArray.hxx"
    echo "occt shim: system headers lack NCollection_AliasedArray.hxx; downloading the OCCT 7.6.3 copy" >&2
    curl -fsSL -o "$extra_inc/NCollection_AliasedArray.hxx" "$url" \
      || { echo "occt shim: could not download $url" >&2; exit 1; }
  fi
  extra_note=$(sha256sum "$extra_inc/NCollection_AliasedArray.hxx" | awk '{print $1}')
fi

# The key includes the extra header when the script had to fetch it.
key=$(printf '%s\n' "$src_hash" "$occt_ver" "$triple" "$cxx_id" "$extra_note" | sha256sum | awk '{print substr($1,1,16)}')
out="$cache/$key"
mkdir -p "$out"
so="$out/liblust_occt.so"

libs=${OCCT_LIBS:-"-lTKPrim -lTKTopAlgo -lTKMesh -lTKBRep -lTKG2d -lTKG3d -lTKGeomBase -lTKGeomAlgo -lTKMath -lTKernel"}
inc_flags="-I$inc"
if [ -n "$extra_inc" ]; then
  inc_flags="$inc_flags -I$extra_inc"
fi

start=$(python3 -c 'import time; print(time.perf_counter())')
# shellcheck disable=SC2086
"$cxx" -shared -fPIC -fvisibility=hidden -O2 -std=c++17 -Wno-deprecated-declarations \
  $inc_flags \
  -o "$so" "$src_cc" \
  -L"$libdir" -Wl,-rpath,"$libdir" \
  $libs -pthread
end=$(python3 -c 'import time; print(time.perf_counter())')
python3 -c 'import sys; print("occt shim: built in %.3fs -> %s" % (float(sys.argv[1]) - float(sys.argv[2]), sys.argv[3]))' "$end" "$start" "$so"

{
  echo "occt=$occt_ver"
  echo "triple=$triple"
  echo "cxx=$cxx $cxx_id"
  echo "key=$key"
  echo "src=$src_hash"
} >"$out/build-id.txt"
# Absolute path. The compiler reads this file and does not recompute the key.
printf '%s\n' "$so" >"$cache/current"
echo "occt shim: current $cache/current"
