# Lua API Closure

This file is the authoritative `mp` API closure for the retained built-in Lua
modules: `player/lua/defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`,
`select.lua`, and `osc.lua`. P5.3-P5.5 implement this closure in the Rust host.
Do not implement from a guessed list; extend the sources and rerun the audit
instead.

`tests/lua-api-audit.sh` scans the six files, resolves `require "mp.*"` and
`package.loaded["mp.*"]` aliases, ignores comments and string literals,
regenerates **Call sites**, and fails when:

- a referenced symbol has no row in **Classification**;
- a classification row is duplicated or uses an unknown provider or scope;
- a classification row no longer has a reference or an implied field in the
  sources.

## Method

- Only the six files above are in scope. Other retained C bindings and Lua
  modules are not part of this closure.
- **Provider** names where the symbol comes from: `host` is the native Rust C
  ABI that replaced the bindings in the removed `player/lua.c`; `defaults.lua`,
  `assdraw.lua`, `options.lua`, and `input.lua` are the unchanged built-in
  module files; `data` marks conversion sentinels and internal state.
- **Scope** is the port target: `required` is needed by the OSC closure
  (`osc.lua`, `defaults.lua`, `assdraw.lua`, `options.lua`, and `mp.msg`) by
  P5.4; `built-in` is needed only when `select.lua` and `input.lua` load by
  P5.5; `none` is defined or referenced but never called by a retained script,
  so the unchanged Lua definition is enough; `data` marks required table
  constants and state.
- **Call sites** count every reference, including definitions and assignments
  in `defaults.lua`. A function defined there therefore has a site even when no
  script calls it. The table is generated; do not edit it by hand.
- The interpreter must also open the `io`, `os`, and `package` standard
  libraries used by these modules; that is a host requirement, not an `mp.*`
  symbol.

## Classification

