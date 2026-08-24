import http.server, threading
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        code = 200 if self.path == "/healthz" else 404
        self.send_response(code); self.end_headers()
    def log_message(self, *a): pass
srv1 = http.server.HTTPServer(("127.0.0.1", 9101), H)
srv2 = http.server.HTTPServer(("127.0.0.1", 9102), H)
threading.Thread(target=srv1.serve_forever, daemon=True).start()
srv2.serve_forever()   # serves /healthz correctly
