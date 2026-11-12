use std::ffi::c_ulong;

unsafe extern "C" {
    fn compress2(
        dest: *mut u8,
        dest_len: *mut c_ulong,
        source: *const u8,
        source_len: c_ulong,
        level: i32,
    ) -> i32;
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("screenshot has no size".to_owned());
    }
    let stride = width as usize * 4;
    if rgba.len() != stride * height as usize {
        return Err("screenshot pixels do not match its size".to_owned());
    }
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks_exact(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut compressed = vec![0u8; raw.len() + raw.len() / 8 + 1024];
    let mut compressed_len = compressed.len() as c_ulong;
    // SAFETY: both buffers are live, and compressed_len describes the
    // writable destination capacity.
    let status = unsafe {
        compress2(
            compressed.as_mut_ptr(),
            &mut compressed_len,
            raw.as_ptr(),
            raw.len() as c_ulong,
            6,
        )
    };
    if status != 0 {
        return Err(format!("zlib compress2 failed with {status}"));
    }
    compressed.truncate(compressed_len as usize);

    let mut png = Vec::new();
    png.extend_from_slice(&SIGNATURE);
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &header);
    write_chunk(&mut png, b"IDAT", &compressed);
    write_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn write_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = Crc::new();
    crc.update(kind);
    crc.update(data);
    png.extend_from_slice(&crc.finish().to_be_bytes());
}

struct Crc {
    value: u32,
}

impl Crc {
    fn new() -> Self {
        Crc { value: 0xffff_ffff }
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.value ^= *byte as u32;
            for _ in 0..8 {
                let mask = (self.value & 1).wrapping_neg();
                self.value = (self.value >> 1) ^ (0xedb8_8320 & mask);
            }
        }
    }

    fn finish(self) -> u32 {
        !self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_png_header_and_payload() {
        let pixels = [0x20, 0x40, 0x60, 0xff].repeat(16);
        let png = encode_png(&pixels, 4, 4).expect("encode");
        assert!(png.starts_with(&SIGNATURE));
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &4u32.to_be_bytes());
        assert_eq!(&png[20..24], &4u32.to_be_bytes());
        assert!(png.windows(4).any(|window| window == b"IEND"));
    }

    #[test]
    fn rejects_mismatched_pixels() {
        assert!(encode_png(&[0; 3], 2, 2).is_err());
    }
}


pub(crate) fn write_automatic(
    directory: &std::path::Path,
    index: &mut u32,
    png: &[u8],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    loop {
        *index = index.checked_add(1).ok_or("screenshot names exhausted")?;
        let target = directory.join(format!("shot{:04}.png", index));
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot create {}: {error}", target.display())),
        };
        file.write_all(png)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        return Ok(target);
    }
}

#[cfg(test)]
mod automatic_tests {
    use super::*;

    #[test]
    fn automatic_shots_preserve_files_and_competing_creators() {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../.work/tmp/shots-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("shot0001.png"), b"original").unwrap();
        std::os::unix::fs::symlink("shot0001.png", directory.join("shot0002.png")).unwrap();
        let barrier = std::sync::Barrier::new(8);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8u8).map(|value| {
                let directory = &directory;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let path = write_automatic(directory, &mut 0, &[value]).unwrap();
                    (path, value)
                })
            }).collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(std::fs::read(directory.join("shot0001.png")).unwrap(), b"original");
        for (path, value) in results { assert_eq!(std::fs::read(path).unwrap(), [value]); }
        assert!(write_automatic(&directory.join("missing"), &mut 0, b"fail").is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
