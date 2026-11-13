#!/usr/bin/env bun
// Loopback-only audio meter. A session acknowledgement is simulated; recognition is not.
const server = Bun.serve({
  hostname: '127.0.0.1', port: 0,
  fetch(req, s) { return s.upgrade(req) ? undefined : new Response('mock only', {status: 426}); },
  websocket: {
    open() { console.log(JSON.stringify({type:'connection_open'})); },
    message(ws, raw) {
      const event = JSON.parse(String(raw));
      if (event.type === 'session.update') {
        console.log(JSON.stringify({type:'session_update', mock:true}));
        ws.send(JSON.stringify({type:'session.updated'}));
      } else if (event.type === 'input_audio_buffer.append') {
        const data = Buffer.from(event.audio, 'base64');
        let peak = 0, nonzero = 0;
        for (let i = 0; i + 1 < data.length; i += 2) {
          const value = Math.abs(data.readInt16LE(i));
          peak = Math.max(peak, value); if (value) nonzero++;
        }
        console.log(JSON.stringify({type:'audio', bytes:data.length, nonzero, peak}));
      }
    },
    close() { console.log(JSON.stringify({type:'connection_closed'})); }
  }
});
console.log(JSON.stringify({type:'listening',port:server.port,mock:true}));
