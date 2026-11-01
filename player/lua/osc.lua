local assdraw = require 'mp.assdraw'
local opt = require 'mp.options'

local user_opts = {
    hidetimeout = 500,
    fadeduration = 200,
    visibility = "auto",
    scalewindowed = 1,
    scalefullscreen = 1,
    volume_step = 5,
}

opt.read_options(user_opts, "osc")

local icon_font = "mpv-osd-symbols"
local icons = {
    play = "\238\132\129",
    pause = "\238\128\130",
    volume = {"\238\132\139", "\238\132\140", "\238\132\141", "\238\132\142"},
    mute = "\238\132\138",
    audio = "\238\132\134",
    subtitle = "\238\132\135",
    fullscreen = "\238\132\136",
    exit_fullscreen = "\238\132\137",
}

local state = {
    visible = false,
    mouse_in_window = false,
    dragging = false,
    hover_seek = nil,
    paused = false,
    fullscreen = false,
    muted = false,
    volume = 100,
    duration = 0,
    position = 0,
    audio_count = 0,
    sub_count = 0,
    aid = "-",
    sid = "-",
}

local layout = {}
local overlay = mp.create_osd_overlay("ass-events")
local visibility = user_opts.visibility
local tick_timer = nil
local hide_timer = nil

local function format_time(seconds)
    if seconds == nil or seconds < 0 then
        return "--:--"
    end
    return mp.format_time(seconds, seconds >= 3600 and "%H:%M:%S" or "%M:%S")
end

local function osd_size()
    local width, height, aspect = mp.get_osd_size()
    if width <= 0 or height <= 0 then
        return 1280, 720, 16 / 9, true
    end
    return width, height, aspect > 0 and aspect or width / height, false
end

local function update_state()
    state.paused = mp.get_property_native("pause", false)
    state.fullscreen = mp.get_property_native("fullscreen", false)
    state.muted = mp.get_property_native("mute", false)
    state.volume = mp.get_property_number("volume", 100)
    state.duration = mp.get_property_number("duration", 0)
    state.position = mp.get_property_number("time-pos", 0)
    state.aid = mp.get_property("aid", "-")
    state.sid = mp.get_property("sid", "-")
    state.audio_count = 0
    state.sub_count = 0
    for _, track in ipairs(mp.get_property_native("track-list", {})) do
        if track.type == "audio" then
            state.audio_count = state.audio_count + 1
        elseif track.type == "sub" then
            state.sub_count = state.sub_count + 1
        end
    end
end

local function update_layout()
    local width, height, aspect, headless = osd_size()
    update_state()
    local scale = math.max(1, height / 720)
    if state.fullscreen then
        scale = scale * user_opts.scalefullscreen
    else
        scale = scale * user_opts.scalewindowed
    end
    local pad = math.floor(12 * scale)
    local button = math.floor(40 * scale)
    local bar_height = math.floor(64 * scale)
    local bar_y = height - bar_height - math.floor(8 * scale)
    local right = width - pad
    local fullscreen = {x = right - button, y = bar_y + (bar_height - button) / 2,
                        w = button, h = button}
    right = fullscreen.x - pad
    local sub = nil
    if state.sub_count > 0 then
        sub = {x = right - button, y = fullscreen.y, w = button, h = button}
        right = sub.x - pad
    end
    local audio = nil
    if state.audio_count > 1 then
        audio = {x = right - button, y = fullscreen.y, w = button, h = button}
        right = audio.x - pad
    end
    local volume = {x = right - button, y = fullscreen.y, w = button, h = button}
    right = volume.x - pad
    local time_width = math.floor(70 * scale)
    local duration = {x = right - time_width, y = bar_y, w = time_width, h = bar_height}
    local play = {x = pad, y = bar_y + (bar_height - button) / 2, w = button, h = button}
    local elapsed = {x = play.x + button + pad, y = bar_y, w = time_width, h = bar_height}
    local seek_start = elapsed.x + time_width + pad
    local seek_end = duration.x - pad
    local seek_height = math.floor(12 * scale)
    local seek = {x = seek_start, y = bar_y + (bar_height - seek_height) / 2,
                  w = math.max(40, seek_end - seek_start), h = seek_height}

    layout = {
        width = width, height = height, aspect = aspect, headless = headless,
        scale = scale, pad = pad, button = button, bar_height = bar_height,
        bar_y = bar_y, play = play, elapsed = elapsed, seek = seek,
        duration = duration, volume = volume, audio = audio, sub = sub,
        fullscreen = fullscreen,
    }

    mp.set_property_native("user-data/osc/margins", {l = 0, r = 0, t = 0, b = 0})
    mp.set_property_native("user-data/osc/areas", {
        play = play, seek = seek, volume = volume,
        audio = audio, sub = sub, fullscreen = fullscreen,
    })

    if not headless then
        mp.set_mouse_area(0, bar_y, width, height, "showhide")
    end
end

local function volume_icon()
    if state.muted or state.volume <= 0 then
        return icons.mute
    end
    if state.volume <= 33 then
        return icons.volume[1]
    elseif state.volume <= 66 then
        return icons.volume[2]
    elseif state.volume <= 99 then
        return icons.volume[3]
    end
    return icons.volume[4]
