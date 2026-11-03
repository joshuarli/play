project('mpv',
        'c',
        license: ['GPL2+', 'LGPL2.1+'],
        version: files('./MPV_VERSION'),
        meson_version: '>=1.3.0',
        default_options: [
            'backend_max_links=16',
            'buildtype=debugoptimized',
            'b_lundef=false',
            'c_std=c11',
            'cpp_std=c++20',
            'cpp_eh=default',
            'warning_level=2',
        ]
)

build_root = meson.project_build_root()
source_root = meson.project_source_root()
python = find_program('python3')

# ffmpeg
libavcodec = dependency('libavcodec', version: '>= 60.31.102')
libavfilter = dependency('libavfilter', version: '>= 9.12.100')
libavformat = dependency('libavformat', version: '>= 60.16.100')
libavutil = dependency('libavutil', version: '>= 58.29.100')
libswresample = dependency('libswresample', version: '>= 4.12.100')
libswscale = dependency('libswscale', version: '>= 7.5.100')

libplacebo = dependency('libplacebo', version: '>=6.338.2',
                default_options: ['default_library=static', 'demos=false'])

libass = dependency('libass', version: '>= 0.12.2')

# the dependency order of libass -> ffmpeg is necessary due to
# static linking symbol resolution between fontconfig and libass
dependencies = [libass,
                libavcodec,
                libavfilter,
                libavformat,
                libavutil,
                libplacebo,
                libswresample,
                libswscale]

player_audio_lib = get_option('player-audio-lib')
if player_audio_lib != ''
    dependencies += declare_dependency(link_args: [player_audio_lib])
endif

player_ffi_lib = get_option('player-ffi-lib')
if player_ffi_lib != ''
    dependencies += declare_dependency(link_args: [player_ffi_lib])
endif

# Keeps track of all enabled/disabled features
features = {
    'debug': get_option('debug'),
    'ffmpeg': true,
    'gpl': get_option('gpl'),
    'libass': true,
    'libplacebo': true,
}


# generic sources
sources = files(
    ## Audio
    'audio/aframe.c',
    'audio/chmap.c',
    'audio/chmap_avchannel.c',
    'audio/chmap_sel.c',
    'audio/decode/ad_lavc.c',
    'audio/decode/ad_symphonia.c',
    'audio/decode/ad_spdif.c',
    'audio/filter/af_drop.c',
    'audio/filter/af_format.c',
    'audio/filter/af_scaletempo.c',
    'audio/filter/af_scaletempo2.c',
    'audio/filter/af_scaletempo2_internals.c',
    'audio/fmt-conversion.c',
    'audio/format.c',
    'audio/out/ao.c',
    'audio/out/ao_null.c',
    'audio/out/buffer.c',

    ## Core
    'common/av_common.c',
    'common/av_log.c',
    'common/codecs.c',
    'common/common.c',
    'common/msg.c',
    'common/playlist.c',
    'common/stats.c',
    'common/tags.c',
    'common/version.c',

    ## Demuxers
    'demux/codec_tags.c',
    'demux/cue.c',
    'demux/cache.c',
    'demux/demux.c',
    'demux/demux_lavf.c',
    'demux/demux_mkv.c',
    'demux/demux_mkv_timeline.c',
    'demux/demux_playlist.c',
    'demux/demux_timeline.c',
    'demux/ebml.c',
    'demux/packet.c',
    'demux/packet_pool.c',
    'demux/timeline.c',

    ## Filters
    'filters/f_async_queue.c',
    'filters/f_autoconvert.c',
    'filters/f_auto_filters.c',
    'filters/f_decoder_wrapper.c',
    'filters/f_demux_in.c',
    'filters/f_hwtransfer.c',
    'filters/f_output_chain.c',
    'filters/f_swresample.c',
    'filters/f_swscale.c',
    'filters/f_utils.c',
    'filters/filter.c',
    'filters/frame.c',
    'filters/user_filters.c',

    ## Input
    'input/cmd.c',
    'input/event.c',
    'input/input.c',
    'input/ipc.c',
    'input/keycodes.c',

    ## Misc
    'misc/bstr.c',
    'misc/charset_conv.c',
    'misc/codepoint_width.c',
    'misc/dispatch.c',
    'misc/io_utils.c',
    'misc/json.c',
    'misc/language.c',
    'misc/natural_sort.c',
    'misc/node.c',
    'misc/path_utils.c',
    'misc/random.c',
    'misc/rendezvous.c',
    'misc/thread_pool.c',
    'misc/thread_tools.c',

    ## Options
    'options/m_config_core.c',
    'options/m_config_frontend.c',
    'options/m_option.c',
    'options/m_property.c',
    'options/options.c',
    'options/parse_commandline.c',
    'options/parse_configfile.c',
    'options/path.c',

    ## Player
    'player/album_art.c',
    'player/audio.c',
    'player/client.c',
    'player/command.c',
    'player/configfiles.c',
    'player/external_files.c',
    'player/loadfile.c',
    'player/main.c',
    'player/misc.c',
    'player/osd.c',
    'player/playloop.c',
    'player/screenshot.c',
    'player/scripting.c',
    'player/sub.c',
    'player/tui.c',
    'player/video.c',

    ## clipboard
    'player/clipboard/clipboard.c',
    'player/clipboard/clipboard-vo.c',

    ## Streams
    'stream/stream.c',
    'stream/stream_cb.c',
    'stream/stream_file.c',
    'stream/stream_memory.c',

    ## Subtitles
    'sub/ass_mp.c',
    'sub/dec_sub.c',
    'sub/draw_bmp.c',
    'sub/filter_sdh.c',
    'sub/img_convert.c',
    'sub/lavc_conv.c',
    'sub/osd.c',
    'sub/osd_libass.c',
    'sub/sd_ass.c',
    'sub/sd_lavc.c',

    ## Video
    'video/csputils.c',
    'video/decode/vd_lavc.c',
    'video/filter/refqueue.c',
    'video/filter/vf_format.c',
    'video/filter/vf_sub.c',
    'video/fmt-conversion.c',
    'video/hwdec.c',
    'video/image_loader.c',
    'video/image_writer.c',
    'video/img_format.c',
    'video/mp_image.c',
    'video/mp_image_pool.c',
    'video/out/aspect.c',
    'video/out/bitmap_packer.c',
    'video/out/dither.c',
    'video/out/dr_helper.c',
    'video/out/filter_kernels.c',
    'video/out/gpu/context.c',
    'video/out/gpu/error_diffusion.c',
    'video/out/gpu/hwdec.c',
    'video/out/gpu/lcms.c',
    'video/out/gpu/osd.c',
    'video/out/gpu/ra.c',
    'video/out/gpu/shader_cache.c',
    'video/out/gpu/user_shaders.c',
    'video/out/gpu/utils.c',
    'video/out/gpu/video.c',
    'video/out/gpu/video_shaders.c',
    'video/out/vo.c',
    'video/out/vo_null.c',
    'video/out/win_state.c',
    'video/repack.c',
    'video/sws_utils.c',

    ## libplacebo
    'video/out/placebo/ra_pl.c',
    'video/out/placebo/utils.c',
    'video/out/vo_gpu_next.c',
    'video/out/gpu_next/context.c',

    ## osdep
    'osdep/io.c',
    'osdep/subprocess.c',
    'osdep/timer.c',

    ## tree_allocator
    'ta/ta.c',
    'ta/ta_talloc.c',
    'ta/ta_utils.c',
)


