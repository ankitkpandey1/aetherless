# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey

#!/usr/bin/env python3
"""
Aetherless Function Handler - Hello World API
Example handler demonstrating the Aetherless protocol.
"""
import os
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        
        response = {
            'message': 'Hello from Aetherless! 🚀',
            'function': os.environ.get('AETHER_FUNCTION_ID', 'unknown'),
            'path': self.path
        }
        self.wfile.write(json.dumps(response, indent=2).encode())
    
    def log_message(self, format, *args):
        func_id = os.environ.get('AETHER_FUNCTION_ID', 'handler')
        print(f"[{func_id}] {format % args}")


def main():
    function_id = os.environ.get('AETHER_FUNCTION_ID', 'hello')
    port = int(os.environ.get('AETHER_TRIGGER_PORT', '8080'))
    
    # Connect to Aetherless orchestrator
    socket_path = os.environ.get('AETHER_SOCKET')
    if not socket_path:
        print(f"[{function_id}] ERROR: AETHER_SOCKET not set")
        return
    
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    sock.send(b'READY')  # Signal ready to orchestrator
    print(f"[{function_id}] Connected to orchestrator")
    
    # Start HTTP server
    print(f"[{function_id}] Starting on port {port}...")
    server = HTTPServer(('0.0.0.0', port), Handler)
    print(f"[{function_id}] Listening on http://0.0.0.0:{port}")
    server.serve_forever()


if __name__ == '__main__':
    main()
