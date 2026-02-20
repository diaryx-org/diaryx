#!/bin/bash
set -euo pipefail

BINARY="/home/adammharris/diaryx/target/release/diaryx_sync_server"
NEW_BINARY="/tmp/diaryx-deploy/diaryx_sync_server"

# Ensure target directory exists
mkdir -p "$(dirname "${BINARY}")"

# Backup current binary
if [ -f "${BINARY}" ]; then
  cp "${BINARY}" "${BINARY}.bak"
fi

# Move new binary into place
mv "${NEW_BINARY}" "${BINARY}"
chmod +x "${BINARY}"
rm -rf /tmp/diaryx-deploy

# Restart service (requires passwordless sudo for systemctl)
sudo systemctl restart diaryx-sync

# Health check
sleep 2
if curl -sf http://localhost:3030/health > /dev/null; then
  echo "Deploy successful — health check passed"
else
  echo "Health check failed! Rolling back..."
  cp "${BINARY}.bak" "${BINARY}"
  sudo systemctl restart diaryx-sync
  exit 1
fi
