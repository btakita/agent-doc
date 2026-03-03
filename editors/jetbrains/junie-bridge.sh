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

# Signal the IDE agent (by creating/updating the request file)
# The IDE agent should be watching for changes to REQUEST_FILE.

# Wait for RESPONSE_FILE to be created/updated
# Polling for 2 minutes
MAX_WAIT=120
WAITED=0
while [ ! -f "${RESPONSE_FILE}" ] || [ "${RESPONSE_FILE}" -ot "${REQUEST_FILE}" ]; do
  sleep 1
  WAITED=$((WAITED + 1))
  if [ ${WAITED} -ge ${MAX_WAIT} ]; then
    rm "${LOCK_FILE}"
    echo '{"is_error": true, "result": "Timed out waiting for Junie IDE agent response."}'
    exit 1
  fi
done

# Output response to stdout (agent-doc expects JSON)
cat "${RESPONSE_FILE}"

# Cleanup
rm "${LOCK_FILE}"
