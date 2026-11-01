// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod input;
mod playlist;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::Path;

use log::{info, warn};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoderOptions, FinalizeResult};
use symphonia::core::errors::{Error, Result};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::{MetadataOptions, Visual};
use symphonia::core::units::{Duration, Time, TimeBase, Timestamp};

use crate::cli::args::Args;
use crate::output;
use crate::player::input::{Command, Keyboard};
use crate::player::playlist::Playlist;
use crate::ui;

#[derive(Copy, Clone)]
enum SeekPosition {
    Time(Time),
    Timestamp(Timestamp),
}

pub fn run(args: &Args) -> Result<i32> {
    let mut playlist = Playlist::from_input(&args.input)?;

    if args.probe_only || args.verify_only || args.decode_only {
        process_playlist(&playlist, args)
    }
    else {
        play_playlist(&mut playlist, args)
    }
}

fn open_input(path: &Path) -> Result<Box<dyn FormatReader>> {
    // Create a hint to help the format registry guess what format reader is appropriate.
    let mut hint = Hint::new();

    // If the path string is '-' then read from standard input.
    let source = if path.as_os_str() == "-" {
        Box::new(ReadOnlySource::new(std::io::stdin())) as Box<dyn MediaSource>
    }
    else {
        // Provide the file extension as a hint.
        if let Some(extension) = path.extension().and_then(OsStr::to_str) {
            hint.with_extension(extension);
        }

        Box::new(File::open(path)?)
    };

    let mss = MediaSourceStream::new(source, Default::default());

    // Use the default options for format readers and metadata readers.
    let fmt_opts: FormatOptions = Default::default();
    let meta_opts: MetadataOptions = Default::default();

    match symphonia::default::get_probe().probe(&hint, mss, fmt_opts, meta_opts) {
        Ok(format) => Ok(format),
        Err(err) => {
            // The input was not supported by any format reader.
            info!("the input is not supported");
            Err(err)
        }
    }
}

fn process_playlist(playlist: &Playlist, args: &Args) -> Result<i32> {
    if !playlist.is_directory() {
        return process_file(playlist.current(), args);
    }

    let mut succeeded = 0usize;

    for (index, path) in playlist.paths().iter().enumerate() {
        ui::print_line(format_args!("[{}/{}] {}", index + 1, playlist.len(), path.display()));

        match process_file(path, args) {
            Ok(0) => succeeded += 1,
            Ok(_) => (),
            Err(err) => warn!("failed to process '{}': {err}", path.display()),
        }
    }

    Ok(if succeeded == 0 { 1 } else { 0 })
}

fn process_file(path: &Path, args: &Args) -> Result<i32> {
    let mut format = open_input(path)?;

    if args.dump_visuals {
        dump_visuals(&mut format, display_name(path));
    }

    let dec_opts = AudioDecoderOptions::default().gapless(!args.no_gapless);
    let track_num = args.track;

    if args.probe_only {
        // Probe-only mode only prints information about the format, tracks, metadata, etc.
        ui::print_format(path, &mut format);
        Ok(0)
    }
    else if args.verify_only {
        // Verify-only mode decodes and verifies the audio, but does not play it.
        let opts = DecodeOptions { dec_opts: dec_opts.verify(true), track_num };
        decode_only(format, opts)
    }
    else {
        // Decode-only mode decodes the audio, but does not play or verify it.
        let opts = DecodeOptions { dec_opts: dec_opts.verify(false), track_num };
        decode_only(format, opts)
    }
}

/// Options for the decode command.
#[derive(Copy, Clone)]
struct DecodeOptions {
    dec_opts: AudioDecoderOptions,
    track_num: Option<usize>,
}

