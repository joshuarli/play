local directory = assert(os.getenv("OSD_CAPTURE_DIR"), "OSD_CAPTURE_DIR is not set")

local function write(path, data)
    local file = assert(io.open(path, "wb"))
    file:write(data)
    file:close()
end

local function write_meta(path, width, height, stride)
    local file = assert(io.open(path, "w"))
    file:write(string.format("%d %d %d\n", width, height, stride))
    file:close()
end

local function capture(label, flags)
    local result = mp.command_native({name = "screenshot-raw", flags = flags, format = "rgba"})
    assert(result and result.data and result.w and result.h and result.stride,
           "screenshot-raw failed for " .. flags)
    assert(result.stride == result.w * 4, "unexpected screenshot stride")
    write(directory .. "/" .. label .. ".rgba", result.data)
    write_meta(directory .. "/" .. label .. ".meta", result.w, result.h, result.stride)
    mp.msg.info(string.format("captured %s %dx%d", label, result.w, result.h))
end

mp.register_script_message("osd-capture", function(mode)
    assert(mode == "video" or mode == "osd" or mode == "subtitles", "unknown capture mode")
    capture("base", "video")
    capture("overlay", mode)
end)
