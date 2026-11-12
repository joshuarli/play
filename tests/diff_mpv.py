"""Differential playback against real mpv over a generated media corpus.

Each case is a short file produced by ffmpeg. A case fails when the reference
mpv plays it to the end and this player does not, or when the two disagree on
duration or video size. Cases that mpv itself cannot play are reported, not
failed.
"""
import shutil
import subprocess
import time
from pathlib import Path

from harness import ROOT, Failure, Mpv

REFERENCE = Path("/opt/homebrew/bin/mpv")
CORPUS = ROOT / ".work" / "fixtures" / "diff"
SECONDS = 2
SPEED = 4.0
DURATION_TOLERANCE = 0.15

VIDEO = ["-f", "lavfi", "-i", f"testsrc2=s=320x240:r=25:d={SECONDS}"]


def tone(channels, rate):
    layout = {1: "mono", 2: "stereo", 3: "2.1", 4: "quad", 6: "5.1", 8: "7.1"}[channels]
    return ["-f", "lavfi", "-i",
            f"sine=frequency=440:sample_rate={rate}:duration={SECONDS}",
            "-af", f"aformat=channel_layouts={layout}"]


def audio_only(codec, channels, rate, extension, extra=()):
    return [*tone(channels, rate), "-c:a", codec, *extra], extension


def with_video(video_codec, extension, video_extra=(), audio_codec="aac", channels=2, rate=48000,
               source=VIDEO):
    return [*source, *tone(channels, rate), "-c:v", video_codec, *video_extra,
            "-c:a", audio_codec, "-shortest"], extension


def video_only(video_codec, extension, video_extra=(), source=VIDEO):
    return [*source, "-c:v", video_codec, *video_extra], extension


