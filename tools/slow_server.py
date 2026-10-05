"""A slow local download server for GUI tests: python slow_server.py PORT SIZE BYTES_PER_SECOND
(per connection, with Range support, like a real file server)."""
import http.server, socketserver, sys, time, re
SIZE = int(sys.argv[2]); RATE = int(sys.argv[3]); CHUNK = b"x" * 65536
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def send_body(self, head):
        r = self.headers.get("Range"); start, end = 0, SIZE - 1
        if r:
            m = re.match(r"bytes=(\d+)-(\d*)", r); start = int(m[1]); end = int(m[2]) if m[2] else SIZE - 1
            self.send_response(206); self.send_header("Content-Range", f"bytes {start}-{end}/{SIZE}")
        else:
            self.send_response(200)
        self.send_header("Accept-Ranges", "bytes"); self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Content-Disposition", 'attachment; filename="ubuntu-test.iso"'); self.end_headers()
        if head: return
        left = end - start + 1
        try:
            while left > 0:
                n = min(left, len(CHUNK)); self.wfile.write(CHUNK[:n]); left -= n; time.sleep(n / RATE)
        except Exception: pass
    def do_GET(self): self.send_body(False)
    def do_HEAD(self): self.send_body(True)
class Srv(socketserver.ThreadingMixIn, http.server.HTTPServer): daemon_threads = True
Srv(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