# compiler stuff
cc = meson.get_compiler('c')

flags = ['-D_FILE_OFFSET_BITS=64']
flags_c = []
link_flags = []

test_flags = ['-Wdisabled-optimization',
              '-Wempty-body',
              '-Wformat',
              '-Wimplicit-fallthrough',
              '-Woverflow',
              '-Wparentheses',
              '-Wpointer-arith',
              '-Wshadow',
              '-Wundef',
              '-Wvla',
              '-Wno-cast-function-type',
              '-Wno-format-zero-length',
              '-Wno-missing-field-initializers',
              '-Wno-sign-compare',
              '-Wno-switch',
              '-Wno-unused-parameter',
              '-fno-math-errno',
              '-fno-signed-zeros',
              '-fno-trapping-math']

test_flags_c = ['-Wmissing-prototypes',
                '-Wstrict-prototypes',
                '-Werror=implicit-function-declaration',
                '-Wno-pointer-sign']

flags += cc.get_supported_arguments(test_flags)
flags_c += cc.get_supported_arguments(test_flags_c)

if cc.has_multi_arguments('-Wformat', '-Werror=format-security')
    flags += '-Werror=format-security'
endif

if cc.get_id() == 'gcc'
    # https://gcc.gnu.org/bugzilla/show_bug.cgi?id=66425
    flags +='-Wno-unused-result'
endif

darwin = host_machine.system() == 'darwin'

features += {'darwin': darwin}

flags += ['-D_GNU_SOURCE']

link_flags += cc.get_supported_link_arguments(['-Wl,-z,noexecstack'])

features += {'build-date': get_option('build-date')}
if not features['build-date']
    flags += '-DNO_BUILD_TIMESTAMPS'
endif

features += {'disable-packet-pool': get_option('disable-packet-pool')}

libdl = dependency('dl', required: false)
features += {'libdl': libdl.found()}
if features['libdl']
    dependencies += libdl
endif

# C11 atomics are mandatory but linking to the library is not always required.
dependencies += cc.find_library('atomic', required: false)

pthreads = dependency('threads')
sources += files('osdep/threads-posix.c')
features += {'pthread-condattr-setclock':
                cc.has_header_symbol('pthread.h',
                                     'pthread_condattr_setclock',
                                     args: '-D_GNU_SOURCE',
                                     dependencies: pthreads)}
dependencies += pthreads

pthread_debug = get_option('pthread-debug')
features += {'pthread-debug': pthread_debug.allowed()}

if get_option('fuzzers')
    if get_option('cplayer') or not get_option('libmpv')
        error('fuzzers require !cplayer and libmpv')
    endif
    # Adding flags manually until https://github.com/mesonbuild/meson/pull/9825
    flags += ['-fsanitize=address,undefined,fuzzer', '-fno-omit-frame-pointer']
    flags += ['-DFUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION']
    link_flags += ['-fsanitize=address,undefined,fuzzer', '-fno-omit-frame-pointer']
endif

add_project_arguments(flags, language: ['c', 'cpp', 'objc'])
add_project_arguments(flags_c, language: ['c', 'objc'])
add_project_link_arguments(link_flags, language: ['c', 'cpp', 'objc'])


# osdep
if darwin
    path_source = files('osdep/path-darwin.c')
    timer_source = files('osdep/timer-darwin.c')
endif

cocoa = dependency('appleframeworks', modules: ['Cocoa', 'IOKit', 'QuartzCore'],
                   required: get_option('cocoa'))
features += {'cocoa': cocoa.found()}
if features['cocoa']
    dependencies += cocoa
    path_source += files('osdep/path-mac.m')
    sources += files('osdep/language-mac.c',
                     'osdep/utils-mac.c',
                     'osdep/mac/app_bridge.m',
                     'player/clipboard/clipboard-mac.m')
    main_fn_source = files('osdep/main-fn-mac.c')
endif

if not get_option('fuzzers') and cc.has_function('fork', prefix : '#include <unistd.h>')
    sources += files('osdep/subprocess-posix.c')
else
    sources += files('osdep/subprocess-dummy.c')
endif
sources += files('input/ipc-unix.c',
                 'osdep/poll_wrapper.c',
                 'osdep/terminal-unix.c',
                 'sub/filter_regex.c')

if not features['cocoa']
    sources += files('osdep/language-posix.c')
    main_fn_source = files('osdep/main-fn-unix.c')
endif

if not darwin
    path_source = files('osdep/path-unix.c')
    timer_source = files('osdep/timer-linux.c')
endif

if host_machine.system() == 'linux'
    sources += files('osdep/als-linux.c')
else
    sources += files('osdep/als-voctrl.c')
endif

features += {'ppoll': cc.has_function('ppoll', args: '-D_GNU_SOURCE',
                                      prefix: '#include <poll.h>')}
features += {'memrchr': cc.has_function('memrchr', args: '-D_GNU_SOURCE',
                                        prefix: '#include <string.h>')}

optical_devices = {
    'darwin': '/dev/disk1',
    'freebsd': '/dev/cd0',
    'openbsd': '/dev/rcd0c',
    'linux': '/dev/sr0',
}
if host_machine.system() in optical_devices
    optical_device = optical_devices[host_machine.system()]
else
    optical_device = '/dev/cdrom'
endif

features += {'android': host_machine.system() == 'android'}
if features['android']
    dependencies += cc.find_library('android')
    sources += files('misc/jni.c',
                     'osdep/android/strnlen.c',
                     'video/out/android_common.c',
                     'video/out/vo_mediacodec_embed.c')
endif

features += {'glob-posix': cc.has_function('glob', prefix: '#include <glob.h>')}

