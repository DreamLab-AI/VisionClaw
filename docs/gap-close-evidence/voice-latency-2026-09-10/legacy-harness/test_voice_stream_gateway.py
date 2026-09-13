"""Protocol regression tests; no models, Docker, microphone or GPU required."""
import asyncio
import struct
import unittest
from unittest.mock import patch

from aiohttp import web, WSMsgType
from aiohttp.test_utils import TestClient, TestServer
from scripts import voice_stream_gateway as gateway


class GatewayTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancellation_fences_audio_and_closes_upstream(self):
        closed = asyncio.Event()
        engines = []

        async def fake_stream(session, engine, text):
            engines.append(engine)
            try:
                for _ in range(100):
                    yield struct.pack("<h", 1000) * 480
            finally:
                closed.set()

        app = web.Application()
        app.router.add_get("/voice", gateway.voice)
        async with TestClient(TestServer(app)) as client:
            with patch.object(gateway, "pcm_stream", fake_stream):
                async with client.ws_connect("/voice") as ws:
                    await ws.send_json({"type": "speak", "text": "first"})
                    started = await ws.receive_json()
                    self.assertEqual(started["type"], "started")
                    first = await ws.receive()
                    self.assertEqual(first.type, WSMsgType.BINARY)
                    await ws.send_json({"type": "cancel"})
                    while True:
                        event = await ws.receive()
                        if event.type == WSMsgType.TEXT:
                            self.assertIn('"cancelled"', event.data)
                            break
                    await asyncio.wait_for(closed.wait(), 1)
                    await ws.send_json({"type": "speak", "text": "replacement"})
                    replacement = await ws.receive_json()
                    frame = await ws.receive()
                    self.assertEqual(frame.type, WSMsgType.BINARY)
                    self.assertEqual(struct.unpack("!I", frame.data[:4])[0], replacement["generation"])
                    self.assertGreater(replacement["generation"], started["generation"])
                    self.assertEqual(engines, ["pocket", "pocket"])


if __name__ == "__main__":
    unittest.main()
