# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey

#!/usr/bin/env python3
"""
Aetherless REST API Handler Example
Demonstrates routing and JSON request/response handling.
"""
import os
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler


class APIHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        routes = {
            '/users': {'users': [{'id': 1, 'name': 'Alice'}, {'id': 2, 'name': 'Bob'}]},
            '/health': {'status': 'healthy', 'function': os.environ.get('AETHER_FUNCTION_ID')},
            '/': {'message': 'Welcome to Aetherless REST API'},
        }
        response = routes.get(self.path, {'error': 'Not found', 'path': self.path})
        status = 200 if self.path in routes else 404
        
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(response, indent=2).encode())

    def do_POST(self):
        content_length = int(self.headers.get('Content-Length', 0))
        body = json.loads(self.rfile.read(content_length)) if content_length else {}
        
        self.send_response(201)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps({'created': body, 'id': 123}).encode())

    def log_message(self, format, *args):
        func_id = os.environ.get('AETHER_FUNCTION_ID', 'api')
        print(f"[{func_id}] {format % args}")


def main():
    function_id = os.environ.get('AETHER_FUNCTION_ID', 'rest-api')
    port = int(os.environ.get('AETHER_TRIGGER_PORT', '3000'))
    
    # Connect to orchestrator
    socket_path = os.environ.get('AETHER_SOCKET')
    if not socket_path:
        print(f"[{function_id}] ERROR: AETHER_SOCKET not set")
        return
    
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    sock.send(b'READY')
    print(f"[{function_id}] Connected to orchestrator")
    
    print(f"[{function_id}] Starting REST API on port {port}...")
    server = HTTPServer(('0.0.0.0', port), APIHandler)
    server.serve_forever()


if __name__ == '__main__':
    main()
