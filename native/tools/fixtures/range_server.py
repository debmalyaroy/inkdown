"""A tiny web server for the UI tests: serves one folder on 127.0.0.1 only.

Usage: python range_server.py FOLDER PORT_FILE [--no-range]

It answers `Range` requests (with `If-Range`), sends `ETag` and honours
`If-None-Match`, which is what the "open from a web address" feature relies on.
With --no-range it behaves like a plain static server. The port it got is written
to PORT_FILE. It stops when PORT_FILE is deleted.
"""
import hashlib
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = os.path.abspath(sys.argv[1])
port_file = sys.argv[2]
ranges = '--no-range' not in sys.argv


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        name = self.path.split('?')[0].lstrip('/')
        if name == 'stream.log':
            # like a log endpoint that stays open: two lines at once, then one every 0.4 s
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.end_headers()
            try:
                self.wfile.write(b'2026-11-15T10:00:00.000Z INFO  [stream] connected\n2026-11-15T10:00:00.100Z WARN  [stream] first lines\n')
                self.wfile.flush()
                for i in range(40):
                    if not os.path.exists(port_file):
                        break
                    time.sleep(0.4)
                    # a euro sign split over two writes: half a character must wait for the rest
                    self.wfile.write(('2026-11-15T10:00:%02d.000Z ERROR [stream] tick %d costs ' % (i + 1, i)).encode() + b'\xe2\x82')
                    self.wfile.flush()
                    time.sleep(0.05)
                    self.wfile.write(b'\xac5\r\n')
                    self.wfile.flush()
            except OSError:
                pass
            return
        path = os.path.abspath(os.path.join(root, name))
        if not path.startswith(root) or not os.path.isfile(path):
            self.send_response(404)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        with open(path, 'rb') as f:
            data = f.read()
        etag = '"%s"' % hashlib.sha1(data).hexdigest()[:16]
        if self.headers.get('If-None-Match') == etag:
            self.send_response(304)
            self.send_header('ETag', etag)
            self.end_headers()
            return
        rng = self.headers.get('Range') if ranges else None
        # like real servers: a partial answer only if the validator still matches... or,
        # for a file that only grows, when the client sends none
        if_range = self.headers.get('If-Range')
        start = None
        if rng and rng.startswith('bytes=') and rng.endswith('-'):
            start = int(rng[6:-1])
        if start is not None and start >= len(data) and (if_range is None or if_range == etag):
            self.send_response(416)
            self.send_header('Content-Range', 'bytes */%d' % len(data))
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        # this test server treats a log as append-only: the old validator is accepted
        if start is not None and start < len(data):
            body = data[start:]
            self.send_response(206)
            self.send_header('Content-Range', 'bytes %d-%d/%d' % (start, len(data) - 1, len(data)))
        else:
            body = data
            self.send_response(200)
        self.send_header('ETag', etag)
        self.send_header('Content-Type', 'text/plain; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
with open(port_file, 'w') as f:
    f.write(str(server.server_address[1]))
threading.Thread(target=server.serve_forever, daemon=True).start()
while os.path.exists(port_file):
    time.sleep(0.3)
server.shutdown()
