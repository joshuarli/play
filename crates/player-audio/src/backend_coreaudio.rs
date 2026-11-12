use crate::sink::AudioSink;
use crate::underrun::Underruns;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Condvar, Mutex};

type OSStatus = i32;
type AudioQueueRef = *mut c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;
type AudioQueuePropertyID = u32;
type Boolean = u8;

const NO_ERR: OSStatus = 0;
const FORMAT_LINEAR_PCM: u32 = 0x6c70636d;
const FLAG_FLOAT_PACKED: u32 = 0x1 | 0x8;
const LAYOUT_USE_BITMAP: u32 = 0x10000;
const PROPERTY_CHANNEL_LAYOUT: AudioQueuePropertyID = u32::from_be_bytes(*b"aqcl");

const BUFFER_COUNT: usize = 10;
const FRAMES_PER_BUFFER: usize = 1024;

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioChannelLayoutHeader {
    tag: u32,
    bitmap: u32,
    descriptions: u32,
}

#[repr(C)]
struct AudioQueueBuffer {
    capacity: u32,
    _pad0: u32,
    data: *mut u8,
    byte_size: u32,
    _pad1: u32,
    user: *mut c_void,
    packet_capacity: u32,
    _pad2: u32,
    packet_descriptions: *const c_void,
    packet_count: u32,
    _pad3: u32,
}

unsafe extern "C" {
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: unsafe extern "C" fn(*mut c_void, AudioQueueRef, AudioQueueBufferRef),
        user_data: *mut c_void,
        run_loop: *mut c_void,
        run_loop_mode: *mut c_void,
        flags: u32,
        out_queue: *mut AudioQueueRef,
    ) -> OSStatus;
    fn AudioQueueAllocateBuffer(
        queue: AudioQueueRef,
        byte_size: u32,
        out_buffer: *mut AudioQueueBufferRef,
    ) -> OSStatus;
    fn AudioQueueEnqueueBuffer(
        queue: AudioQueueRef,
        buffer: AudioQueueBufferRef,
        packet_descs: u32,
        descs: *const c_void,
    ) -> OSStatus;
    fn AudioQueueStart(queue: AudioQueueRef, start_time: *const c_void) -> OSStatus;
    fn AudioQueuePause(queue: AudioQueueRef) -> OSStatus;
    fn AudioQueueStop(queue: AudioQueueRef, immediate: Boolean) -> OSStatus;
    fn AudioQueueSetProperty(
        queue: AudioQueueRef,
        id: AudioQueuePropertyID,
        data: *const c_void,
        data_size: u32,
    ) -> OSStatus;
    fn AudioQueueDispose(queue: AudioQueueRef, immediate: Boolean) -> OSStatus;
}

struct QueueState {
    buffers: Vec<AudioQueueBufferRef>,
    free: Vec<AudioQueueBufferRef>,
    in_flight: usize,
    streaming: bool,
    underruns: Underruns,
}

struct Shared {
    state: Mutex<QueueState>,
    ready: Condvar,
}

unsafe extern "C" fn buffer_done(user: *mut c_void, _queue: AudioQueueRef, buffer: AudioQueueBufferRef) {
    // SAFETY: user points to the Shared owned by the live sink; the queue
    // guarantees no callback fires after dispose returns.
    let shared = unsafe { &*(user as *const Shared) };
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if state.free.contains(&buffer) {
        return;
    }
    state.in_flight = state.in_flight.saturating_sub(1);
    if state.in_flight == 0 && state.streaming {
        state.underruns.starved();
    }
    state.free.push(buffer);
    shared.ready.notify_one();
}

pub struct CoreAudioSink {
    queue: AudioQueueRef,
    shared: Box<Shared>,
    channels: usize,
    rate: u32,
    staged_frames: usize,
    current: AudioQueueBufferRef,
    started: bool,
}

