import sys

sys.path.insert(0, "/work/tests")

from harness import Mpv, ROOT
from wayland import GPU_ARGS, Compositor

compositor = Compositor()
try:
    with Mpv(GPU_ARGS, env=compositor.env) as player:
        player.load(ROOT / ".work/fixtures/h264.mp4")
        player.wait_event("file-loaded", timeout=300)
        player.wait_property("osd-width", lambda value: value > 0, timeout=300)
        player.command("show-text", "gpu-next osd", 2000)
        print("dims", player.get_property("osd-dimensions"), flush=True)
        print("dims2", player.get_property("osd-dimensions"), flush=True)
        print("clean", flush=True)
finally:
    compositor.close()