features += {'glob': features['glob-posix']}

features += {'vt.h': cc.has_header_symbol('sys/vt.h', 'VT_GETMODE')}

features += {'consio.h': not features['vt.h'] and cc.has_header_symbol('sys/consio.h', 'VT_GETMODE')}

features += {'wsdisplay-usl-io.h': cc.has_header_symbol('dev/wscons/wsdisplay_usl_io.h', 'VT_GETMODE', prefix: '#include <sys/types.h>')}

# macOS's pthread_setname_np is a special snowflake and differs from literally every other platform.
features += {'mac-thread-name': darwin}

features += {'glibc-thread-name': false}
if not features['mac-thread-name']
    features += {'glibc-thread-name': cc.has_function('pthread_setname_np', args: '-D_GNU_SOURCE',
                                                      dependencies: pthreads, prefix: '#include <pthread.h>')}
endif

features += {'bsd-thread-name': false}
if not features['mac-thread-name'] and not features['glibc-thread-name']
    features += {'bsd-thread-name': cc.has_function('pthread_set_name_np', dependencies: pthreads,
                                                    prefix: '#include <pthread.h>\n#include <pthread_np.h>')}
endif

features += {'bsd-fstatfs': cc.has_function('fstatfs', prefix: '#include <sys/mount.h>\n#include <sys/param.h>')}

features += {'linux-fstatfs': cc.has_function('fstatfs', prefix: '#include <sys/vfs.h>')}

features += {'vector': cc.has_function_attribute('vector_size', required: get_option('vector'))}

sources += path_source + timer_source


# various file generations
tools_directory = join_paths(source_root, 'TOOLS')
docutils_wrapper = find_program(join_paths(tools_directory, 'docutils-wrapper.py'))
file2string = find_program(join_paths(tools_directory, 'file2string.py'))
matroska = find_program(join_paths(tools_directory, 'matroska.py'))
mpv_desktop = find_program(join_paths(tools_directory, 'gen-mpv-desktop.py'))

ebml_defs = custom_target('ebml_defs.inc',
    output: 'ebml_defs.inc',
    command: [matroska, '--generate-definitions', '@OUTPUT@'],
)

ebml_types = custom_target('ebml_types.h',
    output: 'ebml_types.h',
    command: [matroska, '--generate-header', '@OUTPUT@'],
)

sources += [ebml_defs, ebml_types]

subdir('common')
subdir('etc')
subdir('sub')

if darwin
    subdir(join_paths('TOOLS', 'osxbundle'))
endif

cdda_opt = get_option('cdda').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)
cdio = dependency('libcdio', version: '>= 0.90', required: cdda_opt)
cdda = dependency('libcdio_paranoia', required: cdda_opt)
features += {'cdda': cdda.found() and cdio.found()}

dvbin_opt = get_option('dvbin').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)

dvdnav_opt = get_option('dvdnav').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)

iconv = dependency('iconv', required: get_option('iconv'))
features += {'iconv': iconv.found()}
if features['iconv']
    dependencies += iconv
endif

lcms2 = dependency('lcms2', version: '>= 2.6', required: get_option('lcms2'))
features += {'lcms2': lcms2.found()}
if features['lcms2']
    dependencies += lcms2
endif

libarchive = dependency('libarchive', version: '>= 3.4.0', required: get_option('libarchive'))
features += {'libarchive': libarchive.found()}

libavdevice = dependency('libavdevice', version: '>= 60.3.100', required: get_option('libavdevice'))
features += {'libavdevice': libavdevice.found()}

libbluray = dependency('libbluray', version: '>= 0.5.0', required: get_option('libbluray'))
features += {'libbluray': libbluray.found()}

libm = cc.find_library('m', required: false)
if libm.found()
    dependencies += libm
endif

librt = cc.find_library('rt', required: false)
if librt.found()
    dependencies += librt
endif

features += {'lua': true}
lua_version = ['lua52']
sources += files('player/lua_host.c')

rubberband = dependency('rubberband', version: '>= 1.8.0', required: get_option('rubberband'))
features += {'rubberband': rubberband.found()}
if features['rubberband']
    features += {'rubberband-3': rubberband.version().version_compare('>= 3.0.0')}
    dependencies += rubberband
    sources += files('audio/filter/af_rubberband.c')
endif

sdl2 = dependency('sdl2', required: false)

sdl2_gamepad = get_option('sdl2-gamepad').require(
    sdl2.found(),
    error_message: 'sdl2 was not found!',
)
features += {'sdl2-gamepad': sdl2_gamepad.allowed()}
if features['sdl2-gamepad']
    sources += files('input/sdl_gamepad.c')
    dependencies += sdl2
endif

uchardet_opt = get_option('uchardet').require(
    features['iconv'],
    error_message: 'iconv was not found!',
)
uchardet = dependency('uchardet', required: uchardet_opt)
features += {'uchardet': uchardet.found()}
if features['uchardet']
    dependencies += uchardet
endif

vapoursynth = dependency('vapoursynth', version: '>= 56', required: get_option('vapoursynth'))
vapoursynth_script = dependency('vapoursynth-script', version: '>= 56',
                                required: get_option('vapoursynth'))
features += {'vapoursynth': vapoursynth.found() and vapoursynth_script.found()}

zimg = dependency('zimg', version: '>= 2.9', required: get_option('zimg'))
features += {'zimg': zimg.found()}
if features['zimg']
    dependencies += zimg
    sources += files('video/filter/vf_fingerprint.c',
                     'video/zimg.c')
    features += {'zimg-st428': zimg.version().version_compare('>= 3.0.5')}
endif

zlib = dependency('zlib', required: get_option('zlib'))
features += {'zlib': zlib.found()}
if features['zlib']
    dependencies += zlib
endif


# audio output dependencies
alsa = dependency('alsa', version: '>= 1.0.18', required: get_option('alsa'))
features += {'alsa': alsa.found()}
if features['alsa']
    dependencies += alsa
    sources += files('audio/out/ao_alsa.c')
endif

audiounit_opt = get_option('audiounit').require(darwin)
audiounit = {
    'deps': dependency('appleframeworks', modules: ['Foundation', 'AudioToolbox'],
                       required: audiounit_opt),
    'symbol': cc.has_header_symbol('AudioToolbox/AudioToolbox.h', 'kAudioUnitSubType_RemoteIO',
                                   required: audiounit_opt),
}
features += {'audiounit': audiounit['deps'].found() and audiounit['symbol']}
if features['audiounit']
    dependencies += audiounit['deps']
    sources += files('audio/out/ao_audiounit.m')
