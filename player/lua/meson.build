lua_files = ['defaults.lua', 'assdraw.lua', 'options.lua', 'input.lua',
             'select.lua', 'osc.lua']
foreach file: lua_files
    lua_file = custom_target(file,
        input: file,
        output: file + '.inc',
        command: [file2string, '@INPUT@', '@OUTPUT@', '@SOURCE_ROOT@'],
    )
    sources += lua_file
endforeach
