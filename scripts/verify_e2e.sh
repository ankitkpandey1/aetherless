#!/bin/bash
set -e

# Aetherless End-to-End Verification Script
# Verifies orchestrator startup, handler deployment, and metrics

RUST_LOG=info
SOCKET_DIR="/tmp/aetherless-test/sockets"
SNAPSHOT_DIR="/tmp/aetherless-test/snapshots"
STATS_FILE="/dev/shm/aetherless-stats.json"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}== Starting Aetherless E2E Verification ==${NC}"

# 1. Setup Environment
# Aggressive cleanup of previous runs
pkill -f "aether.*up" || true
pkill -f "python.*handler.py" || true
sleep 1

rm -rf /tmp/aetherless-test
mkdir -p "$SOCKET_DIR" "$SNAPSHOT_DIR"
rm -f "$STATS_FILE"

# 2. Build Binaries (Already built in debug, but ensure)
# cargo build --quiet

# 3. Create Test Function Handler
cat <<EOF > /tmp/aetherless-test/handler.py
#!/usr/bin/env python3
import os
import socket
import sys
import time
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain')
        self.end_headers()
        self.wfile.write(b"Hello from Aetherless")

    def log_message(self, format, *args):
        pass

# Ensure executable
if __name__ == "__main__":
    # Simulate startup delay
    # time.sleep(0.1)
    
    # Handshake
    sock_path = os.environ.get("AETHER_SOCKET")
    if sock_path:
        # Retry connect
        for i in range(5):
            try:
                s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                s.connect(sock_path)
                s.send(b"READY")
                s.close()
                break
            except Exception as e:
                time.sleep(0.1)

    port = int(os.environ.get("AETHER_TRIGGER_PORT", 8081))
    print(f"Starting server on {port}")
    HTTPServer(('0.0.0.0', port), Handler).serve_forever()
EOF
chmod +x /tmp/aetherless-test/handler.py

# 4. Create Config
cat <<EOF > /tmp/aetherless-test/config.yaml
orchestrator:
  snapshot_dir: $SNAPSHOT_DIR
  warm_pool_size: 2
  restore_timeout_ms: 50
  shm_buffer_size: 1048576

functions:
  - id: test-func
    memory_limit_mb: 128
    trigger_port: 8081
    handler_path: /tmp/aetherless-test/handler.py
    timeout_ms: 5000
    environment:
      TEST_VAR: "true"
EOF

# 5. Start Orchestrator in Background
echo "Starting orchestrator..."
./target/debug/aether -c /tmp/aetherless-test/config.yaml up --foreground --warm-pool &
PID=$!

# Cleanup trap
cleanup() {
    echo "Stopping orchestrator..."
    kill -INT $PID || true
    wait $PID || true
    echo -e "${GREEN}== Verification Complete ==${NC}"
}
trap cleanup EXIT

# 6. Wait for Startup
sleep 2

# 7. Verification Steps

# Check Metrics Endpoint
echo -n "Checking Metrics Endpoint (port 9090)... "
METRICS_OUT=$(curl -v http://127.0.0.1:9090/metrics 2>&1)
if echo "$METRICS_OUT" | grep -q "function_cold_starts_total"; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL${NC}"
    echo "Output was: '$METRICS_OUT'"
    # Check if port is open
    netstat -tulpn | grep 9090 || echo "Port 9090 NOT listening"
    exit 1
fi

# Check Handler Response
echo -n "Checking Function Response (port 8081)... "
RESPONSE=$(curl -s http://localhost:8081/)
if echo "$RESPONSE" | grep -q "Hello from Aetherless"; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL (Got: '$RESPONSE')${NC}"
    exit 1
fi

# Check Stats File
echo -n "Checking Stats File (/dev/shm)... "
if [ -f "$STATS_FILE" ]; then
    echo -e "${GREEN}PASS${NC}"
else
    echo -e "${RED}FAIL${NC}"
    exit 1
fi

echo -e "${GREEN}All checks passed!${NC}"
