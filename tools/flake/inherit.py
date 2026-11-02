# One thread: open a listening socket on a port, close it, connect to the port at once.
# Another: start processes as fast as it can. If a start copies the probe while it is
# open, the port keeps listening in the half-started child until it execs: the connect
# succeeds and is then reset. Without the spawner, every connect must be refused.
import socket, subprocess, sys, threading, time
spawn = sys.argv[1] == 'spawn'
stop = time.time() + 20
counts = {'refused': 0, 'connected': 0, 'reset': 0}
def spawner():
    while time.time() < stop:
        subprocess.Popen(['true']).wait()
if spawn:
    for _ in range(4): threading.Thread(target=spawner, daemon=True).start()
port = 23456
while time.time() < stop:
    p = socket.socket(); p.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    p.bind(('127.0.0.1', port)); p.listen(); p.close()
    c = socket.socket()
    try:
        c.connect(('127.0.0.1', port)); counts['connected'] += 1
        c.settimeout(5)
        try:
            if c.recv(1) == b'': pass
        except ConnectionResetError: counts['reset'] += 1
        except socket.timeout: pass
    except ConnectionRefusedError: counts['refused'] += 1
    except ConnectionResetError: counts['reset'] += 1
    c.close()
print('with spawner' if spawn else 'no spawner  ', counts)