impl CoreAudioSink {
    pub fn open(channels: usize, rate: u32, channel_mask: u64) -> Result<Self, String> {
        if channels == 0 || channels > 8 || rate == 0 {
            return Err("coreaudio needs 1-8 channels and a positive rate".to_string());
        }
        if channel_mask > u32::MAX as u64 {
            return Err("coreaudio channel mask does not fit the bitmap".to_string());
        }
        let bytes_per_frame = (channels * 4) as u32;
        let format = AudioStreamBasicDescription {
            sample_rate: rate as f64,
            format_id: FORMAT_LINEAR_PCM,
            format_flags: FLAG_FLOAT_PACKED,
            bytes_per_packet: bytes_per_frame,
            frames_per_packet: 1,
            bytes_per_frame,
            channels_per_frame: channels as u32,
            bits_per_channel: 32,
            reserved: 0,
        };
        let mut shared = Box::new(Shared {
            state: Mutex::new(QueueState {
                buffers: Vec::with_capacity(BUFFER_COUNT),
                free: Vec::with_capacity(BUFFER_COUNT),
                in_flight: 0,
                streaming: false,
                underruns: Underruns::new(),
            }),
            ready: Condvar::new(),
        });
        let user = (&mut *shared as *mut Shared).cast::<c_void>();
        let mut queue: AudioQueueRef = ptr::null_mut();
        // SAFETY: format describes f32 interleaved PCM; user outlives the queue.
        let status = unsafe {
            AudioQueueNewOutput(
                &format,
                buffer_done,
                user,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                &mut queue,
            )
        };
        if status != NO_ERR || queue.is_null() {
            return Err(format!("AudioQueueNewOutput failed: {status}"));
        }
        let layout = AudioChannelLayoutHeader {
            tag: LAYOUT_USE_BITMAP,
            bitmap: channel_mask as u32,
            descriptions: 0,
        };
        // SAFETY: layout is a valid 12-byte bitmap header for this queue.
        let status = unsafe {
            AudioQueueSetProperty(
                queue,
                PROPERTY_CHANNEL_LAYOUT,
                (&layout as *const AudioChannelLayoutHeader).cast(),
                12,
            )
        };
        if status != NO_ERR {
            // SAFETY: the queue was created above and is not yet shared.
            unsafe { AudioQueueDispose(queue, 1) };
            return Err(format!("channel layout rejected: {status}"));
        }
        let mut sink = CoreAudioSink {
            queue,
            shared,
            channels,
            rate,
            staged_frames: 0,
            current: ptr::null_mut(),
            started: false,
        };
        let buffer_bytes = (FRAMES_PER_BUFFER * channels * 4) as u32;
        for _ in 0..BUFFER_COUNT {
            let mut buffer: AudioQueueBufferRef = ptr::null_mut();
            // SAFETY: the queue is live and buffer receives the new buffer.
            let status =
                unsafe { AudioQueueAllocateBuffer(queue, buffer_bytes, &mut buffer) };
            if status != NO_ERR || buffer.is_null() {
                // SAFETY: the queue was created above and is not yet shared.
                unsafe { AudioQueueDispose(queue, 1) };
                return Err(format!("buffer allocation failed: {status}"));
            }
            // SAFETY: the buffer is valid and its capacity covers the request.
            unsafe {
                assert!((*buffer).capacity >= buffer_bytes);
                assert!(!(*buffer).data.is_null());
            }
            let mut state = sink
                .shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.buffers.push(buffer);
            state.free.push(buffer);
        }
        sink.refill()?;
        Ok(sink)
    }

    fn refill(&mut self) -> Result<(), String> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        while state.free.is_empty() {
            state = self
                .shared
                .ready
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
        self.current = state.free.pop().unwrap_or(ptr::null_mut());
        self.staged_frames = 0;
        if self.current.is_null() {
            return Err("audio queue returned a null buffer".to_string());
        }
        Ok(())
    }

    fn enqueue_current(&mut self) -> Result<(), String> {
        if self.current.is_null() || self.staged_frames == 0 {
            return Ok(());
        }
        // SAFETY: current is a live queue buffer; the staged prefix holds frames.
        unsafe {
            (*self.current).byte_size =
                (self.staged_frames * self.channels * 4) as u32;
        }
        // SAFETY: the filled buffer belongs to this queue.
        let status = unsafe { AudioQueueEnqueueBuffer(self.queue, self.current, 0, ptr::null()) };
        if status != NO_ERR {
            return Err(format!("AudioQueueEnqueueBuffer failed: {status}"));
        }
        {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.in_flight += 1;
            state.streaming = true;
            state.underruns.data();
        }
        self.current = ptr::null_mut();
        self.staged_frames = 0;
        if !self.started {
            // SAFETY: the queue holds at least one buffer.
            let status = unsafe { AudioQueueStart(self.queue, ptr::null()) };
            if status != NO_ERR {
                return Err(format!("AudioQueueStart failed: {status}"));
            }
            self.started = true;
        }
        Ok(())
    }

    fn stop_streaming(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .streaming = false;
    }

    fn status(status: OSStatus, what: &str) -> Result<(), String> {
        if status == NO_ERR {
            Ok(())
        } else {
            Err(format!("{what} failed: {status}"))
        }
    }
}