end

local function add_text(ass, x, y, size, content, font)
    ass:new_event()
    ass:append(string.format("{\\an7\\pos(%d,%d)\\fs%d\\bord0\\shad0\\1c&HFFFFFF&%s}%s",
                             x, y, size, font and ("\\fn" .. font) or "", content))
end

local function render()
    if not state.visible then
        overlay.data = ""
        overlay:update()
        return
    end
    update_state()
    local ass = assdraw.ass_new()
    local bar_top = layout.bar_y
    local bar_bottom = layout.bar_y + layout.bar_height
    local fade = math.max(0, math.floor(user_opts.fadeduration / 2))
    local fade_tag = fade > 0 and string.format("\\fad(%d,%d)", fade, fade) or ""

    ass:new_event()
    ass:append("{\\an7\\pos(0,0)\\bord0\\shad0\\1c&H000000&\\alpha&H70&" .. fade_tag .. "}")
    ass:draw_start()
    ass:round_rect_cw(layout.pad, bar_top, layout.width - layout.pad, bar_bottom, 8)
    ass:draw_stop()

    local seek = layout.seek
    ass:new_event()
    ass:append("{\\bord0\\shad0\\1c&H333333&" .. fade_tag .. "}")
    ass:draw_start()
    ass:rect_cw(seek.x, seek.y, seek.x + seek.w, seek.y + seek.h)
    ass:draw_stop()

    local progress = 0
    if state.duration > 0 then
        progress = math.max(0, math.min(1, state.position / state.duration))
    end
    ass:new_event()
    ass:append("{\\bord0\\shad0\\1c&HFFFFFF&" .. fade_tag .. "}")
    ass:draw_start()
    ass:rect_cw(seek.x, seek.y, seek.x + seek.w * progress, seek.y + seek.h)
    ass:draw_stop()

    local text_y = bar_top + (layout.bar_height - math.floor(20 * layout.scale)) / 2
    add_text(ass, layout.play.x + math.floor(6 * layout.scale), text_y,
             math.floor(28 * layout.scale), state.paused and icons.play or icons.pause,
             icon_font)
    add_text(ass, layout.elapsed.x, text_y, math.floor(20 * layout.scale),
             format_time(state.position))
    add_text(ass, layout.duration.x, text_y, math.floor(20 * layout.scale),
             format_time(state.duration))
    add_text(ass, layout.volume.x + math.floor(2 * layout.scale), text_y,
             math.floor(24 * layout.scale),
             volume_icon() .. string.format(" %d%%", math.floor(state.volume)), icon_font)
    if layout.audio then
        add_text(ass, layout.audio.x, text_y, math.floor(24 * layout.scale),
                 icons.audio .. " " .. tostring(state.aid), icon_font)
    end
    if layout.sub then
        add_text(ass, layout.sub.x, text_y, math.floor(24 * layout.scale),
                 icons.subtitle .. " " .. tostring(state.sid), icon_font)
    end
    add_text(ass, layout.fullscreen.x + math.floor(2 * layout.scale), text_y,
             math.floor(28 * layout.scale),
             state.fullscreen and icons.exit_fullscreen or icons.fullscreen, icon_font)

    if state.hover_seek and state.mouse_in_window then
        local tooltip = format_time(state.hover_seek)
        local tooltip_x = seek.x + seek.w * (state.duration > 0 and state.position / state.duration or 0)
        ass:new_event()
        ass:append(string.format("{\\an2\\pos(%d,%d)\\fs%d\\bord1\\shad0\\1c&HFFFFFF&\\3c&H000000&}%s",
                                 tooltip_x, seek.y - 6, math.floor(18 * layout.scale), tooltip))
    end

    overlay.res_x = layout.width
    overlay.res_y = layout.height
    overlay.data = ass.text
    overlay:update()
end

local function tick()
    tick_timer = nil
    render()
end

local function request_tick()
    if tick_timer == nil then
        tick_timer = mp.add_timeout(0.05, tick)
    elseif not tick_timer:is_enabled() then
        tick_timer:resume()
    end
end

local function hide()
    state.visible = false
    if hide_timer then
        hide_timer:kill()
        hide_timer = nil
    end
    if tick_timer then
        tick_timer:kill()
        tick_timer = nil
    end
    overlay.data = ""
    overlay:update()
end

local function schedule_hide()
    if visibility ~= "auto" then
        return
    end
    if hide_timer then
        hide_timer:kill()
    end
    hide_timer = mp.add_timeout(user_opts.hidetimeout / 1000, function()
        hide_timer = nil
        if state.dragging or state.mouse_in_window then
            schedule_hide()
        else
            hide()
        end
    end)
end

local function show()
    if visibility == "never" then
        return
    end
    state.visible = true
    schedule_hide()
    request_tick()
end