fn decode_only(mut reader: Box<dyn FormatReader>, opts: DecodeOptions) -> Result<i32> {
    // If the user provided a track number, select that track if it exists, otherwise, select the
    // default audio track.
    let track = opts
        .track_num
        .and_then(|t| reader.tracks().get(t))
        .or_else(|| reader.default_track(TrackType::Audio));

    // Return if no track has been found.
    let track = match track {
        Some(track) => track,
        _ => return Ok(0),
    };

    // Get the audio codec parameters from the track. Return if the track is not an audio track, or
    // does not have any codec parameters.
    let codec_params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(audio)) => audio,
        _ => return Ok(0),
    };

    // Create a decoder for the track.
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(codec_params, &opts.dec_opts)?;

    // Save the track ID to filter demuxed packets.
    let track_id = track.id;

    // Decode all packets, ignoring all decode errors.
    while let Some(packet) = reader.next_packet()? {
        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id != track_id {
            continue;
        }

        // Decode the packet into audio samples.
        match decoder.decode(&packet) {
            Ok(_decoded) => continue,
            Err(Error::DecodeError(err)) => warn!("decode error: {err}"),
            Err(err) => return Err(err),
        }
    }

    info!("end of stream");

    // Finalize the decoder and return the verification result if it's been enabled.
    do_verification(decoder.finalize())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Transition {
    End,
    Next,
    Previous,
    Quit,
}

struct FileOutcome {
    transition: Transition,
    verify_code: i32,
}

enum TrackAction {
    End(i32),
    Seek(Timestamp),
    Next,
    Previous,
    Quit,
}

fn play_playlist(playlist: &mut Playlist, args: &Args) -> Result<i32> {
    let keyboard_enabled = args.input.as_os_str() != "-" && std::io::stdin().is_terminal();

    let keyboard = match Keyboard::new(keyboard_enabled) {
        Ok(keyboard) => keyboard,
        Err(err) => {
            warn!("keyboard control unavailable: {err}");
            Keyboard::disabled()
        }
    };

    ui::set_raw_mode(keyboard.is_raw());

    let initial_seek = initial_seek(args);
    let mut apply_initial_seek = true;
    let mut played_any = false;
    let mut verification_failed = false;

    loop {
        let path = playlist.current();

        if playlist.is_directory() {
            let index = playlist.current_index();
            ui::print_line(format_args!(
                "[{}/{}] {}",
                index + 1,
                playlist.len(),
                path.display()
            ));
        }

        let seek_pos = if apply_initial_seek { initial_seek } else { None };
        apply_initial_seek = false;

        match play_file(path, args, seek_pos, !playlist.is_directory(), &keyboard) {
            Ok(outcome) => {
                if outcome.verify_code != 0 {
                    verification_failed = true;
                }

                match outcome.transition {
                    Transition::End => {
                        played_any = true;

                        if !playlist.advance() {
                            break;
                        }
                    }
                    Transition::Next => {
                        played_any = true;
                        playlist.next();
                    }
                    Transition::Previous => {
                        played_any = true;
                        playlist.previous();
                    }
                    Transition::Quit => {
                        played_any = true;
                        break;
                    }
                }
            }
            Err(err) => {
                if !playlist.is_directory() {
                    return Err(err);
                }

                warn!("failed to play '{}': {err}", path.display());

                if !playlist.advance() {
                    break;
                }
            }
        }
    }

    if verification_failed || !played_any { Ok(1) } else { Ok(0) }
}

fn initial_seek(args: &Args) -> Option<SeekPosition> {
    if let Some(time) = args.seek {
        Time::try_from_secs_f64(time).map(SeekPosition::Time)
    }
    else {
        args.seek_ts.map(|ts| SeekPosition::Timestamp(Timestamp::new(ts)))
    }
}

/// Options for playing a single track.
#[derive(Copy, Clone)]
struct PlayTrackOptions {
    decoder_opts: AudioDecoderOptions,
    track_id: u32,
    seek_ts: Timestamp,
    no_progress: bool,
}

