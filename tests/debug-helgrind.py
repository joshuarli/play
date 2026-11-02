import sys

sys.path.insert(0, "/work/tests")

from harness import Mpv
from wayland import GPU_ARGS, Compositor, fixture_1080p

compositor = Compositor()
try:
    with Mpv(GPU_ARGS, env=compositor.env) as player:
        player.load(fixture_1080p())
        player.wait_event("file-loaded", timeout=30)
        player.wait_property("osd-width", lambda value: value > 0, timeout=30)
        player.command("show-text", "gpu-next osd", 2000)
        dims = player.get_property("osd-dimensions")
        print("dims", dims, flush=True)
        windowed = player.get_property("osd-dimensions")["w"]
        player.set_property("fullscreen", True)
        fullscreen = player.wait_property(
            "osd-dimensions", lambda value: value["w"] != windowed, timeout=30)["w"]
        player.set_property("fullscreen", False)
        player.wait_property(
            "osd-dimensions", lambda value: value["w"] == windowed, timeout=30)
        player.set_property("window-scale", 0.5)
        player.wait_property(
            "osd-dimensions", lambda value: value["w"] != windowed, timeout=30)
        print("clean", flush=True)
finally:
    compositor.close()