<!-- lua-api:classify-begin -->
| Symbol | Provider | Scope | Semantics |
|---|---|---|---|
| `mp.ARRAY` | data | data | Sentinel table for native arrays. |
| `mp.ARRAY.info` | data | data | Description of the array sentinel. |
| `mp.ARRAY.type` | data | data | Type tag of the array sentinel. |
| `mp.MAP` | data | data | Sentinel table for native maps. |
| `mp.MAP.info` | data | data | Description of the map sentinel. |
| `mp.MAP.type` | data | data | Type tag of the map sentinel. |
| `mp.UNKNOWN_TYPE` | data | data | Sentinel table for native values with no Lua conversion. |
| `mp.UNKNOWN_TYPE.info` | data | data | Description of the unsupported-type sentinel. |
| `mp.UNKNOWN_TYPE.type` | data | data | Type tag of the unsupported-type sentinel. |
| `mp._legacy_overlay` | defaults.lua | none | Cached overlay used only by set_osd_ass. |
| `mp._legacy_overlay.data` | defaults.lua | none | Cached overlay ASS data; set_osd_ass only. |
| `mp._legacy_overlay.res_x` | defaults.lua | none | Cached overlay width; set_osd_ass only. |
| `mp._legacy_overlay.res_y` | defaults.lua | none | Cached overlay height; set_osd_ass only. |
| `mp.abort_async_command` | defaults.lua | none | Abort an async command handle; no retained caller. |
| `mp.add_forced_key_binding` | defaults.lua | none | Register a forced script key binding; no retained caller. |
| `mp.add_hook` | defaults.lua | none | Register a script hook; no retained caller. |
| `mp.add_key_binding` | defaults.lua | required | Register a script key binding and message handler. |
| `mp.add_periodic_timer` | defaults.lua | required | Create a repeating timer. |
| `mp.add_timeout` | defaults.lua | required | Create a one-shot timer. |
| `mp.assdraw.ass_new` | assdraw.lua | required | Create an ASS drawing builder. |
| `mp.cancel_timer` | defaults.lua | none | Stop and clear a timer; no retained caller. |
| `mp.command` | host | required | Parse and run one command string. |
| `mp.command_native` | host | required | Run a command from a native command node. |
| `mp.command_native_async` | defaults.lua | none | Run a native command asynchronously; no retained caller. |
| `mp.commandv` | host | required | Run a command from positional string arguments. |
| `mp.create_osd_overlay` | defaults.lua | required | Create a persistent OSD overlay object. |
| `mp.disable_key_bindings` | defaults.lua | required | Disable a key-binding section. |
| `mp.dispatch_events` | defaults.lua | required | Run the script event loop, timers, and idle callbacks. |
| `mp.enable_key_bindings` | defaults.lua | required | Enable the section created by set_key_bindings. |
| `mp.find_config_file` | host | required | Resolve a config-relative path without loading it automatically. |
| `mp.flush_keybindings` | defaults.lua | required | Rebuild and enable dirty key-binding sections. |
| `mp.format_time` | host | required | Format seconds using a time format string. |
| `mp.get_mouse_pos` | defaults.lua | required | Return the last mouse position from mouse-pos. |
| `mp.get_next_timeout` | defaults.lua | required | Return seconds until the next due timer. |
| `mp.get_opt` | defaults.lua | none | Read a script option from options/script-opts; no retained caller. |
| `mp.get_osd_margins` | defaults.lua | none | Return OSD margins. |
| `mp.get_osd_size` | defaults.lua | required | Return OSD width, height, and aspect. |
| `mp.get_property` | host | required | Read a property in string form; nil when unavailable. |
| `mp.get_property_bool` | host | required | Read a property as a boolean. |
| `mp.get_property_native` | host | required | Read a property and convert its native value to Lua. |
| `mp.get_property_number` | host | required | Read a property as a number. |
| `mp.get_script_name` | defaults.lua | required | Return mp.script_name. |
| `mp.get_time` | host | required | Return monotonic time in seconds. |
| `mp.input.select` | input.lua | built-in | Request a console selection UI; the console module is removed. |
| `mp.input.terminate` | input.lua | built-in | Close the console selection UI; the console module is removed. |
| `mp.input_define_section` | defaults.lua | required | Define an input section from a binding config string. |
| `mp.input_disable_section` | defaults.lua | required | Disable an input section. |
| `mp.input_enable_section` | defaults.lua | required | Enable an input section, optionally with flags. |
| `mp.input_set_section_mouse_area` | host | required | Set the mouse hit area for an input section. |
| `mp.keep_running` | defaults.lua | required | Event-loop liveness flag; cleared by exit(). |
| `mp.log` | host | required | Write a message to the player log at the given level. |
| `mp.msg` | defaults.lua | required | Logging namespace built from mp.log. |
| `mp.msg.debug` | defaults.lua | required | Log at debug level. |
| `mp.msg.error` | defaults.lua | required | Log at error level. |
| `mp.msg.info` | defaults.lua | built-in | Log at info level. |
| `mp.msg.verbose` | defaults.lua | required | Log at verbose level. |
| `mp.msg.warn` | defaults.lua | required | Log at warning level. |
| `mp.observe_property` | defaults.lua | required | Register a callback for property changes. |
| `mp.options.read_options` | options.lua | required | Load script options from config files and script-opts. |
| `mp.osd_message` | defaults.lua | required | Show a transient OSD message via show-text. |
| `mp.raw_abort_async_command` | host | none | Abort a pending async command by id; no retained caller. |
| `mp.raw_command_native_async` | host | none | Start a native command with a completion reply keyed by id; no retained caller. |
| `mp.raw_hook_add` | host | none | Register a raw hook by id, name, and priority; no retained caller. |
| `mp.raw_hook_continue` | host | none | Continue a raw hook by id; no retained caller. |
| `mp.raw_observe_property` | host | required | Register a property observation by numeric id and type. |
| `mp.raw_unobserve_property` | host | required | Remove a property observation by id. |
| `mp.raw_wait_event` | host | required | Wait for the next script event, bounded by a timeout. |
| `mp.register_event` | defaults.lua | required | Register an event handler and enable the event. |
| `mp.register_idle` | defaults.lua | required | Register a callback run before the event loop sleeps. |
| `mp.register_script_message` | defaults.lua | required | Register a handler for a script message. |
| `mp.remove_key_binding` | defaults.lua | none | Remove a script key binding and handler; no retained caller. |
| `mp.request_event` | host | required | Enable or disable delivery of a named event. |
| `mp.script_name` | host | required | Name of the running script; set by the host at state creation. |
| `mp.set_key_bindings` | defaults.lua | required | Replace a section's bindings from a list. |
| `mp.set_mouse_area` | defaults.lua | required | Set the mouse area for a key-binding section. |
| `mp.set_osd_ass` | defaults.lua | none | Legacy set_osd_ass overlay update; no retained caller. |
| `mp.set_property` | host | required | Set a property from a string value. |
| `mp.set_property_native` | host | required | Set a property from a Lua value converted to a native node. |
| `mp.unobserve_property` | defaults.lua | required | Remove property callbacks by function. |
| `mp.unregister_event` | defaults.lua | none | Remove an event handler; no retained caller. |
| `mp.unregister_idle` | defaults.lua | none | Remove an idle callback; no retained caller. |
| `mp.unregister_script_message` | defaults.lua | built-in | Remove a script message handler. |
| `mp.use_suspend` | defaults.lua | required | Deprecated suspend flag, ignored with a warning. |
| `mp.utils.file_info` | host | built-in | Stat a path and return a file-info table. |
| `mp.utils.format_bytes_humanized` | defaults.lua | none | Format a byte count in binary units; no retained caller. |
| `mp.utils.format_json` | host | built-in | Encode a Lua value as JSON. |
| `mp.utils.format_table` | defaults.lua | built-in | Render a Lua table with cycle detection. |
| `mp.utils.getcwd` | defaults.lua | none | Read working-directory; no retained caller. |
| `mp.utils.getpid` | defaults.lua | none | Read pid; no retained caller. |
| `mp.utils.parse_json` | host | built-in | Decode JSON into a Lua value. |
| `mp.utils.readdir` | host | built-in | List a directory with an optional filter. |
| `mp.utils.split_path` | host | built-in | Split a path into directory and basename. |
| `mp.utils.subprocess` | defaults.lua | none | Run a subprocess command node; no retained caller. |
| `mp.utils.subprocess_detached` | defaults.lua | none | Run a detached subprocess via run; no retained caller. |
| `mp.utils.to_string` | defaults.lua | built-in | Render a Lua value as a string. |
| `mp.wait_event` | defaults.lua | required | Wait for one event and normalize the deprecated file_error field. |
<!-- lua-api:classify-end -->

