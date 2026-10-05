"""Talk to TurboDM like the browser extension does: start `turbodm.exe --native-host`, send a
framed ping and expect the running app to answer (through the named pipe)."""
import json, struct, subprocess, sys

host = subprocess.Popen([sys.argv[1], "--native-host"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
message = json.dumps({"type": "ping"}).encode()
host.stdin.write(struct.pack("<I", len(message)) + message)
host.stdin.flush()
length = struct.unpack("<I", host.stdout.read(4))[0]
reply = json.loads(host.stdout.read(length))
host.stdin.close()
host.wait(10)
print("native host replied:", reply)
assert reply.get("ok") and reply.get("running"), reply
