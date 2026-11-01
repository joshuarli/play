def _check(results, name, action):
    try:
        action()
    except Exception as error:
        results.check(name, False, str(error))
    else:
        results.check(name, True)


def gate_osc_input(results, mpv_class, fixture_entry, fixture_path):
    entry = fixture_entry("h264")
    with mpv_class(["--vo=null", "--ao=null"]) as mpv:
        mpv.load(fixture_path(entry))
        mpv.wait_event("file-loaded", timeout=10)
        mpv.wait_property(
            "time-pos", lambda value: isinstance(value, (int, float)) and value > 0,
            timeout=10,
        )
        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)

        bindings = mpv.get_property("input-bindings")
        osc = [binding for binding in bindings if binding.get("owner") == "osc"]
        commands = {binding.get("cmd", "") for binding in osc}
        results.check("osc-input: key binding",
                      any("osc/toggle-osc" in command for command in commands),
                      str(sorted(commands)[:3]))
        results.check("osc-input: mouse binding",
                      any("mouse_move" in binding.get("key", "").lower()
                          or "mbtn_left" in binding.get("key", "").lower()
                          for binding in osc),
                      str([binding.get("key") for binding in osc][:6]))

        areas = mpv.get_property("user-data/osc/areas")
        results.check("osc-input: hit areas",
                      isinstance(areas, dict) and
                      all(name in areas for name in ("play", "seek", "volume",
                                                     "fullscreen")),
                      str(areas))

        def click(box):
            x = int(box["x"] + box["w"] / 2)
            y = int(box["y"] + box["h"] / 2)
            mpv.command("mouse", x, y)
            mpv.command("keypress", "MBTN_LEFT")

        _check(results, "osc-input: play button", lambda: (
            click(areas["play"]),
            mpv.wait_property("pause", lambda value: value is False, timeout=5),
        ))

        mpv.set_property("pause", True)
        mpv.wait_property("pause", lambda value: value is True, timeout=5)
        _check(results, "osc-input: script binding", lambda: (
            mpv.command("script-binding", "osc/play-pause"),
            mpv.wait_property("pause", lambda value: value is False, timeout=5),
        ))

        seek = areas["seek"]
        seek_x = int(seek["x"] + seek["w"] * 0.75)
        seek_y = int(seek["y"] + seek["h"] / 2)
        _check(results, "osc-input: seek bar", lambda: (
            mpv.command("mouse", seek_x, seek_y),
            mpv.command("keypress", "MBTN_LEFT"),
            mpv.wait_property(
                "time-pos", lambda value: isinstance(value, (int, float))
                and value > 1.0, timeout=5,
            ),
        ))

        _check(results, "osc-input: fullscreen button", lambda: (
            click(areas["fullscreen"]),
            mpv.wait_property("fullscreen", lambda value: value is True, timeout=5),
        ))
        areas = mpv.get_property("user-data/osc/areas")
        _check(results, "osc-input: fullscreen restore", lambda: (
            click(areas["fullscreen"]),
            mpv.wait_property("fullscreen", lambda value: value is False, timeout=5),
        ))

    results.check("osc-input: exit", mpv.returncode == 0, f"exit {mpv.returncode}")
