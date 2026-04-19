#!/bin/bash
set -euo pipefail

BINARY="/home/adammharris/diaryx/target/release/diaryx_sync_server"
NEW_BINARY="/tmp/diaryx-deploy/diaryx_sync_server"
DEPLOY_DIR="/tmp/diaryx-deploy"

# Ensure target directory exists
mkdir -p "$(dirname "${BINARY}")"

# Backup current binary
if [ -f "${BINARY}" ]; then
  cp "${BINARY}" "${BINARY}.bak"
fi

# Move new binary into place
mv "${NEW_BINARY}" "${BINARY}"
chmod +x "${BINARY}"

# ── Deploy Caddyfile (diff-and-copy) ────────────────────────────────────────

CADDY_CHANGED=false
if [ -f "${DEPLOY_DIR}/Caddyfile" ]; then
  if ! diff -q "${DEPLOY_DIR}/Caddyfile" /etc/caddy/Caddyfile >/dev/null 2>&1; then
    sudo cp "${DEPLOY_DIR}/Caddyfile" /etc/caddy/Caddyfile
    CADDY_CHANGED=true
    echo "Caddyfile updated"
  else
    echo "Caddyfile unchanged"
  fi
fi

# ── Deploy systemd service (diff-and-copy) ──────────────────────────────────

SYSTEMD_CHANGED=false
if [ -f "${DEPLOY_DIR}/diaryx-sync.service" ]; then
  if ! diff -q "${DEPLOY_DIR}/diaryx-sync.service" /etc/systemd/system/diaryx-sync.service >/dev/null 2>&1; then
    sudo cp "${DEPLOY_DIR}/diaryx-sync.service" /etc/systemd/system/diaryx-sync.service
    SYSTEMD_CHANGED=true
    echo "systemd service updated"
  else
    echo "systemd service unchanged"
  fi
fi

# ── Reload/restart services ─────────────────────────────────────────────────

if [ "${SYSTEMD_CHANGED}" = true ]; then
  sudo systemctl daemon-reload
fi

sudo systemctl restart diaryx-sync

if [ "${CADDY_CHANGED}" = true ]; then
  sudo systemctl reload caddy
fi

rm -rf "${DEPLOY_DIR}"

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