## Call sites

<!-- lua-api:calls-begin -->
| Symbol | Count | Sites |
|---|---|---|
| `mp.ARRAY.info` | 1 | `defaults.lua:9` |
| `mp.ARRAY.type` | 1 | `defaults.lua:10` |
| `mp.MAP.info` | 1 | `defaults.lua:12` |
| `mp.MAP.type` | 1 | `defaults.lua:13` |
| `mp.UNKNOWN_TYPE.info` | 1 | `defaults.lua:6` |
| `mp.UNKNOWN_TYPE.type` | 1 | `defaults.lua:7` |
| `mp._legacy_overlay` | 3 | `defaults.lua:699`, `defaults.lua:700`, `defaults.lua:709` |
| `mp._legacy_overlay.data` | 2 | `defaults.lua:704`, `defaults.lua:708` |
| `mp._legacy_overlay.res_x` | 2 | `defaults.lua:702`, `defaults.lua:706` |
| `mp._legacy_overlay.res_y` | 2 | `defaults.lua:703`, `defaults.lua:707` |
| `mp.abort_async_command` | 1 | `defaults.lua:654` |
| `mp.add_forced_key_binding` | 1 | `defaults.lua:258` |
| `mp.add_hook` | 1 | `defaults.lua:616` |
| `mp.add_key_binding` | 24 | `defaults.lua:254`, `select.lua:42`, `select.lua:125`, `select.lua:183`, `select.lua:187`, `select.lua:192`, `select.lua:197`, `select.lua:207`, `select.lua:232`, `select.lua:257`, `select.lua:384`, `select.lua:439`, `select.lua:497`, `select.lua:553`, `select.lua:599`, `select.lua:673`, `select.lua:677`, `select.lua:681`, `select.lua:685`, `osc.lua:431`, `osc.lua:434`, `osc.lua:437`, `osc.lua:440`, `osc.lua:443` |
| `mp.add_periodic_timer` | 2 | `defaults.lua:275`, `defaults.lua:280` |
| `mp.add_timeout` | 4 | `defaults.lua:274`, `defaults.lua:633`, `osc.lua:243`, `osc.lua:270` |
| `mp.assdraw.ass_new` | 1 | `osc.lua:170` |
| `mp.cancel_timer` | 1 | `defaults.lua:304` |
| `mp.command` | 2 | `select.lua:580`, `select.lua:747` |
| `mp.command_native` | 6 | `defaults.lua:685`, `defaults.lua:689`, `defaults.lua:807`, `select.lua:270`, `select.lua:440`, `select.lua:659` |
| `mp.command_native_async` | 1 | `defaults.lua:627` |
| `mp.commandv` | 30 | `defaults.lua:32`, `defaults.lua:39`, `defaults.lua:43`, `defaults.lua:580`, `defaults.lua:819`, `input.lua:40`, `input.lua:55`, `input.lua:64`, `input.lua:68`, `input.lua:76`, `input.lua:81`, `select.lua:77`, `select.lua:379`, `select.lua:483`, `select.lua:548`, `select.lua:625`, `select.lua:647`, `osc.lua:322`, `osc.lua:357`, `osc.lua:359`, `osc.lua:361`, `osc.lua:363`, `osc.lua:365`, `osc.lua:385`, `osc.lua:387`, `osc.lua:435`, `osc.lua:438`, `osc.lua:441`, `osc.lua:444`, `osc.lua:453` |
| `mp.create_osd_overlay` | 3 | `defaults.lua:664`, `defaults.lua:700`, `osc.lua:45` |
| `mp.disable_key_bindings` | 1 | `defaults.lua:129` |
| `mp.dispatch_events` | 2 | `defaults.lua:515`, `defaults.lua:531` |
| `mp.enable_key_bindings` | 3 | `defaults.lua:125`, `osc.lua:457`, `osc.lua:458` |
| `mp.find_config_file` | 3 | `options.lua:54`, `options.lua:58`, `select.lua:656` |
| `mp.flush_keybindings` | 2 | `defaults.lua:143`, `defaults.lua:570` |
| `mp.format_time` | 2 | `select.lua:204`, `osc.lua:54` |
| `mp.get_mouse_pos` | 5 | `defaults.lua:46`, `osc.lua:327`, `osc.lua:349`, `osc.lua:373`, `osc.lua:382` |
| `mp.get_next_timeout` | 2 | `defaults.lua:332`, `defaults.lua:551` |
| `mp.get_opt` | 1 | `defaults.lua:19` |
| `mp.get_osd_margins` | 1 | `defaults.lua:718` |
| `mp.get_osd_size` | 2 | `defaults.lua:713`, `osc.lua:58` |
| `mp.get_property` | 13 | `defaults.lua:778`, `select.lua:46`, `select.lua:276`, `select.lua:390`, `select.lua:441`, `select.lua:498`, `select.lua:603`, `select.lua:637`, `select.lua:690`, `select.lua:716`, `select.lua:725`, `osc.lua:72`, `osc.lua:73` |
| `mp.get_property_bool` | 1 | `select.lua:651` |
| `mp.get_property_native` | 37 | `defaults.lua:20`, `defaults.lua:47`, `defaults.lua:714`, `defaults.lua:719`, `options.lua:129`, `select.lua:30`, `select.lua:37`, `select.lua:45`, `select.lua:49`, `select.lua:128`, `select.lua:143`, `select.lua:155`, `select.lua:157`, `select.lua:209`, `select.lua:216`, `select.lua:218`, `select.lua:233`, `select.lua:241`, `select.lua:258`, `select.lua:292`, `select.lua:293`, `select.lua:294`, `select.lua:375`, `select.lua:385`, `select.lua:444`, `select.lua:529`, `select.lua:556`, `select.lua:588`, `select.lua:607`, `select.lua:624`, `select.lua:694`, `select.lua:715`, `select.lua:718`, `osc.lua:66`, `osc.lua:67`, `osc.lua:68`, `osc.lua:76` |
| `mp.get_property_number` | 4 | `defaults.lua:782`, `osc.lua:69`, `osc.lua:70`, `osc.lua:71` |
| `mp.get_script_name` | 3 | `defaults.lua:15`, `options.lua:48`, `input.lua:56` |
| `mp.get_time` | 4 | `defaults.lua:296`, `defaults.lua:312`, `defaults.lua:337`, `defaults.lua:351` |
| `mp.input.select` | 12 | `select.lua:72`, `select.lua:139`, `select.lua:173`, `select.lua:222`, `select.lua:247`, `select.lua:368`, `select.lua:406`, `select.lua:478`, `select.lua:544`, `select.lua:576`, `select.lua:620`, `select.lua:742` |
| `mp.input.terminate` | 1 | `select.lua:750` |
| `mp.input_define_section` | 3 | `defaults.lua:28`, `defaults.lua:122`, `defaults.lua:172` |
| `mp.input_disable_section` | 2 | `defaults.lua:42`, `defaults.lua:130` |
| `mp.input_enable_section` | 3 | `defaults.lua:35`, `defaults.lua:126`, `defaults.lua:173` |
| `mp.input_set_section_mouse_area` | 1 | `defaults.lua:134` |
| `mp.keep_running` | 3 | `defaults.lua:420`, `defaults.lua:423`, `defaults.lua:539` |
| `mp.log` | 8 | `defaults.lua:490`, `defaults.lua:491`, `defaults.lua:492`, `defaults.lua:493`, `defaults.lua:494`, `defaults.lua:495`, `defaults.lua:496`, `defaults.lua:497` |
| `mp.msg` | 2 | `defaults.lua:489`, `defaults.lua:503` |
| `mp.msg.debug` | 3 | `options.lua:50`, `options.lua:56`, `options.lua:66` |
| `mp.msg.error` | 7 | `defaults.lua:535`, `defaults.lua:590`, `options.lua:11`, `options.lua:18`, `options.lua:88`, `options.lua:118`, `select.lua:36` |
| `mp.msg.info` | 2 | `defaults.lua:500`, `select.lua:630` |
| `mp.msg.verbose` | 1 | `options.lua:69` |
| `mp.msg.warn` | 6 | `options.lua:60`, `options.lua:83`, `options.lua:114`, `select.lua:29`, `select.lua:421`, `select.lua:457` |
| `mp.observe_property` | 10 | `defaults.lua:396`, `options.lua:135`, `osc.lua:392`, `osc.lua:393`, `osc.lua:397`, `osc.lua:398`, `osc.lua:399`, `osc.lua:400`, `osc.lua:401`, `osc.lua:405` |
| `mp.options.read_options` | 1 | `osc.lua:13` |
| `mp.osd_message` | 6 | `defaults.lua:574`, `select.lua:31`, `select.lua:38`, `select.lua:491`, `osc.lua:309`, `osc.lua:425` |
| `mp.raw_abort_async_command` | 1 | `defaults.lua:656` |
| `mp.raw_command_native_async` | 1 | `defaults.lua:631` |
| `mp.raw_hook_add` | 1 | `defaults.lua:621` |
| `mp.raw_hook_continue` | 1 | `defaults.lua:592` |
| `mp.raw_observe_property` | 1 | `defaults.lua:400` |
| `mp.raw_unobserve_property` | 1 | `defaults.lua:407` |
| `mp.raw_wait_event` | 1 | `defaults.lua:506` |
| `mp.register_event` | 9 | `defaults.lua:428`, `defaults.lua:465`, `defaults.lua:466`, `defaults.lua:467`, `defaults.lua:601`, `defaults.lua:641`, `osc.lua:410`, `osc.lua:416`, `osc.lua:420` |
| `mp.register_idle` | 2 | `defaults.lua:472`, `defaults.lua:570` |
| `mp.register_script_message` | 8 | `defaults.lua:251`, `defaults.lua:376`, `defaults.lua:487`, `input.lua:34`, `osc.lua:424`, `osc.lua:427`, `osc.lua:428`, `osc.lua:429` |
| `mp.remove_key_binding` | 1 | `defaults.lua:262` |
| `mp.request_event` | 2 | `defaults.lua:435`, `defaults.lua:458` |
| `mp.script_name` | 6 | `defaults.lua:16`, `defaults.lua:73`, `defaults.lua:117`, `defaults.lua:153`, `defaults.lua:156`, `defaults.lua:242` |
| `mp.set_key_bindings` | 3 | `defaults.lua:89`, `osc.lua:447`, `osc.lua:451` |
| `mp.set_mouse_area` | 2 | `defaults.lua:133`, `osc.lua:139` |
| `mp.set_osd_ass` | 1 | `defaults.lua:698` |
| `mp.set_property` | 7 | `select.lua:145`, `select.lua:178`, `select.lua:227`, `select.lua:252`, `select.lua:411`, `select.lua:604`, `select.lua:611` |
| `mp.set_property_native` | 4 | `osc.lua:132`, `osc.lua:133`, `osc.lua:303`, `osc.lua:461` |
| `mp.unobserve_property` | 1 | `defaults.lua:403` |
| `mp.unregister_event` | 1 | `defaults.lua:438` |
| `mp.unregister_idle` | 1 | `defaults.lua:476` |
| `mp.unregister_script_message` | 3 | `defaults.lua:266`, `defaults.lua:380`, `input.lua:47` |
| `mp.use_suspend` | 2 | `defaults.lua:527`, `defaults.lua:533` |
| `mp.utils.file_info` | 1 | `select.lua:522` |
| `mp.utils.format_bytes_humanized` | 1 | `defaults.lua:785` |
| `mp.utils.format_json` | 5 | `input.lua:41`, `input.lua:56`, `input.lua:68`, `input.lua:77`, `input.lua:81` |
| `mp.utils.format_table` | 2 | `defaults.lua:725`, `defaults.lua:771` |
| `mp.utils.getcwd` | 1 | `defaults.lua:777` |
| `mp.utils.getpid` | 1 | `defaults.lua:781` |
| `mp.utils.parse_json` | 2 | `input.lua:37`, `select.lua:453` |
| `mp.utils.readdir` | 1 | `select.lua:507` |
| `mp.utils.split_path` | 2 | `select.lua:54`, `select.lua:434` |
| `mp.utils.subprocess` | 1 | `defaults.lua:795` |
| `mp.utils.subprocess_detached` | 1 | `defaults.lua:818` |
| `mp.utils.to_string` | 4 | `defaults.lua:753`, `defaults.lua:755`, `defaults.lua:761`, `select.lua:595` |
| `mp.wait_event` | 2 | `defaults.lua:505`, `defaults.lua:561` |
<!-- lua-api:calls-end -->
