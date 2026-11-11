#!/usr/bin/env bash
# Usage: bash packaging/test-install.sh ubuntu:24.04 native-packages-output
# The compiler runs on the host so it cannot supply missing runtime libraries.
set -euo pipefail
image=${1:?Supply an Ubuntu, Debian or Fedora container image}
packages=$(realpath "${2:?Supply a native-packages output directory}")
case "$(uname -m)" in
  x86_64) target=linux-amd64 ;;
  aarch64) target=linux-arm64 ;;
  *) echo 'Unsupported test architecture' >&2; exit 1 ;;
esac
case "$image" in
  ubuntu:*|debian:*) format=deb ;;
  fedora:*) format=rpm ;;
  *) echo 'Unsupported test distribution' >&2; exit 1 ;;
esac
package_dir="$packages/packages/$target/$format"
test -d "$package_dir"
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
checks=$(mktemp -d)
trap 'rm -rf -- "$checks"' EXIT
cc -std=c99 -Wall -Wextra -Werror "$script_dir/check-runtime-libs.c" -ldl -o "$checks/check-runtime-libs"
docker run --rm \
  --volume "$package_dir:/packages:ro" \
  --volume "$checks:/checks:ro" \
  --env "FORMAT=$format" "$image" sh -ec '
    set -- /packages/*."$FORMAT"
    test "$#" -eq 1
    test -f "$1"
    mkdir -p /root/.config/whatsawra
    printf "%s\n" "preserve-existing-settings" > /root/.config/whatsawra/fixture
    if [ "$FORMAT" = deb ]; then
      apt-get update
      DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$1"
      dpkg-query -W whatsawra
    else
      dnf install -y --setopt=install_weak_deps=False "$1"
      rpm -q whatsawra
    fi
    whatsawra --version
    /checks/check-runtime-libs
    test -s /usr/share/applications/whatsawra.desktop
    test -s /usr/share/icons/hicolor/scalable/apps/whatsawra.svg
    grep -qx "Icon=whatsawra" /usr/share/applications/whatsawra.desktop
    grep -qx "StartupWMClass=whatsawra" /usr/share/applications/whatsawra.desktop
    test -s /usr/share/whatsawra/omarchy/whatsawra.json.tpl
    test -x /usr/share/whatsawra/omarchy/whatsawra-theme
    if [ "$FORMAT" = deb ]; then apt-get remove -y whatsawra; else dnf remove -y whatsawra; fi
    test ! -e /usr/bin/whatsawra
    test ! -e /usr/share/applications/whatsawra.desktop
    test ! -e /usr/share/icons/hicolor/scalable/apps/whatsawra.svg
    test ! -e /usr/share/whatsawra/omarchy/whatsawra.json.tpl
    test ! -e /usr/share/whatsawra/omarchy/whatsawra-theme
    test "$(cat /root/.config/whatsawra/fixture)" = preserve-existing-settings
  '