CASES = {
    "h264-aac.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p"]),
    "h264-odd-size.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p"],
                                    source=["-f", "lavfi", "-i", f"testsrc2=s=321x241:r=25:d={SECONDS}"]),
    "h264-anamorphic.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv420p", "-aspect", "16:9"]),
    "h264-rotated.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p", "-metadata:s:v",
                                                      "rotate=90"]),
    "h264-yuv444.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv444p"]),
    "h264-yuvj420.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuvj420p"]),
    "h264-high10.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv420p10le"]),
    "h264-29.97.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p"],
                                 source=["-f", "lavfi", "-i",
                                         f"testsrc2=s=320x240:r=30000/1001:d={SECONDS}"]),
    "h264-vfr.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv420p", "-vf",
                                                  "setpts='if(gte(N,25),PTS*2,PTS)'", "-fps_mode", "vfr"]),
    "h264-only.mp4": video_only("libx264", "mp4", ["-pix_fmt", "yuv420p"]),
    "h264-short.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p", "-t", "0.3"]),
    "hevc-aac.mp4": with_video("libx265", "mp4", ["-pix_fmt", "yuv420p", "-tag:v", "hvc1"]),
    "hevc-main10.mkv": with_video("libx265", "mkv", ["-pix_fmt", "yuv420p10le"]),
    "vp9-opus.webm": with_video("libvpx-vp9", "webm", ["-deadline", "realtime", "-cpu-used", "8"],
                                audio_codec="libopus"),
    "vp9-vorbis.webm": with_video("libvpx-vp9", "webm", ["-deadline", "realtime", "-cpu-used", "8"],
                                  audio_codec="libvorbis"),
    "vp9-profile2.webm": with_video("libvpx-vp9", "webm", ["-deadline", "realtime", "-cpu-used", "8",
                                                           "-pix_fmt", "yuv420p10le"],
                                    audio_codec="libopus"),
    "av1-opus.mkv": with_video("libsvtav1", "mkv", ["-preset", "12"], audio_codec="libopus"),
    "av1-aac.mp4": with_video("libaom-av1", "mp4", ["-cpu-used", "8", "-row-mt", "1"]),
    "mpeg4-mp3.avi": with_video("mpeg4", "avi", ["-q:v", "5"], audio_codec="libmp3lame"),
    "mjpeg-pcm.avi": with_video("mjpeg", "avi", ["-q:v", "5"], audio_codec="pcm_s16le"),
    "mjpeg.mov": with_video("mjpeg", "mov", ["-q:v", "5"]),
    "h264-alac.mov": with_video("libx264", "mov", ["-pix_fmt", "yuv420p"], audio_codec="alac"),
    "h264-flac.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv420p"], audio_codec="flac"),
    "h264-aac51.mp4": with_video("libx264", "mp4", ["-pix_fmt", "yuv420p"], channels=6),
    "h264-aac71.mkv": with_video("libx264", "mkv", ["-pix_fmt", "yuv420p"], channels=8),
    "aac-mono-22050.m4a": audio_only("aac", 1, 22050, "m4a"),
    "aac-stereo-44100.m4a": audio_only("aac", 2, 44100, "m4a"),
    "aac-51.m4a": audio_only("aac", 6, 48000, "m4a"),
    "aac-71.m4a": audio_only("aac", 8, 48000, "m4a"),
    "aac-quad.m4a": audio_only("aac", 4, 48000, "m4a"),
    "aac-he.m4a": audio_only("aac_at", 2, 44100, "m4a", ["-profile:a", "4", "-b:a", "48k"]),
    "aac-he-v2.m4a": audio_only("aac_at", 2, 44100, "m4a", ["-profile:a", "28", "-b:a", "24k"]),
    "aac-96k.m4a": audio_only("aac", 2, 96000, "m4a"),
    "aac-8k.m4a": audio_only("aac", 1, 8000, "m4a"),
    "mp3-mono-32k.mp3": audio_only("libmp3lame", 1, 32000, "mp3"),
    "mp3-stereo.mp3": audio_only("libmp3lame", 2, 44100, "mp3"),
    "mp3-cbr-8k.mp3": audio_only("libmp3lame", 1, 8000, "mp3", ["-b:a", "16k"]),
    "opus-mono.opus": audio_only("libopus", 1, 48000, "opus"),
    "opus-stereo.ogg": audio_only("libopus", 2, 48000, "ogg"),
    "opus-51.opus": audio_only("libopus", 6, 48000, "opus"),
    "opus-71.mka": audio_only("libopus", 8, 48000, "mka"),
    "opus-24k.opus": audio_only("libopus", 2, 24000, "opus"),
    "vorbis-mono.ogg": audio_only("libvorbis", 1, 44100, "ogg"),
    "vorbis-51.ogg": audio_only("libvorbis", 6, 48000, "ogg"),
    "flac-16.flac": audio_only("flac", 2, 44100, "flac"),
    "flac-24-96k.flac": audio_only("flac", 2, 96000, "flac", ["-sample_fmt", "s32"]),
    "flac-mono.flac": audio_only("flac", 1, 48000, "flac"),
    "flac-51.flac": audio_only("flac", 6, 48000, "flac"),
    "flac-71.flac": audio_only("flac", 8, 48000, "flac"),
    "alac.m4a": audio_only("alac", 2, 44100, "m4a"),
    "alac-51.m4a": audio_only("alac", 6, 48000, "m4a"),
    "wav-s16.wav": audio_only("pcm_s16le", 2, 44100, "wav"),
    "wav-s24.wav": audio_only("pcm_s24le", 2, 48000, "wav"),
    "wav-f32.wav": audio_only("pcm_f32le", 2, 48000, "wav"),
    "wav-mono-11025.wav": audio_only("pcm_s16le", 1, 11025, "wav"),
    "wav-51.wav": audio_only("pcm_s16le", 6, 48000, "wav"),
    "wav-71.wav": audio_only("pcm_s16le", 8, 48000, "wav"),
    "wav-3ch.wav": audio_only("pcm_s16le", 3, 48000, "wav"),
}