endif

avfoundation = dependency('appleframeworks', modules: ['CoreMedia', 'AVFoundation'],
                          required: get_option('avfoundation'))
features += {'avfoundation': avfoundation.found()}
if features['avfoundation']
    dependencies += avfoundation
    sources += files('audio/out/ao_avfoundation.m')
endif

coreaudio = dependency('appleframeworks', modules: ['CoreFoundation', 'CoreAudio',
                       'AudioUnit', 'AudioToolbox'], required: get_option('coreaudio'))
features += {'coreaudio': coreaudio.found()}
if features['coreaudio']
    dependencies += coreaudio
    sources += files('audio/out/ao_coreaudio.c',
                     'audio/out/ao_coreaudio_exclusive.c',
                     'audio/out/ao_coreaudio_properties.c')
endif

if features['audiounit'] or features['coreaudio'] or features['avfoundation']
    sources += files('audio/out/ao_coreaudio_chmap.c',
                     'audio/out/ao_coreaudio_utils.c')
endif

if features['avfoundation']
    sources += files('audio/out/ao_coreaudio_properties.c')
endif

jack_opt = get_option('jack').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)
jack = dependency('jack', required: jack_opt)
features += {'jack': jack.found()}
if features['jack']
    dependencies += jack
    sources += files('audio/out/ao_jack.c')
endif

openal = dependency('openal', version: '>= 1.13', required: get_option('openal'))
features += {'openal': openal.found()}
if features['openal']
    dependencies += openal
    sources += files('audio/out/ao_openal.c')
endif

features += {'audiotrack':
    get_option('audiotrack').require(features['android']).allowed()}
if features['audiotrack']
    sources += files('audio/out/ao_audiotrack.c')
endif

aaudio_opt = get_option('aaudio').require(features['android'])
features += {'aaudio': cc.has_header_symbol('aaudio/AAudio.h',
                                            'AAudioStream', required: aaudio_opt)}
if features['aaudio']
    sources += files('audio/out/ao_aaudio.c')
endif

opensles = cc.find_library('OpenSLES', required: get_option('opensles'))
features += {'opensles': opensles.found()}
if features['opensles']
    dependencies += opensles
    sources += files('audio/out/ao_opensles.c')
endif

oss_opt = get_option('oss-audio').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)
features += {'oss-audio': cc.has_header_symbol('sys/soundcard.h', 'SNDCTL_DSP_HALT',
                                               required: oss_opt)}
if features['oss-audio']
    sources += files('audio/out/ao_oss.c')
endif

pipewire = dependency('libpipewire-0.3', version: '>= 0.3.57', required: get_option('pipewire'))
features += {'pipewire': pipewire.found()}
if features['pipewire']
    dependencies += pipewire
    sources += files('audio/out/ao_pipewire.c')
endif

pulse = dependency('libpulse', version: '>= 1.0', required: get_option('pulse'))
features += {'pulse': pulse.found()}
if features['pulse']
    dependencies += pulse
    sources += files('audio/out/ao_pulse.c')
endif

sdl2_audio = get_option('sdl2-audio').require(
    sdl2.found(),
    error_message: 'sdl2 was not found!',
)
features += {'sdl2-audio': sdl2_audio.allowed()}
if features['sdl2-audio']
    sources += files('audio/out/ao_sdl.c')
    dependencies += sdl2
endif

sndio = dependency('sndio', required: get_option('sndio'))
features += {'sndio': sndio.found()}
features += {'sndio-1-9': sndio.version().version_compare('>= 1.9.0')}
if features['sndio']
    dependencies += sndio
    sources += files('audio/out/ao_sndio.c')
endif


# video output dependencies
caca_opt = get_option('caca').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)
caca = dependency('caca', version: '>= 0.99.beta18', required: caca_opt)
features += {'caca': caca.found()}
if features['caca']
    dependencies += caca
    sources += files('video/out/vo_caca.c')
endif

drm = dependency('libdrm', version: '>= 2.4.105', required: get_option('drm'))
libdisplay_info = dependency('libdisplay-info', version: '>= 0.1.1', required: get_option('drm'))
features += {'drm': drm.found() and libdisplay_info.found() and
             (features['vt.h'] or features['consio.h'] or features['wsdisplay-usl-io.h'])}
if features['drm']
    dependencies += [drm, libdisplay_info]
    sources += files('video/drmprime.c',
                     'video/out/drm_atomic.c',
                     'video/out/drm_common.c',
                     'video/out/drm_prime.c',
                     'video/out/hwdec/hwdec_drmprime.c',
                     'video/out/hwdec/hwdec_drmprime_overlay.c',
                     'video/out/vo_drm.c')
endif

gbm_opt = get_option('gbm').require(
    features['drm'],
    error_message: 'drm was not found!'
)
gbm = dependency('gbm', version: '>= 17.1.0', required: gbm_opt)
features += {'gbm': gbm.found()}
if features['gbm']
    dependencies += gbm
endif

jpeg = dependency('libjpeg', required: get_option('jpeg'))
features += {'jpeg': jpeg.found()}
if features['jpeg']
    dependencies += jpeg
endif

sdl2_video = get_option('sdl2-video').require(
    sdl2.found(),
    error_message: 'sdl2 was not found!',
)
features += {'sdl2-video': sdl2_video.allowed()}
if features['sdl2-video']
    sources += files('video/out/vo_sdl.c')
    dependencies += sdl2
endif

sixel = dependency('libsixel', version: '>= 1.5', required: get_option('sixel'))
features += {'sixel': sixel.found()}
if features['sixel']
    dependencies += sixel
    sources += files('video/out/vo_sixel.c')
endif

features += {'posix-shm': cc.has_function('shm_open', prefix: '#include <sys/mman.h>')}

wayland = {
    'deps': [dependency('wayland-client', version: '>= 1.21.0', required: get_option('wayland')),
             dependency('wayland-cursor', version: '>= 1.21.0', required: get_option('wayland')),
             dependency('wayland-protocols', version: '>= 1.31', required: get_option('wayland')),
             dependency('xkbcommon', version: '>= 0.3.0', required: get_option('wayland'))],
    'header': cc.has_header('linux/input-event-codes.h', required: get_option('wayland'),
                            # Pass CFLAGS from a related package as a hint for non-Linux
                            dependencies: dependency('wayland-client', required: get_option('wayland'))),
    'scanner': find_program('wayland-scanner', required: get_option('wayland')),
}
wayland_deps = true
foreach dep: wayland['deps']
    if not dep.found()
        wayland_deps = false
        break
    endif