local function visibility_mode(mode)
    if mode == "cycle" then
        if visibility == "auto" then
            mode = "always"
        elseif visibility == "always" then
            mode = "never"
        else
            mode = "auto"
        end
    end
    if mode ~= "auto" and mode ~= "always" and mode ~= "never" then
        return
    end
    visibility = mode
    mp.set_property_native("user-data/osc/visibility", mode)
    if mode == "never" then
        hide()
    else
        show()
    end
    mp.osd_message("OSC visibility: " .. mode)
end

local function in_box(box, x, y)
    return box ~= nil and x >= box.x and x <= box.x + box.w
        and y >= box.y and y <= box.y + box.h
end

local function seek_to(x)
    if state.duration <= 0 or layout.seek.w <= 0 then
        return
    end
    local ratio = math.max(0, math.min(1, (x - layout.seek.x) / layout.seek.w))
    mp.commandv("seek", ratio * state.duration, "absolute+exact")
end

local function mouse_move()
    state.mouse_in_window = true
    local x, y = mp.get_mouse_pos()
    if state.dragging then
        seek_to(x)
    end
    if in_box(layout.seek, x, y) and state.duration > 0 then
        local ratio = math.max(0, math.min(1, (x - layout.seek.x) / layout.seek.w))
        state.hover_seek = ratio * state.duration
    else
        state.hover_seek = nil
    end
    show()
    request_tick()
end

local function mouse_leave()
    state.mouse_in_window = false
    state.hover_seek = nil
    schedule_hide()
    request_tick()
end

local function button_up()
    local x, y = mp.get_mouse_pos()
    if state.dragging then
        state.dragging = false
        seek_to(x)
        request_tick()
        return
    end
    if in_box(layout.play, x, y) then
        mp.commandv("cycle", "pause")
    elseif layout.audio and in_box(layout.audio, x, y) then
        mp.commandv("cycle", "audio")
    elseif layout.sub and in_box(layout.sub, x, y) then
        mp.commandv("cycle", "sub")
    elseif in_box(layout.volume, x, y) then
        mp.commandv("cycle", "mute")
    elseif in_box(layout.fullscreen, x, y) then
        mp.commandv("cycle", "fullscreen")
    elseif in_box(layout.seek, x, y) then
        seek_to(x)
    end
    request_tick()
end

local function button_down()
    local x, y = mp.get_mouse_pos()
    if in_box(layout.seek, x, y) then
        state.dragging = true
        seek_to(x)
        request_tick()
    end
end

local function mouse_wheel(direction)
    local x, y = mp.get_mouse_pos()
    if in_box(layout.volume, x, y) then
        local delta = direction == "up" and user_opts.volume_step or -user_opts.volume_step
        mp.commandv("add", "volume", delta)
    elseif in_box(layout.seek, x, y) then
        mp.commandv("seek", direction == "up" and 5 or -5)
    end
    request_tick()
end

mp.observe_property("pause", "bool", request_tick)
mp.observe_property("fullscreen", "bool", function()
    update_layout()
    request_tick()
end)
mp.observe_property("volume", "number", request_tick)
mp.observe_property("mute", "bool", request_tick)
mp.observe_property("duration", "number", request_tick)
mp.observe_property("time-pos", "number", request_tick)
mp.observe_property("track-list", "native", function()
    update_layout()
    request_tick()
end)
mp.observe_property("osd-dimensions", "native", function()
    update_layout()
    request_tick()
end)

mp.register_event("start-file", function()
    state.dragging = false
    state.hover_seek = nil
    update_layout()
    show()
end)
mp.register_event("file-loaded", function()
    update_layout()
    request_tick()
end)
mp.register_event("shutdown", function()
    hide()
end)

mp.register_script_message("osc-message", function(message, duration)
    mp.osd_message(message, duration)
end)
mp.register_script_message("osc-visibility", visibility_mode)
mp.register_script_message("osc-show", show)
mp.register_script_message("osc-hide", hide)

mp.add_key_binding("o", "toggle-osc", function()
    visibility_mode("cycle")
end)
mp.add_key_binding(nil, "play-pause", function()
    mp.commandv("cycle", "pause")
end)
mp.add_key_binding(nil, "seek-forward", function()
    mp.commandv("seek", 1, "relative")
end)
mp.add_key_binding(nil, "seek-backward", function()
    mp.commandv("seek", -1, "relative")
end)
mp.add_key_binding(nil, "toggle-fullscreen", function()
    mp.commandv("cycle", "fullscreen")
end)

mp.set_key_bindings({
    {"mouse_move", mouse_move},
    {"mouse_leave", mouse_leave},
}, "showhide", "force")
mp.set_key_bindings({
    {"mbtn_left", button_up, button_down},
    {"mbtn_left_dbl", function() mp.commandv("cycle", "fullscreen") end},
    {"wheel_up", function() mouse_wheel("up") end},
    {"wheel_down", function() mouse_wheel("down") end},
}, "input", "force")
mp.enable_key_bindings("showhide", "allow-vo-dragging+allow-hide-cursor")
mp.enable_key_bindings("input")

update_layout()
mp.set_property_native("user-data/osc/visibility", visibility)
if visibility == "never" then
    hide()
else
    show()
end
