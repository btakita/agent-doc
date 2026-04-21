#!/usr/bin/env bash

# Junie Bridge Script for agent-doc
# This script acts as the 'junie' command called by agent-doc.
# It writes the prompt (from stdin) to a file and waits for a response from the Junie IDE agent.

# Configuration
BRIDGE_DIR="${HOME}/.cache/junie-bridge"
REQUEST_FILE="${BRIDGE_DIR}/request.md"
RESPONSE_FILE="${BRIDGE_DIR}/response.json"
LOCK_FILE="${BRIDGE_DIR}/bridge.lock"

mkdir -p "${BRIDGE_DIR}"

# Check for existing lock (avoid concurrent agent-doc runs with Junie)
if [ -f "${LOCK_FILE}" ]; then
  # If lock is older than 5 minutes, assume it's stale
  if [ $(( $(date +%s) - $(stat -c %Y "${LOCK_FILE}") )) -gt 300 ]; then
    rm "${LOCK_FILE}"
  else
    echo '{"is_error": true, "result": "Junie bridge is already in use or locked."}'
    exit 1
  fi
fi
touch "${LOCK_FILE}"

# Capture prompt from stdin
cat > "${REQUEST_FILE}"

# Fire-and-forget: Return immediately to agent-doc with a 'submitting' message
# This allows agent-doc run to finish, and the plugin to open the request and copy it to the clipboard
echo '{"is_error": false, "result": "Submitting to junie..."}'

# Cleanup
rm "${LOCK_FILE}"