endforeach
features += {'wayland': wayland_deps and wayland['header'] and wayland['scanner'].found()}

if features['wayland']
    subdir(join_paths('video', 'out'))
endif

if features['wayland'] and features['wayland-protocols-1-39']
    sources += files('player/clipboard/clipboard-wayland.c')
endif

features += {'memfd-create': false}
if features['wayland']
    features += {'memfd-create': cc.has_function('memfd_create',
                                                 prefix: '#define _GNU_SOURCE\n#include <sys/mman.h>')}
endif

dmabuf_wayland = get_option('dmabuf-wayland').require(
    features['memfd-create'] and features['wayland'],
    error_message: 'memfd-create or wayland was not found!',
)
features += {'dmabuf-wayland': dmabuf_wayland.allowed()}
if features['dmabuf-wayland']
    sources += files('video/out/vo_dmabuf_wayland.c')
    sources += files('video/out/hwdec/dmabuf_interop_wl.c')
    sources += files('video/out/wldmabuf/context_wldmabuf.c')
    sources += files('video/out/wldmabuf/ra_wldmabuf.c')
endif

x11_opt = get_option('x11').require(
    get_option('gpl'),
    error_message: 'the build is not GPL!',
)
x11 = {
    'deps': [dependency('x11', version: '>= 1.0.0', required: x11_opt),
             dependency('xscrnsaver', version: '>= 1.0.0', required: x11_opt),
             dependency('xext', version: '>= 1.0.0', required: x11_opt),
             dependency('xpresent', version: '>= 1.0.0', required: x11_opt),
             dependency('xrandr', version: '>= 1.4.0', required: x11_opt)],
}
x11_deps = true
foreach dep: x11['deps']
    if not dep.found()
        x11_deps = false
        break
    endif
endforeach
features += {'x11': x11_deps}

if features['x11']
    dependencies += x11['deps']
    sources += files('video/out/vo_x11.c',
                     'video/out/x11_common.c')
endif

xv_opt = get_option('xv').require(
    features['x11'],
    error_message: 'x11 could not be found!',
)
xv = dependency('xv', required: xv_opt)
features += {'xv': xv.found()}
if features['xv']
    dependencies += xv
    sources += files('video/out/vo_xv.c')
endif

if features['wayland'] or features['x11'] or features['drm']
    sources += ('video/out/present_sync.c')
endif


# OpenGL feature checking
gl_allowed = get_option('gl').allowed()
features += {'gl': false}

GL = dependency('', required: false)
if darwin
    GL = dependency('appleframeworks', modules: 'OpenGL', required: get_option('gl-cocoa'))
elif features['x11']
    GL = dependency('GL', required: get_option('gl-x11'))
endif

gl_cocoa = get_option('gl-cocoa').require(
    features['cocoa'] and GL.found() and gl_allowed,
    error_message: 'cocoa and GL were not found!',
)
features += {'gl-cocoa': gl_cocoa.allowed()}
if features['gl-cocoa']
    dependencies += GL
    features += {'gl': true}
endif

gl_x11 = get_option('gl-x11').require(
    GL.found() and gl_allowed and features['x11'],
    error_message: 'GL and x11 were not found!',
)
features += {'gl-x11': gl_x11.allowed()}
if features['gl-x11']
    dependencies += GL
    features += {'gl': true}
    sources += files('video/out/opengl/context_glx.c')
endif

egl = dependency('egl', version: '>= 1.4.0', required: get_option('egl'))
features += {'egl': egl.found() and gl_allowed}
if features['egl']
    dependencies += egl
endif

egl_android_opt = get_option('egl-android').require(
    features['android'] and gl_allowed,
    error_message: 'the OS is not android!',
)
egl_android = cc.find_library('EGL', required: egl_android_opt)
features += {'egl-android': egl_android.found()}
if features['egl-android']
    dependencies += egl_android
    features += {'gl': true}
    sources += files('video/out/opengl/context_android.c')
endif

egl_drm = get_option('egl-drm').require(
    features['drm'] and features['egl'] and features['gbm'] and gl_allowed,
    error_message: 'either drm, egl, or gbm could not be found!',
)
features += {'egl-drm': egl_drm.allowed()}
if features['egl-drm']
    features += {'gl': true}
    sources += files('video/out/opengl/context_drm_egl.c')
endif

egl_wayland_opt = get_option('egl-wayland').require(
    features['egl'] and features['wayland'] and gl_allowed,
    error_message: 'either egl or wayland could not be found!',
)
egl_wayland = dependency('wayland-egl', version: '>= 9.0.0', required: egl_wayland_opt)
features += {'egl-wayland': egl_wayland.found()}
if features['egl-wayland']
    dependencies += egl_wayland
    features += {'gl': true}
    sources += files('video/out/opengl/context_wayland.c')
endif

egl_x11 = get_option('egl-x11').require(
    features['egl'] and gl_allowed and features['x11'],
    error_message: 'either egl or x11 could not be found!',
)
features += {'egl-x11': egl_x11.allowed()}
if features['egl-x11']
    features += {'gl': true}
    sources += files('video/out/opengl/context_x11egl.c')
endif

plain_gl = get_option('plain-gl').require(
    gl_allowed,
    error_message: 'gl was not enabled!',
)
if plain_gl.allowed()
    features += {'gl': true}
endif

if features['egl'] or features['egl-android']
    sources += files('video/out/opengl/egl_helpers.c')
endif

if features['gl'] and features['egl']
    sources += files('video/filter/vf_gpu_egl.c')
endif

if features['gl']
    sources += files('video/out/opengl/common.c',
                     'video/out/opengl/context.c',
                     'video/out/opengl/formats.c',
                     'video/out/opengl/ra_gl.c',
                     'video/out/opengl/utils.c')
elif not features['gl'] and get_option('gl').enabled()
    error('gl enabled but no OpenGL video output could be found!')
endif


# vulkan
vulkan_opt = get_option('vulkan').require(
    libplacebo.get_variable('pl_has_vulkan', default_value: '0') == '1',
    error_message: 'libplacebo compiled without vulkan support!',
)
# libplacebo/ffmpeg public headers depend on Vulkan 1.3
vulkan = dependency('vulkan', version: '>= 1.3.238', required: vulkan_opt)
features += {'vulkan': vulkan.found() and (vulkan.type_name() == 'internal' or
                       cc.has_header_symbol('vulkan/vulkan_core.h',
                                            'VK_VERSION_1_3',
                                            dependencies: vulkan))}
