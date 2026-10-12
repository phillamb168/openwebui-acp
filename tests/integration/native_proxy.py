"""Loopback relay transport fixture: preserve signed requests, optionally lose an ack.

Only the fixed disposable backend is reachable. WebSockets are tunneled verbatim;
HTTP events remain signed by the native runner/adapter, never by this proxy.
"""
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import select
import socket
import threading


class RelayProxy(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port, backend_port, agent):
        self.backend_port = backend_port
        self.agent = agent
        self.drop_next = False
        self.attempts = []
        self.guard = threading.Lock()
        super().__init__(('127.0.0.1', port), Handler)


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.headers.get('Upgrade', '').lower() == 'websocket':
            with socket.create_connection(('127.0.0.1', self.server.backend_port), timeout=10) as upstream:
                head = self.requestline + '\r\n' + ''.join(k + ': ' + v + '\r\n' for k, v in self.headers.items()) + '\r\n'
                upstream.sendall(head.encode())
                sockets = [self.connection, upstream]
                while True:
                    ready, _, _ = select.select(sockets, [], [], 1)
                    for source in ready:
                        data = source.recv(65536)
                        if not data:
                            self.close_connection = True
                            return
                        (upstream if source is self.connection else self.connection).sendall(data)
        else:
            self.forward()

    def do_POST(self):
        self.forward()

    def do_PATCH(self):
        self.forward()

    def do_DELETE(self):
        self.forward()

    def forward(self):
        size = int(self.headers.get('Content-Length', '0'))
        if size > 4 * 1024 * 1024:
            self.send_error(413)
            return
        body = self.rfile.read(size)
        conn = http.client.HTTPConnection('127.0.0.1', self.server.backend_port, timeout=30)
        try:
            conn.request(self.command, self.path, body, dict(self.headers))
            response = conn.getresponse()
            data = response.read()
            lost = False
            if self.command == 'POST' and self.path == '/events':
                event = json.loads(body)
                if event.get('kind') == 9 and event.get('pubkey') == self.server.agent:
                    with self.server.guard:
                        self.server.attempts.append({'id': event['id'], 'body_sha256': hashlib.sha256(body).hexdigest(),
                                                     'status': response.status})
                        lost = self.server.drop_next and response.status == 200
                        if lost:
                            self.server.drop_next = False
            self.send_response(response.status)
            for k, v in response.getheaders():
                if k.lower() not in ('content-length', 'transfer-encoding', 'connection', 'server', 'date'):
                    self.send_header(k, v)
            data = b'{}' if lost else data
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        finally:
            conn.close()
