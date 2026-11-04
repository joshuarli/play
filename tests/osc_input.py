import time


NEAREST_KEYFRAME = 0.55


def _check(results, name, action):
    try:
        action()
    except Exception as error:
        results.check(name, False, str(error))
    else:
        results.check(name, True)


def _center(box):
    return int(box["x"] + box["w"] / 2), int(box["y"] + box["h"] / 2)


def _click(mpv, box):
    mpv.command("mouse", *_center(box))
    mpv.command("keypress", "MBTN_LEFT")


def _seek_x(box, fraction):
    return int(box["x"] + box["w"] * fraction)


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _inside(inner, outer):
    return (inner["x"] >= outer["x"] and inner["y"] >= outer["y"]
            and inner["x"] + inner["w"] <= outer["x"] + outer["w"]
            and inner["y"] + inner["h"] <= outer["y"] + outer["h"])


def _overlap(left, right):
    return not (left["x"] + left["w"] <= right["x"] or right["x"] + right["w"] <= left["x"]
                or left["y"] + left["h"] <= right["y"] or right["y"] + right["h"] <= left["y"])


def gate_osc_input(results, mpv_class, fixture_entry, fixture_path):
    playback_controls(results, mpv_class, fixture_entry, fixture_path)
    track_controls(results, mpv_class, fixture_entry, fixture_path)
    pointer_visibility(results, mpv_class, fixture_entry, fixture_path)