if features['vulkan']
    dependencies += vulkan
    sources += files('video/out/hwdec/hwdec_vulkan.c',
                     'video/out/vulkan/context.c',
                     'video/out/vulkan/utils.c',
                     'video/filter/vf_gpu_vulkan.c')
endif

if features['vulkan'] and features['android']
    sources += files('video/out/vulkan/context_android.c')
endif

if features['vulkan'] and features['wayland']
    sources += files('video/out/vulkan/context_wayland.c')
endif

if features['vulkan'] and features['x11']
     sources += files('video/out/vulkan/context_xlib.c')
endif

features += {'vk-khr-display': false}

if features['vulkan'] or (features['gl'] and features['egl'])
    sources += files('video/filter/vf_gpu.c')
endif

# hwaccel
ffnvcodec = dependency('ffnvcodec', version: '>= 11.1.5.1', required: false)
features += {'ffnvcodec': ffnvcodec.found()}
if features['ffnvcodec']
    dependencies += ffnvcodec
    sources += files('video/cuda.c')
endif

cuda_hwaccel = get_option('cuda-hwaccel').require(
    features['ffnvcodec'],
    error_message: 'ffnvcodec was not found!',
)
features += {'cuda-hwaccel': cuda_hwaccel.allowed()}
if features['cuda-hwaccel']
    sources += files('video/out/hwdec/hwdec_cuda.c')
endif

cuda_interop = get_option('cuda-interop').require(
    features['cuda-hwaccel'] and (features['gl'] or features['vulkan']),
    error_message: 'cuda-hwaccel and either gl or vulkan were not found!',
)
features += {'cuda-interop': cuda_interop.allowed()}
if features['cuda-interop'] and features['gl']
    sources += files('video/out/hwdec/hwdec_cuda_gl.c')
endif
if features['cuda-interop'] and features['vulkan']
    sources += files('video/out/hwdec/hwdec_cuda_vk.c')
endif

android_media_ndk = get_option('android-media-ndk').require(
    features['android'] and cc.has_header_symbol('media/NdkImageReader.h', 'AIMAGE_FORMAT_PRIVATE')
)
features += {'android-media-ndk': android_media_ndk.allowed()}
if features['android-media-ndk']
    # header only, library is dynamically loaded
    sources += files('video/out/hwdec/hwdec_aimagereader.c')
endif

# this is an arbitrary GLES 3.x symbol
ios_gl = cc.has_header_symbol('OpenGLES/ES3/glext.h', 'GL_RGB32F', required:
    get_option('ios-gl').require(darwin))
features += {'ios-gl': ios_gl}
if features['ios-gl']
    sources += files('video/out/hwdec/hwdec_ios_gl.m')
endif

libva = dependency('libva', version: '>= 1.1.0', required: get_option('vaapi'))

vaapi_drm = dependency('libva-drm', version: '>= 1.1.0', required:
    get_option('vaapi-drm').require(libva.found()))
features += {'vaapi-drm': vaapi_drm.found()}
if features['vaapi-drm']
    dependencies += vaapi_drm
endif

vaapi_wayland = dependency('libva-wayland', version: '>= 1.1.0', required:
    get_option('vaapi-wayland').require(libva.found() and features['wayland']))
features += {'vaapi-wayland': vaapi_wayland.found()}
if features['vaapi-wayland']
    dependencies += vaapi_wayland
endif

vaapi_x11 = dependency('libva-x11', version: '>= 1.1.0', required:
    get_option('vaapi-x11').require(libva.found() and features['x11']))
features += {'vaapi-x11': vaapi_x11.found()}
if features['vaapi-x11']
    dependencies += vaapi_x11
    sources += files('video/out/vo_vaapi.c')
endif

vaapi = get_option('vaapi').require(libva.found() and (features['vaapi-drm'] or
                                    features['vaapi-wayland'] or features['vaapi-x11']))
features += {'vaapi': vaapi.allowed()}

if features['vaapi']
    dependencies += libva
    sources += files('video/filter/vf_vavpp.c',
                     'video/vaapi.c',
                     'video/out/hwdec/hwdec_vaapi.c',
                     'video/out/hwdec/dmabuf_interop_pl.c')
endif

features += {'dmabuf-interop-gl': features['egl'] and drm.found()}
if features['dmabuf-interop-gl']
    dependencies += drm.partial_dependency(includes: true, compile_args: true)
    sources += files('video/out/hwdec/dmabuf_interop_gl.c')
endif

vdpau_opt = get_option('vdpau').require(
    features['x11'],
    error_message: 'x11 was not found!',
)
vdpau = dependency('vdpau', version: '>= 0.2', required: vdpau_opt)
features += {'vdpau': vdpau.found()}
if features['vdpau']
    dependencies += vdpau
    sources += files('video/filter/vf_vdpaupp.c',
                     'video/out/vo_vdpau.c',
                     'video/vdpau.c',
                     'video/vdpau_mixer.c')
endif

features += {'vdpau-gl-x11': features['vdpau'] and gl_x11.allowed()}
if features['vdpau'] and features['vdpau-gl-x11']
    sources += files('video/out/opengl/hwdec_vdpau.c')
endif

videotoolbox_gl = get_option('videotoolbox-gl').require(
    features['gl-cocoa'] or features['ios-gl'],
    error_message: 'gl-cocoa nor ios-gl could be found!',
)
features += {'videotoolbox-gl': videotoolbox_gl.allowed()}
corevideo = dependency('appleframeworks', modules: ['CoreVideo'], required:
    get_option('videotoolbox-pl').require(darwin))
videotoolbox_pl = get_option('videotoolbox-pl').require(
    features['vulkan'] and corevideo.found(),
    error_message: 'vulkan or CV metal support could be found!',
)
features += {'videotoolbox-pl': videotoolbox_pl.allowed()}
if features['videotoolbox-gl'] or features['videotoolbox-pl'] or features['ios-gl']
    sources += files('video/out/hwdec/hwdec_vt.c')
endif
if features['videotoolbox-gl']
    sources += files('video/out/hwdec/hwdec_mac_gl.c')
endif
if features['videotoolbox-pl']
    dependencies += corevideo
    sources += files('video/out/hwdec/hwdec_vt_pl.m')
endif


# macOS features
macos_sdk_version_py = ''
if darwin
    macos_sdk_version_py = find_program(join_paths(source_root, 'TOOLS', 'macos-sdk-version.py'),
                                                   required: true)
endif

