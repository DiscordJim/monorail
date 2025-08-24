#!/usr/bin/env python3
import socket

HOST = "127.0.0.1"
PORT = 6942

def main():
    with socket.create_connection((HOST, PORT), timeout=5) as s:
        s.sendall(b"yo")
        # if the server is echoing, read a reply (optional)
        try:
            data = s.recv(4096)
            if data:
                print("got:", data.decode(errors="replace"))
        except socket.timeout:
            pass

if __name__ == "__main__":
    main()