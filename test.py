#!/usr/bin/env python3
import asyncio
import signal

HOST = "127.0.0.1"
PORT = 6968

async def handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    addr = writer.get_extra_info("peername")
    print(f"Connected: {addr}")
    try:
        while data := await reader.read(4096):
            print(data)
            writer.write(data)            # echo back exactly what we got
            await writer.drain()
    except ConnectionResetError:
        pass
    finally:
        print(f"Disconnected: {addr}")
        writer.close()
        await writer.wait_closed()

async def main():
    server = await asyncio.start_server(handle, HOST, PORT)
    addrs = ", ".join(str(s.getsockname()) for s in server.sockets)
    print(f"Echo server listening on {addrs}")

    # Graceful shutdown (Ctrl+C / SIGTERM)
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, stop.set)
        except NotImplementedError:
            # Windows may not support signal handlers in asyncio
            pass

    async with server:
        await stop.wait()

if __name__ == "__main__":
    asyncio.run(main())