fn play_file(
    path: &Path,
    args: &Args,
    seek_pos: Option<SeekPosition>,
    print_format: bool,
    keyboard: &Keyboard,
) -> Result<FileOutcome> {
    let mut reader = open_input(path)?;

    if args.dump_visuals {
        dump_visuals(&mut reader, display_name(path));
    }

    if print_format {
        ui::print_format(path, &mut reader);
    }

    // If the user provided a track number, select that track if it exists, otherwise, select the
    // default audio track.
    let mut track_id = match select_track(&*reader, args.track) {
        Some(track_id) => track_id,
        None => {
            return Ok(FileOutcome { transition: Transition::End, verify_code: 0 });
        }
    };

    let mut seek_ts = match perform_seek(&mut reader, &mut track_id, seek_pos) {
        Some(seek_ts) => seek_ts,
        None => return Ok(FileOutcome { transition: Transition::End, verify_code: 0 }),
    };

    let decoder_opts = AudioDecoderOptions::default().gapless(!args.no_gapless).verify(args.verify);
    let mut audio_output: Option<Box<dyn output::AudioOutput>> = None;

    loop {
        let opts = PlayTrackOptions {
            decoder_opts,
            track_id,
            seek_ts,
            no_progress: args.no_progress,
        };

        match play_track(&mut reader, &mut audio_output, opts, keyboard) {
            Ok(TrackAction::End(verify_code)) => {
                // Flush the audio output to finish playing back any leftover samples.
                if let Some(audio_output) = audio_output.as_mut() {
                    audio_output.flush();
                }

                return Ok(FileOutcome { transition: Transition::End, verify_code });
            }
            Ok(TrackAction::Next) => {
                return Ok(FileOutcome { transition: Transition::Next, verify_code: 0 });
            }
            Ok(TrackAction::Previous) => {
                return Ok(FileOutcome { transition: Transition::Previous, verify_code: 0 });
            }
            Ok(TrackAction::Quit) => {
                return Ok(FileOutcome { transition: Transition::Quit, verify_code: 0 });
            }
            Ok(TrackAction::Seek(target)) => {
                let seek_pos = Some(SeekPosition::Timestamp(target));

                match perform_seek(&mut reader, &mut track_id, seek_pos) {
                    Some(seeked_ts) => seek_ts = seeked_ts,
                    None => {
                        return Ok(FileOutcome { transition: Transition::End, verify_code: 0 });
                    }
                }
            }
            Err(Error::ResetRequired) => {
                // Handle a demuxer reset.
                track_id = match do_reset(&mut reader) {
                    Some(id) => id,
                    None => {
                        return Ok(FileOutcome { transition: Transition::End, verify_code: 0 });
                    }
                };
                seek_ts = Timestamp::ZERO;
            }
            Err(err) => return Err(err),
        }
    }
}

fn select_track(reader: &dyn FormatReader, track_num: Option<usize>) -> Option<u32> {
    track_num
        .and_then(|t| reader.tracks().get(t))
        .or_else(|| reader.default_track(TrackType::Audio))
        .map(|track| track.id)
}

fn perform_seek(
    reader: &mut Box<dyn FormatReader>,
    track_id: &mut u32,
    seek_pos: Option<SeekPosition>,
) -> Option<Timestamp> {
    let seek_pos = match seek_pos {
        Some(seek_pos) => seek_pos,
        None => return Some(Timestamp::ZERO),
    };

    let seek_to = match seek_pos {
        SeekPosition::Time(time) => SeekTo::Time { time, track_id: Some(*track_id) },
        SeekPosition::Timestamp(ts) => SeekTo::Timestamp { ts, track_id: *track_id },
    };

    // Attempt the seek. If the seek fails, ignore the error and return a seek timestamp of 0 so
    // that no samples are trimmed.
    match reader.seek(SeekMode::Accurate, seek_to) {
        Ok(seeked_to) => Some(seeked_to.required_ts),
        Err(Error::ResetRequired) => {
            // Handle a demuxer reset.
            *track_id = do_reset(reader)?;
            Some(Timestamp::ZERO)
        }
        Err(err) => {
            // Don't give-up on a seek error.
            warn!("seek error: {err}");
            Some(Timestamp::ZERO)
        }
    }
}