macos_sdk_path = ''
macos_sdk_version = '0.0'
if darwin and macos_sdk_version_py.found()
    macos_sdk_info = run_command(macos_sdk_version_py, check: true).stdout().split(',')
    macos_sdk_path = macos_sdk_info[0]
    macos_sdk_version = macos_sdk_info[1]
endif

if macos_sdk_path != ''
    message('Detected macOS sdk path: ' + macos_sdk_path)
endif

if macos_sdk_version != '0.0'
    message('Detected macOS SDK: ' + macos_sdk_version)
    add_languages('objc', native: false)
    objc_link_flags = ['-isysroot', macos_sdk_path, '-L/usr/lib', '-L/usr/local/lib']
    add_project_link_arguments(objc_link_flags, language: ['c', 'objc'])
endif

swift = get_option('swift-build').require(
    darwin and features['cocoa'],
    error_message: 'cocoa was not found',
)
xcrun = find_program('xcrun', required: swift)
swift_ver = '0.0'
if xcrun.found()
    swift_prog = find_program(run_command(xcrun, '-find', 'swiftc', check: true).stdout().strip())
    swift_ver_string = run_command(swift_prog, '-version', check: true).stdout()
    verRe = '''
#!/usr/bin/env python3
import re
import sys
verRe = re.compile("(?i)version\s?([\d.]+)")
swift_ver = verRe.search(sys.argv[1]).group(1)
sys.stdout.write(swift_ver)
'''
    swift_ver = run_command(python, '-c', verRe, swift_ver_string, check: true).stdout()
    message('Detected Swift version: ' + swift_ver)
endif

swift = get_option('swift-build').require(
    swift.allowed() and macos_sdk_version.version_compare('>= 10.15') and swift_ver.version_compare('>= 4.1'),
    error_message: 'A suitable macos sdk version or swift version could not be found!',
)
features += {'swift': swift.allowed()}

if features['cocoa'] and not features['swift'] and get_option('cplayer')
    error('cplayer enabled but no suitable swift version could be found or was disabled!')
endif

swift_sources = []
if features['cocoa'] and features['swift']
    swift_sources += files('osdep/mac/application.swift',
                           'osdep/mac/app_hub.swift',
                           'osdep/mac/clipboard.swift',
                           'osdep/mac/dialog.swift',
                           'osdep/mac/event_helper.swift',
                           'osdep/mac/input_helper.swift',
                           'osdep/mac/log_helper.swift',
                           'osdep/mac/menu_bar.swift',
                           'osdep/mac/option_helper.swift',
                           'osdep/mac/precise_timer.swift',
                           'osdep/mac/presentation.swift',
                           'osdep/mac/swift_compat.swift',
                           'osdep/mac/swift_extensions.swift',
                           'osdep/mac/type_helper.swift',
                           'video/out/mac/common.swift',
                           'video/out/mac/title_bar.swift',
                           'video/out/mac/view.swift',
                           'video/out/mac/window.swift')
endif

macos_10_15_4_features = get_option('macos-10-15-4-features').require(
    macos_sdk_version.version_compare('>= 10.15.4'),
    error_message: 'A macos sdk version >= 10.15.4 could not be found!',
)
features += {'macos-10-15-4-features': macos_10_15_4_features.allowed()}

macos_11_features = get_option('macos-11-features').require(
    macos_sdk_version.version_compare('>= 11'),
    error_message: 'A macos sdk version >= 11 could not be found!',
)
features += {'macos-11-features': macos_11_features.allowed()}

macos_11_3_features = get_option('macos-11-3-features').require(
    macos_sdk_version.version_compare('>= 11.3'),
    error_message: 'A macos sdk version >= 11.3 could not be found!',
)
features += {'macos-11-3-features': macos_11_3_features.allowed()}

macos_12_features = get_option('macos-12-features').require(
    macos_sdk_version.version_compare('>= 12'),
    error_message: 'A macos sdk version >= 12 could not be found!',
)
features += {'macos-12-features': macos_12_features.allowed()}

macos_cocoa_cb = get_option('macos-cocoa-cb').require(
    features['cocoa'] and features['gl-cocoa'] and features['swift'],
    error_message: 'Either cocoa, gl-cocoa or swift could not be found!',
)
features += {'macos-cocoa-cb': macos_cocoa_cb.allowed()}
if features['cocoa'] and features['vulkan'] and features['swift']
    swift_sources += files('video/out/mac_common.swift',
                           'video/out/mac/metal_layer.swift')
    sources += files('video/out/vulkan/context_mac.m')
endif

if features['swift'] and swift_sources.length() > 0
    subdir('osdep/mac')
endif


# manpages
manpage = 'doc/man/mpv.rst'
rst2man = find_program('rst2man', 'rst2man.py', required: get_option('manpage-build'))
if rst2man.found()
    mandir = get_option('mandir')
    custom_target('manpages',
        input: manpage,
        output: 'mpv.1',
        command: [
          docutils_wrapper, rst2man,
          '--record-dependencies', '@DEPFILE@',
          '--strip-elements-with-class=contents',
          '@INPUT@', '@OUTPUT@'],
        depfile: 'mpv.1.dep',
        install: true,
        install_dir: join_paths(mandir, 'man1')
    )
endif

rst2html = find_program('rst2html', 'rst2html.py', required: get_option('html-build'))
if rst2html.found()
    datadir = get_option('datadir')
    custom_target('html-manpages',
        input: manpage,
        output: 'mpv.html',
        command: [
          docutils_wrapper, rst2html,
          '--record-dependencies', '@DEPFILE@',
          '@INPUT@', '@OUTPUT@'],
        depfile: 'mpv.html.dep',
        install: true,
        install_dir: join_paths(datadir, 'doc', 'mpv')
    )
endif

rst2pdf = find_program('rst2pdf', required: get_option('pdf-build'))
if rst2pdf.found()
    dependency_file = rst2pdf.version().version_compare('>=0.100')
    datadir = get_option('datadir')
    custom_target('pdf-manpages',
        input: manpage,
        output: 'mpv.pdf',
        command: [
            docutils_wrapper, rst2pdf,
            '-c', '-b', '1', '--repeat-table-rows',
            dependency_file ? ['--record-dependencies', '@DEPFILE@'] : [],
            '@INPUT@', '-o', '@OUTPUT@'],
        depfile: 'mpv.pdf.dep',
        install: true,
        install_dir: join_paths(datadir, 'doc', 'mpv')
    )
endif


# Set config.h
conf_data = configuration_data()
conf_data.set_quoted('CONFIGURATION', meson.build_options())
conf_data.set_quoted('DEFAULT_OPTICAL_DEVICE', optical_device)

