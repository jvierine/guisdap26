#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
destination="${1:-/srv/guisdap}"
# Copy versioned products and assets before the config that selects them.
rsync -a --chmod=D755,F644 --exclude config.json web/ "$destination/"
install -m 644 web/config.json "$destination/config.json.new"
mv "$destination/config.json.new" "$destination/config.json"
