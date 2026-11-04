#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
pub mod backend_alsa;
#[cfg(target_os = "macos")]
pub mod backend_coreaudio;
pub mod clock;
pub mod decode;
pub mod mix;
pub mod pipeline;
pub mod probe;
pub mod resample;
pub mod session;
pub mod sink;

pub fn open_sink(
    kind: &sink::SinkKind,
    channels: usize,
    rate: u32,
    channel_mask: u64,
) -> Result<Box<dyn sink::AudioSink>, String> {
    match kind {
        sink::SinkKind::Null => Ok(Box::new(sink::NullSink::new(channels, rate))),
        sink::SinkKind::Default => open_default_sink(channels, rate, channel_mask),
        sink::SinkKind::Device(device) => {
            #[cfg(target_os = "linux")]
            {
                let _ = channel_mask;
                backend_alsa::AlsaSink::open(device, channels, rate)
                    .map(|sink| Box::new(sink) as Box<dyn sink::AudioSink>)
            }
            #[cfg(target_os = "macos")]
            {
                let _ = channel_mask;
                Err(format!(
                    "audio device '{device}' is not supported by the CoreAudio backend"
                ))
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                let _ = (device, channels, rate, channel_mask);
                Err("no native audio backend on this platform".to_string())
            }
        }
    }
}

pub fn open_default_sink(
    channels: usize,
    rate: u32,
    channel_mask: u64,
) -> Result<Box<dyn sink::AudioSink>, String> {
    #[cfg(target_os = "macos")]
    {
        backend_coreaudio::CoreAudioSink::open(channels, rate, channel_mask)
            .map(|sink| Box::new(sink) as Box<dyn sink::AudioSink>)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = channel_mask;
        backend_alsa::AlsaSink::open("default", channels, rate)
            .map(|sink| Box::new(sink) as Box<dyn sink::AudioSink>)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (channels, rate, channel_mask);
        Err("no native audio backend on this platform".to_string())
    }
}