fn play_track(
    reader: &mut Box<dyn FormatReader>,
    audio_output: &mut Option<Box<dyn output::AudioOutput>>,
    opts: PlayTrackOptions,
    keyboard: &Keyboard,
) -> Result<TrackAction> {
    // Get the selected track using the track ID.
    let track = match reader.tracks().iter().find(|track| track.id == opts.track_id) {
        Some(track) => track,
        _ => return Ok(TrackAction::End(0)),
    };

    // Get the audio codec parameters from the track. Return if the track is not an audio track, or
    // does not have any codec parameters.
    let codec_params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(audio)) => audio,
        _ => return Ok(TrackAction::End(0)),
    };

    // Create a decoder for the track.
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(codec_params, &opts.decoder_opts)?;

    // Get the selected track's timebase, duration, and start timestamp.
    let tb = track.time_base;
    let dur = track.duration;
    let start_ts = track.start_ts;

    // The timestamp of the most recently played packet, used as the origin for relative seeks.
    let mut current_ts = opts.seek_ts;

    // Decode and play the packets belonging to the selected track.
    while let Some(packet) = reader.next_packet()? {
        if let Some(command) = keyboard.poll() {
            match command {
                Command::SeekBy(offset_secs) => {
                    match tb {
                        Some(tb) => {
                            let target = clamp_seek(current_ts, offset_secs, tb, start_ts, dur);

                            // Drop the audio output so that stale buffered samples are discarded.
                            *audio_output = None;

                            return Ok(TrackAction::Seek(target));
                        }
                        None => warn!("cannot seek: the track has no time base"),
                    }
                }
                Command::Next => {
                    *audio_output = None;
                    return Ok(TrackAction::Next);
                }
                Command::Previous => {
                    *audio_output = None;
                    return Ok(TrackAction::Previous);
                }
                Command::Quit => {
                    *audio_output = None;
                    return Ok(TrackAction::Quit);
                }
            }
        }

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id != opts.track_id {
            continue;
        }

        //Print out new metadata.
        while !reader.metadata().is_latest() {
            reader.metadata().pop();

            if let Some(rev) = reader.metadata().current() {
                ui::print_update(rev);
            }
        }

        // Decode the packet into audio samples.
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // If the audio output is not open, try to open it.
                if audio_output.is_none() {
                    // Get the capacity of the decoded buffer. Note that this is capacity, not
                    // length! The output will use this to size its internal buffers appropriately.
                    let duration = Duration::from(decoded.capacity() as u64);

                    // Try to open the audio output.
                    audio_output.replace(output::try_open(decoded.spec(), duration).map_err(
                        |_| Error::IoError(std::io::Error::other("failed to open audio output")),
                    )?);
                }
                else {
                    // TODO: Check the audio spec. and duration hasn't changed.
                }

                // Write the decoded audio samples to the audio output if the presentation timestamp
                // for the packet is >= the seeked position (0 if not seeking).
                //
                // NOTE: This is a half-baked approach to seeking! After seeking the reader, packets
                // should be decoded and *samples* discarded up-to the exact *sample* indicated by
                // required_ts. The current approach will discard extra samples if seeking to a
                // sample within a packet.
                if packet.pts >= opts.seek_ts {
                    current_ts = packet.pts;

                    if !opts.no_progress {
                        let _ = ui::print_progress(packet.pts, dur, tb);
                    }

                    if let Some(audio_output) = audio_output {
                        audio_output.write(decoded).map_err(|_| {
                            Error::IoError(std::io::Error::other("failed to write audio output"))
                        })?;
                    }
                }
            }
            Err(Error::DecodeError(err)) => {
                // Decode errors are not fatal. Print the error message and try to decode the next
                // packet as usual.
                warn!("decode error: {err}");
            }
            Err(err) => return Err(err),
        }
    }

    if !opts.no_progress {
        ui::print_line(format_args!(""));
    }

    info!("end of stream");

    // Finalize the decoder and return the verification result if it's been enabled.
    do_verification(decoder.finalize()).map(TrackAction::End)
}

fn clamp_seek(
    current: Timestamp,
    offset_secs: i64,
    time_base: TimeBase,
    start: Timestamp,
    duration: Option<Duration>,
) -> Timestamp {
    let offset_nanos = offset_secs.unsigned_abs().saturating_mul(1_000_000_000);
    let offset = time_base
        .calc_timestamp(Time::from_nanos_u64(offset_nanos))
        .unwrap_or(Timestamp::ZERO)
        .get();

    let offset = if offset_secs < 0 { offset.saturating_neg() } else { offset };
    let target = Timestamp::new(current.get().saturating_add(offset)).max(start);

    match duration {
        Some(duration) => {
            let last_seekable_ts = start
                .checked_add(duration)
                .unwrap_or(Timestamp::MAX)
                .saturating_sub(Duration::new(1))
                .max(start);
            target.min(last_seekable_ts)
        }
        None => target,
    }
}

fn do_reset(reader: &mut Box<dyn FormatReader>) -> Option<u32> {
    // The demuxer indicated that a reset is required. This is sometimes seen with streaming OGG
    // (e.g., Icecast) wherein the entire contents of the container change (new tracks, codecs,
    // metadata, etc.). Therefore, we must select a new track and recreate the decoder.
    ui::print_blank();
    ui::print_tracks(reader.tracks());

    // Select the default audio track since the user's selected track number may no longer be exist
    // or make sense.
    reader.default_track(TrackType::Audio).map(|track| track.id)
}