KNOWN_GAPS = {
}


def build(name):
    recipe, _ = CASES[name]
    path = CORPUS / name
    if path.exists() and path.stat().st_size > 0:
        return path
    CORPUS.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *recipe, str(path)],
                          capture_output=True, text=True, timeout=300)
    if proc.returncode != 0:
        raise Failure(f"ffmpeg could not build {name}: {proc.stderr.strip()[-300:]}")
    return path


def play(player, path):
    """Play a file headless at SPEED and report how it went."""
    import harness

    harness.MPV = player
    outcome = {"loaded": False, "ended": False, "error": None}
    try:
        with Mpv(["--vo=null", "--ao=null", f"--speed={SPEED}"]) as mpv:
            mpv.load(path)
            try:
                mpv.wait_event("file-loaded", timeout=10)
            except Failure as exc:
                outcome["error"] = f"no file-loaded: {str(exc).splitlines()[0]}"
                return outcome
            outcome["loaded"] = True
            outcome["duration"] = mpv.try_get_property("duration")
            outcome["width"] = mpv.try_get_property("width")
            outcome["height"] = mpv.try_get_property("height")
            deadline = time.monotonic() + SECONDS / SPEED * 4 + 8
            while time.monotonic() < deadline:
                ends = [event for event in mpv.events if event.get("event") == "end-file"]
                if ends:
                    reason = ends[0].get("reason")
                    outcome["ended"] = reason == "eof"
                    if reason != "eof":
                        outcome["error"] = f"end-file reason {reason}"
                    break
                if mpv.try_get_property("eof-reached") is True:
                    outcome["ended"] = True
                    break
                time.sleep(0.05)
            else:
                outcome["error"] = f"still playing at {mpv.try_get_property('time-pos')}"
            errors = mpv.error_lines()
            if errors and outcome["error"] is None and not outcome["ended"]:
                outcome["error"] = errors[0]
            outcome["log_errors"] = errors[:2]
    except Failure as exc:
        outcome["error"] = str(exc).splitlines()[0]
    return outcome


def run(results, player, only=None):
    if not REFERENCE.exists() or shutil.which("ffmpeg") is None:
        results.check("mpv-diff: reference mpv and ffmpeg installed", False,
                      f"needs {REFERENCE} and ffmpeg")
        return
    for name in sorted(CASES):
        if only and only not in name:
            continue
        try:
            path = build(name)
        except Failure as exc:
            results.check(f"mpv-diff {name}: fixture", False, str(exc))
            continue
        reference = play(REFERENCE, path)
        ours = play(player, path)
        label = f"mpv-diff {name}"
        if not (reference["loaded"] and reference["ended"]):
            print(f"SKIP {label}: mpv does not play it ({reference['error']})", flush=True)
            continue
        plays = ours["loaded"] and ours["ended"]
        if name in KNOWN_GAPS:
            if plays:
                results.check(f"{label}: known gap {KNOWN_GAPS[name]} still fails", False,
                              "it plays now; remove it from KNOWN_GAPS")
            else:
                print(f"KNOWN {label}: {KNOWN_GAPS[name]} ({ours.get('error')})", flush=True)
            continue
        results.check(f"{label}: plays to the end", plays, str(ours.get("error")))
        if not ours["loaded"]:
            continue
        expected, got = reference.get("duration"), ours.get("duration")
        results.check(f"{label}: duration",
                      isinstance(expected, (int, float)) and isinstance(got, (int, float))
                      and abs(expected - got) <= DURATION_TOLERANCE,
                      f"mpv {expected}, ours {got}")
        if reference.get("width"):
            results.check(f"{label}: video size",
                          (ours.get("width"), ours.get("height"))
                          == (reference.get("width"), reference.get("height")),
                          f"mpv {reference.get('width')}x{reference.get('height')}, "
                          f"ours {ours.get('width')}x{ours.get('height')}")
