from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import gzip, json, threading, time

class MockServer:
    def __init__(self):
        self.seen=[]
        self.discovered=[]
        self.active=0
        self.max_active=0
        self.lock=threading.Lock()
        owner=self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args): pass
            def do_GET(self):
                owner.discovered.append({'path':self.path,'headers':dict(self.headers)})
                data=json.dumps({'data':[{'id':'native-mock'},{'id':'native-mock-extra'}]}).encode()
                self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
            def do_POST(self):
                body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                with owner.lock:
                    owner.seen.append({'path':self.path,'body':body,'headers':dict(self.headers),'receivedAtMs':time.time_ns()//1_000_000})
                    # Measure successful batch production separately from cancelled slow handlers.
                    self.counted=body.get('model')!='native-slow'
                    if self.counted:
                        owner.active+=1
                        owner.max_active=max(owner.max_active,owner.active)
                try:
                    if body.get('model')=='native-slow':
                        time.sleep(2)
                    self.respond(body)
                except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                    pass  # Expected when the UI cancellation test drops a streaming request.
                finally:
                    self.finish_active()
            def finish_active(self):
                with owner.lock:
                    if self.counted:
                        owner.active-=1
                        self.counted=False
            def respond(self,body):
                model=body.get('model')
                if model in ['native-diag-error','native-diag-empty','native-diag-truncated','native-diag-timeout']:
                    payload={
                        'native-diag-error': 'data: {"choices":[{"delta":{"content":"partial"}}]}\n\nevent: error\ndata: {"message":"Access forbidden NATIVE_MOCK_KEY"}\n\ndata: [DONE]\n\n',
                        'native-diag-empty': 'data: [DONE]\n\n',
                        'native-diag-truncated': 'data: {"choices":[{"delta":{"content":"partial"}}]}\n\n',
                        'native-diag-timeout': 'data: [DONE]\n\n',
                    }[model].encode()
                    self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.flush()
                    if model=='native-diag-timeout':time.sleep(6)
                    self.finish_active();self.wfile.write(payload);return
                if body.get('stream'):
                    if self.path=='/v1/messages':
                        events=[{'type':'message_start','message':{'usage':{'input_tokens':12,'output_tokens':0}}},{'type':'content_block_delta','delta':{'type':'text_delta','text':'Hello from native mock'}},{'type':'message_delta','usage':{'output_tokens':8}},{'type':'message_stop'}]
                    elif self.path=='/v1/responses':
                        events=[{'type':'response.output_text.delta','delta':'Hello from native mock'},{'type':'response.completed','response':{'usage':{'input_tokens':12,'output_tokens':8}}}]
                    else:
                        events=[{'choices':[{'delta':{'content':'Hello from native mock'}}]},{'choices':[],'usage':{'prompt_tokens':12,'completion_tokens':8}},'[DONE]']
                    payload=''.join('data: '+(json.dumps(e) if isinstance(e,dict) else e)+'\n\n' for e in events).encode()
                    self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(payload)));self.end_headers()
                    for offset in range(0,len(payload),19):
                        # Count response production, not handler cleanup after Content-Length is complete.
                        last=offset+19>=len(payload)
                        if last: self.finish_active()
                        self.wfile.write(payload[offset:offset+19]);self.wfile.flush()
                        if not last: time.sleep(.003)
                else:
                    payload=gzip.compress(json.dumps({'choices':[{'message':{'role':'assistant','content':'Hello from native mock'}}],'usage':{'prompt_tokens':12,'completion_tokens':8}}).encode())
                    self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Encoding','gzip');self.send_header('Content-Length',str(len(payload)));self.end_headers();self.finish_active();self.wfile.write(payload)
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True)
        self.thread.start()
    @property
    def url(self): return f'http://127.0.0.1:{self.server.server_port}/v1'
    def close(self):self.server.shutdown();self.server.server_close();self.thread.join()
