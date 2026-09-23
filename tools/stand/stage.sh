#!/bin/sh
# Assembles what `turnout deploy` uploads to the stand: its configuration.
#
# The stand runs the published image, so nothing is built there and no source
# goes across - only the compose file, under the name compose looks for. The
# `.env` beside it on the stand holds the database password and is never
# uploaded: it is the stand's own, and it is kept because turnout deploys
# without clearing the directory.
set -eu

out=${1:-deploy}
here="$(cd "$(dirname "$0")/../.." && pwd)"

rm -rf "$out"
mkdir -p "$out"
# From HEAD, not the working tree: what reaches the stand is a committed state.
git -C "$here" show HEAD:docker-compose.install.yml > "$out/docker-compose.yml"
echo "staged docker-compose.yml into $out/"