impl AudioSink for CoreAudioSink {
    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        if !frames.len().is_multiple_of(self.channels) {
            return Err("sink write must hold whole frames".to_string());
        }
        let mut frames = frames;
        while !frames.is_empty() {
            if self.current.is_null() {
                self.refill()?;
            }
            let room = FRAMES_PER_BUFFER - self.staged_frames;
            let take = room.min(frames.len() / self.channels);
            // SAFETY: current is live with FRAMES_PER_BUFFER frames of capacity;
            // the staged region plus take stays inside it.
            unsafe {
                let base = (*self.current).data as *mut f32;
                ptr::copy_nonoverlapping(
                    frames.as_ptr(),
                    base.add(self.staged_frames * self.channels),
                    take * self.channels,
                );
            }
            self.staged_frames += take;
            frames = &frames[take * self.channels..];
            if self.staged_frames == FRAMES_PER_BUFFER {
                self.enqueue_current()?;
            }
        }
        Ok(())
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn queued_frames(&self) -> u64 {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        (state.in_flight * FRAMES_PER_BUFFER + self.staged_frames) as u64
    }

    fn pause(&mut self) -> Result<(), String> {
        // SAFETY: the queue is live.
        Self::status(unsafe { AudioQueuePause(self.queue) }, "AudioQueuePause")?;
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .underruns
            .pause();
        Ok(())
    }

    fn underruns(&self) -> u64 {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .underruns
            .count()
    }

    fn backend_name(&self) -> &'static str {
        "coreaudio"
    }

    fn resume(&mut self) -> Result<(), String> {
        // SAFETY: the queue is live.
        Self::status(
            unsafe { AudioQueueStart(self.queue, ptr::null()) },
            "AudioQueueStart",
        )
    }

    fn flush(&mut self) -> Result<(), String> {
        self.stop_streaming();
        // SAFETY: the queue is live; an immediate stop discards every
        // enqueued buffer, and it does not report each one to the callback.
        Self::status(unsafe { AudioQueueStop(self.queue, 1) }, "AudioQueueStop")?;
        {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.free = state.buffers.clone();
            state.in_flight = 0;
            state.underruns.flush();
        }
        self.staged_frames = 0;
        self.current = ptr::null_mut();
        self.started = false;
        self.refill()
    }

    fn finish(&mut self) -> Result<(), String> {
        self.enqueue_current()?;
        self.stop_streaming();
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .underruns
            .finish();
        Ok(())
    }
}

impl Drop for CoreAudioSink {
    fn drop(&mut self) {
        self.stop_streaming();
        // SAFETY: the queue is live until dispose returns; no callback can
        // fire afterwards, so the shared state dies safely with the sink.
        unsafe { AudioQueueDispose(self.queue, 1) };
        let underruns = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .underruns
            .count();
        if underruns > 0 {
            eprintln!("[ao/coreaudio] {underruns} underruns: the output queue ran dry");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_field_offsets_match_sdk() {
        assert_eq!(std::mem::size_of::<AudioQueueBuffer>(), 56);
        assert_eq!(std::mem::size_of::<AudioStreamBasicDescription>(), 40);
        assert_eq!(std::mem::size_of::<AudioChannelLayoutHeader>(), 12);
    }

    #[test]
    #[ignore = "needs a macOS audio device; run explicitly for the manual checklist"]
    fn repeated_flushes_keep_every_buffer() {
        let mut sink = CoreAudioSink::open(2, 48000, 0x3).expect("default output opens");
        let silence = vec![0.0f32; 1500 * 2];
        for _ in 0..50 {
            sink.write(&silence).expect("write does not starve after flushes");
            sink.flush().expect("flush works");
        }
        let state = sink.shared.state.lock().unwrap();
        assert_eq!(state.free.len() + usize::from(!sink.current.is_null()), BUFFER_COUNT);
        assert_eq!(state.in_flight, 0);
    }

    #[test]
    #[ignore = "needs a macOS audio device; run explicitly for the manual checklist"]
    fn device_smoke() {
        let mut sink = CoreAudioSink::open(2, 48000, 0x3).expect("default output opens");
        let tone: Vec<f32> = (0..4800)
            .flat_map(|frame| {
                let sample =
                    (2.0 * std::f32::consts::PI * 440.0 * frame as f32 / 48000.0).sin() * 0.1;
                [sample, sample]
            })
            .collect();
        sink.write(&tone).expect("write works");
        assert!(sink.queued_frames() > 0);
        sink.pause().expect("pause works");
        sink.resume().expect("resume works");
        sink.flush().expect("flush works");
        assert_eq!(sink.queued_frames(), 0);
    }
}