def playback_controls(results, mpv_class, fixture_entry, fixture_path):
    entry = fixture_entry("h264")
    duration = float(entry["duration"])
    with mpv_class(["--vo=null", "--ao=null", "--keep-open=yes"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property("time-pos", lambda value: _number(value) and value > 0, timeout=10)
        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        mpv.command("set", "osc-visibility", "always")

        areas = mpv.get_property("osc-areas")
        results.check("osc-input: hit areas",
                      isinstance(areas, dict)
                      and set(areas) == {"play", "seek", "volume", "fullscreen", "audio",
                                         "time_elapsed", "time_remaining"},
                      str(areas))
        results.check("osc-input: 720p geometry",
                      areas.get("play") == {"x": 7, "y": 690, "w": 27, "h": 30}
                      and areas["fullscreen"]["x"] + areas["fullscreen"]["w"] == 1273
                      and areas["seek"] == {"x": 234, "y": 690, "w": 640, "h": 30},
                      str(areas))
        boxes = [areas[name] for name in ("play", "seek", "volume", "fullscreen")]
        results.check("osc-input: controls do not overlap",
                      not any(_overlap(left, right)
                              for index, left in enumerate(boxes)
                              for right in boxes[index + 1:]),
                      str(boxes))

        _check(results, "osc-input: play button", lambda: (
            _click(mpv, areas["play"]),
            mpv.wait_property("pause", lambda value: value is False, timeout=5),
        ))
        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        _check(results, "osc-input: pause button", lambda: (
            _click(mpv, areas["play"]),
            mpv.wait_property("pause", lambda value: value is False, timeout=5),
            _click(mpv, areas["play"]),
            mpv.wait_property("pause", lambda value: value is True, timeout=5),
        ))

        seek = areas["seek"]
        seek_y = _center(seek)[1]
        _check(results, "osc-input: seek click", lambda: (
            mpv.command("mouse", _seek_x(seek, 0.75), seek_y),
            mpv.command("keypress", "MBTN_LEFT"),
            mpv.wait_property(
                "time-pos", lambda value: _number(value)
                and abs(value - duration * 0.75) <= NEAREST_KEYFRAME, timeout=5),
        ))
        _check(results, "osc-input: seek drag", lambda: (
            mpv.command("mouse", _seek_x(seek, 0.1), seek_y),
            mpv.command("keydown", "MBTN_LEFT"),
            mpv.wait_property(
                "time-pos", lambda value: _number(value)
                and abs(value - duration * 0.1) <= NEAREST_KEYFRAME, timeout=5),
            mpv.command("mouse", _seek_x(seek, 0.5), seek_y - 200),
            mpv.wait_property(
                "time-pos", lambda value: _number(value)
                and abs(value - duration * 0.5) <= NEAREST_KEYFRAME, timeout=5),
            mpv.command("keyup", "MBTN_LEFT"),
            mpv.command("mouse", _seek_x(seek, 0.05), seek_y - 200),
            time.sleep(0.3),
            mpv.wait_property(
                "time-pos", lambda value: _number(value)
                and abs(value - duration * 0.5) <= NEAREST_KEYFRAME, timeout=5),
        ))

        _check(results, "osc-input: mute button", lambda: (
            _click(mpv, areas["volume"]),
            mpv.wait_property("mute", lambda value: value is True, timeout=5),
            _click(mpv, areas["volume"]),
            mpv.wait_property("mute", lambda value: value is False, timeout=5),
        ))
        _check(results, "osc-input: volume wheel", lambda: (
            mpv.command("mouse", *_center(areas["volume"])),
            mpv.command("keypress", "WHEEL_DOWN"),
            mpv.wait_property("volume", lambda value: _number(value)
                              and abs(value - 95) < 0.5, timeout=5),
            mpv.command("keypress", "WHEEL_UP"),
            mpv.wait_property("volume", lambda value: _number(value)
                              and abs(value - 100) < 0.5, timeout=5),
        ))

        _check(results, "osc-input: fullscreen button", lambda: (
            _click(mpv, areas["fullscreen"]),
            mpv.wait_property("fullscreen", lambda value: value is True, timeout=5),
        ))
        areas = mpv.get_property("osc-areas")
        _check(results, "osc-input: fullscreen restore", lambda: (
            _click(mpv, areas["fullscreen"]),
            mpv.wait_property("fullscreen", lambda value: value is False, timeout=5),
        ))

        mpv.command("set", "window-scale", "2")
        resized = mpv.wait_property(
            "osc-areas", lambda value: abs(value.get("play", {}).get("w", 0) - 18) < 0.01
            and abs(value["play"]["y"] - 460) < 0.01, timeout=5)
        dims = mpv.get_property("osd-dimensions")
        results.check("osc-input: layout follows the window size",
                      dims.get("w") == 640 and dims.get("h") == 480
                      and _inside(resized["fullscreen"], {"x": 0, "y": 0, "w": 640, "h": 480}),
                      f"{dims} {resized}")
        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        _check(results, "osc-input: play button after resize", lambda: (
            _click(mpv, resized["play"]),
            mpv.wait_property("pause", lambda value: value is False, timeout=5),
        ))

        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        mpv.command("set", "osc-visibility", "never")
        _click(mpv, resized["play"])
        time.sleep(0.3)
        results.check("osc-input: hidden osc ignores clicks",
                      mpv.get_property("pause") is True, "pause toggled while hidden")

    results.check("osc-input: exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def track_controls(results, mpv_class, fixture_entry, fixture_path):
    entry = fixture_entry("multiaudio")
    subtitle = fixture_path(fixture_entry("external-srt"))
    with mpv_class(["--vo=null", "--ao=null", "--keep-open=yes", "--pause=yes"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.command("set", "osc-visibility", "always")
        areas = mpv.wait_property("osc-areas", lambda value: "audio" in value, timeout=5)
        results.check("osc-input: audio control with two audio tracks", "audio" in areas,
                      str(areas))
        _check(results, "osc-input: audio button cycles", lambda: (
            mpv.wait_property("aid", lambda value: value == 1, timeout=5),
            _click(mpv, areas["audio"]),
            mpv.wait_property("aid", lambda value: value == 2, timeout=5),
        ))
        mpv.command("sub-add", str(subtitle))
        areas = mpv.wait_property("osc-areas", lambda value: "sub" in value, timeout=5)
        results.check("osc-input: subtitle control after sub-add",
                      _inside(areas["sub"], {"x": 0, "y": 0, "w": 1280, "h": 720})
                      and areas["audio"]["x"] < areas["sub"]["x"] < areas["fullscreen"]["x"],
                      str(areas))
        _check(results, "osc-input: subtitle button cycles", lambda: (
            mpv.wait_property("sid", lambda value: value == 1, timeout=5),
            _click(mpv, areas["sub"]),
            mpv.wait_property("sid", lambda value: value is False, timeout=5),
            _click(mpv, areas["sub"]),
            mpv.wait_property("sid", lambda value: value == 1, timeout=5),
        ))
        _check(results, "osc-input: cycle sub over IPC", lambda: (
            mpv.command("cycle", "sub"),
            mpv.wait_property("sid", lambda value: value is False, timeout=5),
            mpv.command("keypress", "j"),
            mpv.wait_property("sid", lambda value: value == 1, timeout=5),
        ))
    results.check("osc-input: tracks exit", mpv.returncode == 0, f"exit {mpv.returncode}")


def pointer_visibility(results, mpv_class, fixture_entry, fixture_path):
    entry = fixture_entry("h264")
    with mpv_class(["--vo=null", "--ao=null", "--keep-open=yes", "--pause=yes"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        results.check("osc-input: shown at file start",
                      mpv.get_property("osc-visibility") == "auto"
                      and mpv.get_property("osc-visible") is True,
                      str(mpv.get_property("osc-visible")))
        mpv.wait_property("osc-visible", lambda value: value is False, timeout=5)

        mpv.command("mouse", 640, 100)
        results.check("osc-input: motion above the dead zone does not show",
                      mpv.get_property("osc-visible") is False, "shown by distant motion")
        mpv.command("mouse", 640, 400)
        shown = time.monotonic()
        results.check("osc-input: pointer motion shows",
                      mpv.get_property("osc-visible") is True, "still hidden")
        mpv.wait_property("osc-visible", lambda value: value is False, timeout=5)
        elapsed = time.monotonic() - shown
        results.check("osc-input: timed hide", 0.4 <= elapsed <= 2.0, f"{elapsed:.2f} s")

        areas = mpv.get_property("osc-areas")
        mpv.command("mouse", *_center(areas["play"]))
        time.sleep(1.2)
        results.check("osc-input: hovering the bar keeps it shown",
                      mpv.get_property("osc-visible") is True, "hid under the pointer")
        mpv.command("mouse-leave")
        _check(results, "osc-input: pointer leave hides", lambda: (
            mpv.wait_property("osc-visible", lambda value: value is False, timeout=1),
        ))

        mpv.command("set", "osc-visibility", "always")
        time.sleep(1.0)
        results.check("osc-input: always stays shown",
                      mpv.get_property("osc-visible") is True, "always hid")
        mpv.command("set", "osc-visibility", "never")
        mpv.command("mouse", 640, 400)
        results.check("osc-input: never stays hidden",
                      mpv.get_property("osc-visible") is False, "never showed")
    results.check("osc-input: pointer exit", mpv.returncode == 0, f"exit {mpv.returncode}")