# Loop over all features in the build, create a define and add them to config.h
feature_keys = []
foreach feature, allowed: features
    define = 'HAVE_@0@'.format(feature.underscorify().to_upper())
    conf_data.set10(define, allowed)
    # special handling for lua
    if feature == 'lua' and allowed
        feature_keys += lua_version
        continue
    endif
    if allowed
        feature_keys += feature
    endif
endforeach


# Script to sort the feature_keys object.
feature_sort = '''
#!/usr/bin/env python3
import sys
features = " ".join(sorted(sys.argv[1:]))
sys.stdout.write(features)
'''
feature_str = run_command(python, '-c', feature_sort, feature_keys, check: true).stdout()
conf_data.set_quoted('FULLCONFIG', feature_str)
conf_data.set_quoted('MPV_CONFDIR', join_paths(get_option('prefix'), get_option('sysconfdir'), 'mpv'))
conf_data.set_quoted('PLATFORM', host_machine.system())
configure_file(output : 'config.h', configuration : conf_data)
message('List of enabled features: ' + feature_str)

# These are intentionally not added to conf_data.
features += {'cplayer': get_option('cplayer')}
features += {'libmpv-' + get_option('default_library'): get_option('libmpv')}


# build targets
incdir_public = include_directories('./include')
incdir_player_audio = include_directories('rust/player-audio/include')
incdir_player_ffi = include_directories('rust/player-ffi/include')
client_h_define = cc.get_define('MPV_CLIENT_API_VERSION', prefix: '#include "mpv/client.h"',
                                include_directories: incdir_public)
major = client_h_define.split('|')[0].split('<<')[0].strip('() ')
minor = client_h_define.split('|')[1].strip('() ')
client_api_version = major + '.' + minor + '.0'

libmpv = []
libmpv_objects = []
if get_option('libmpv')
    libmpv = library('mpv', sources, dependencies: dependencies, gnu_symbol_visibility: 'hidden',
                     include_directories: [incdir_public, incdir_player_audio, incdir_player_ffi],
                     link_args: cc.get_supported_link_arguments(['-Wl,-Bsymbolic']),
                     version: client_api_version, install: true,
                     build_by_default: true)
    libmpv_objects = libmpv.extract_all_objects(recursive: true)
endif


if get_option('libmpv')
    pkg = import('pkgconfig')
    pkg.generate(libmpv, version: client_api_version,
                 description: 'mpv media player client library')

    headers = ['include/mpv/client.h', 'include/mpv/stream_cb.h']
    install_headers(headers, subdir: 'mpv')

    # Allow projects to build with libmpv by cloning into ./subprojects/mpv
    libmpv_dep = declare_dependency(include_directories: incdir_public, link_with: libmpv)
    meson.override_dependency('mpv', libmpv_dep)
endif

if get_option('cplayer')
    datadir = get_option('datadir')

    conf_files = ['etc/mpv.conf', 'etc/encoding-profiles.conf', 'etc/input.conf',
                  'etc/mplayer-input.conf', 'etc/restore-old-bindings.conf',
                  'etc/restore-osc-bindings.conf']
    install_data(conf_files, install_dir: join_paths(datadir, 'doc', 'mpv'))

    bash_install_dir = join_paths(datadir, 'bash-completion', 'completions')
    install_data('etc/mpv.bash-completion', install_dir: bash_install_dir, rename: 'mpv')

    zsh_install_dir = join_paths(datadir, 'zsh', 'site-functions')
    install_data('etc/_mpv.zsh', install_dir: zsh_install_dir, rename: '_mpv')

    fish_install_dir = join_paths(datadir, 'fish', 'vendor_completions.d')
    install_data('etc/mpv.fish', install_dir: fish_install_dir, rename: 'mpv.fish')

    install_data('etc/mpv.metainfo.xml', install_dir: join_paths(datadir, 'metainfo'))

    foreach size: ['16x16', '32x32', '64x64', '128x128']
        icon_dir = join_paths(datadir, 'icons', 'hicolor', size, 'apps')
        install_data('etc/mpv-icon-8bit-' + size + '.png', install_dir: icon_dir, rename: 'mpv.png')
    endforeach

    hicolor_dir = join_paths(datadir, 'icons', 'hicolor')
    install_data('etc/mpv-gradient.svg', install_dir: join_paths(hicolor_dir, 'scalable', 'apps'),
                 rename: 'mpv.svg')
    install_data('etc/mpv-symbolic.svg', install_dir: join_paths(hicolor_dir, 'symbolic', 'apps'))

    if get_option('libmpv')
        mpv = executable('mpv', main_fn_source, objects: libmpv_objects,
                         dependencies: dependencies, install: true)
    else
        mpv = executable('mpv', main_fn_source + sources, dependencies: dependencies,
                         include_directories: [incdir_public, incdir_player_audio, incdir_player_ffi],
                         install: true)
    endif

    if darwin
        osxbundle = find_program(join_paths(tools_directory, 'osxbundle.py'), required: true)
        custom_target('macos-bundle',
            output: 'mpv.app',
            depends: version_h,
            command: [osxbundle, mpv, '@SOURCE_ROOT@', '-c', get_option('macos-bundle-category')],
        )
    endif

    if not darwin
        if meson.can_run_host_binaries()
            mpv_protocols = custom_target('mpv_protocols',
                output: 'mpv_protocols',
                command: [mpv, '--no-config','--list-protocols'],
                capture: true,
            )
            mpv_desktop_path = join_paths(source_root, 'etc', 'mpv.desktop')
            custom_target('mpv.desktop',
                input: mpv_protocols,
                output: 'mpv.desktop',
                command: [mpv_desktop, mpv_desktop_path, '@INPUT@', '@OUTPUT@'],
                install: true,
                install_dir: join_paths(datadir, 'applications'),
            )
        else
            install_data('etc/mpv.desktop', install_dir: join_paths(datadir, 'applications'))
        endif
    endif
endif

if get_option('tests')
    subdir('test')
endif

if get_option('fuzzers')
    subdir('fuzzers')
endif

if features['sdl2-audio'] or features['sdl2-gamepad'] or features['sdl2-video']
    warning('You are building with optional SDL2 support. If you encounter build issues, try disabling it.')
endif

summary({'cocoa': features['cocoa'] and features['swift'],
         'drm': features['drm'],
         'libmpv': get_option('libmpv'),
         'lua': features['lua'],
         'opengl': features['gl'],
         'vulkan': features['vulkan'],
         'wayland': features['wayland'],
         'x11': features['x11']},
         bool_yn: true)