fn do_verification(finalization: FinalizeResult) -> Result<i32> {
    match finalization.verify_ok {
        Some(is_ok) => {
            // Got a verification result.
            ui::print_line(format_args!("verification: {}", if is_ok { "passed" } else { "failed" }));

            Ok(i32::from(!is_ok))
        }
        // Verification not enabled by user, or unsupported by the codec.
        _ => Ok(0),
    }
}

fn display_name(path: &Path) -> &OsStr {
    match path.file_name() {
        Some(name) if name != "-" => name,
        _ => OsStr::new("NoName"),
    }
}

fn dump_visuals(format: &mut Box<dyn FormatReader>, file_name: &OsStr) {
    if let Some(metadata) = format.metadata().current() {
        for (i, visual) in metadata.media.visuals.iter().enumerate() {
            dump_visual(visual, file_name, i);
        }
    }
}

fn dump_visual(visual: &Visual, file_name: &OsStr, index: usize) {
    let extension = match visual.media_type.as_ref().map(|m| m.to_ascii_lowercase()).as_deref() {
        Some("image/bmp") => ".bmp",
        Some("image/gif") => ".gif",
        Some("image/jpeg") => ".jpg",
        Some("image/png") => ".png",
        _ => ".bin",
    };

    let mut out_file_name = OsString::from(file_name);
    out_file_name.push(format!("-{index:0>2}{extension}"));

    if let Err(err) = File::create(out_file_name).and_then(|mut file| file.write_all(&visual.data))
    {
        warn!("failed to dump visual due to error {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_seek;
    use symphonia::core::units::{Duration, TimeBase, Timestamp};

    fn millisecond_time_base() -> TimeBase {
        TimeBase::try_new(1, 1000).expect("1/1000 is a valid time base")
    }

    #[test]
    fn seek_forward_moves_by_five_seconds() {
        let target = clamp_seek(
            Timestamp::new(10_000),
            5,
            millisecond_time_base(),
            Timestamp::ZERO,
            Some(Duration::new(60_000)),
        );

        assert_eq!(target, Timestamp::new(15_000));
    }

    #[test]
    fn seek_backward_moves_by_five_seconds() {
        let target = clamp_seek(
            Timestamp::new(10_000),
            -5,
            millisecond_time_base(),
            Timestamp::ZERO,
            Some(Duration::new(60_000)),
        );

        assert_eq!(target, Timestamp::new(5_000));
    }

    #[test]
    fn seek_backward_clamps_to_track_start() {
        let target = clamp_seek(
            Timestamp::new(3_000),
            -5,
            millisecond_time_base(),
            Timestamp::ZERO,
            Some(Duration::new(60_000)),
        );

        assert_eq!(target, Timestamp::ZERO);
    }

    #[test]
    fn seek_forward_clamps_to_last_seekable_timestamp() {
        let target = clamp_seek(
            Timestamp::new(58_000),
            5,
            millisecond_time_base(),
            Timestamp::ZERO,
            Some(Duration::new(60_000)),
        );

        assert_eq!(target, Timestamp::new(59_999));
    }

    #[test]
    fn seek_forward_is_unbounded_without_a_duration() {
        let target =
            clamp_seek(Timestamp::new(58_000), 5, millisecond_time_base(), Timestamp::ZERO, None);

        assert_eq!(target, Timestamp::new(63_000));
    }

    #[test]
    fn seek_clamps_to_a_nonzero_track_start() {
        let target = clamp_seek(
            Timestamp::new(1_500),
            -5,
            millisecond_time_base(),
            Timestamp::new(1_000),
            Some(Duration::new(60_000)),
        );

        assert_eq!(target, Timestamp::new(1_000));
    }

    #[test]
    fn seek_uses_the_track_time_base() {
        let time_base = TimeBase::try_new(1, 48_000).expect("1/48000 is a valid time base");
        let target = clamp_seek(
            Timestamp::new(480_000),
            5,
            time_base,
            Timestamp::ZERO,
            Some(Duration::new(4_800_000)),
        );

        assert_eq!(target, Timestamp::new(720_000));
    }
}
