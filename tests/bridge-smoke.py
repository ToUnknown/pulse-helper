"""Exercise the packaged bridge without requesting screen/audio capture."""
import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', help='Path to pulse-media or pulse-media.exe')
args = parser.parse_args()
token = 'a' * 64
origin = 'https://pulse.to-unknown.com'
link = f'pulse-capture://share?origin=https%3A%2F%2Fpulse.to-unknown.com&token={token}'
process = subprocess.Popen([args.binary, link], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

def handshake(client_origin, client_token):
    connection = socket.create_connection(('127.0.0.1', 37654), timeout=2)
    key = base64.b64encode(os.urandom(16)).decode()
    request = (f'GET /pulse HTTP/1.1\r\nHost: 127.0.0.1:37654\r\n'
               f'Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n'
               f'Sec-WebSocket-Version: 13\r\nOrigin: {client_origin}\r\n'
               f'Sec-WebSocket-Protocol: pulse-v1, {client_token}\r\n\r\n')
    connection.sendall(request.encode())
    response = b''
    while not response.endswith(b'\r\n\r\n'):
        part = connection.recv(1)
        if not part:
            break
        response += part
    return connection, response

def read_exact(connection, length):
    result = b''
    while len(result) < length:
        part = connection.recv(length - len(result))
        if not part:
            raise AssertionError('Bridge closed before sending a complete frame')
        result += part
    return result

def read_json(connection):
    first, length = read_exact(connection, 2)
    assert first & 15 == 1, f'Expected a text frame, received opcode {first & 15}'
    assert not length & 128, 'Server frames must be unmasked'
    if length == 126:
        length = struct.unpack('!H', read_exact(connection, 2))[0]
    elif length == 127:
        length = struct.unpack('!Q', read_exact(connection, 8))[0]
    return json.loads(read_exact(connection, length))

def send_json(connection, value):
    data = json.dumps(value).encode()
    assert len(data) < 126
    mask = os.urandom(4)
    connection.sendall(bytes([0x81, 0x80 | len(data)]) + mask + bytes(byte ^ mask[i % 4] for i, byte in enumerate(data)))

try:
    deadline = time.monotonic() + 10
    while True:
        assert process.poll() is None, 'Helper exited before connecting'
        try:
            rejected, response = handshake('https://example.invalid', token)
            break
        except ConnectionRefusedError:
            if time.monotonic() > deadline:
                raise
            time.sleep(0.05)
    with rejected:
        assert response.startswith(b'HTTP/1.1 403'), response
    rejected, response = handshake(origin, 'b' * 64)
    with rejected:
        assert response.startswith(b'HTTP/1.1 403'), response
    connection, response = handshake(origin, token)
    with connection:
        assert response.startswith(b'HTTP/1.1 101'), response
        assert b'Sec-WebSocket-Protocol: pulse-v1'.lower() in response.lower(), response
        ready = read_json(connection)
        assert ready['type'] == 'ready' and ready['protocol'] == 1, ready
        send_json(connection, {'type': 'ping'})
        assert read_json(connection)['type'] == 'pong'
        send_json(connection, {'type': 'stop'})
    assert process.wait(timeout=5) == 0, process.stderr.read().decode(errors='replace')
    print('PASS: untrusted origin/token rejected; ready received without confirmation; ping/stop clean. No capture requested.')
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
