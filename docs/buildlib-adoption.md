# Buildlib adoption: FFmpeg runtime acquisition

This is the BLD-2 pilot record for `play`. It covers the verified acquisition in
`scripts/fetch-ffmpeg-runtime.sh`; it does not change release packaging, the runtime
contract, the application layout or any public Make target.

## 1. Bound change

- **Base revision.** `c3bcf7b7fa7a20847738451525dd9e129af7eb95`.
- **Entrypoint.** `scripts/fetch-ffmpeg-runtime.sh TARGET [ARCHIVE]`. The usage text,
  the two-argument limit, exit code 2 for a bad argument count, and the success and
  error wording are unchanged. `scripts/build.sh`, `scripts/release.sh`,
  `scripts/build-linux.sh` and the documentation still call the same path.
- **Local implementation replaced.** The inline `pin_field` Python heredoc, the shell
  `sha256` function, and the cache/`curl`/`.part`/`mv`/digest block in the base script
  (originally lines 18-40 and 70-108). The old path validated the digest only after
  publishing the download to the cache, then deleted a mismatched file; an unsupported
  target surfaced as a missing digest.
- **Buildlib API.** `buildlib.locks.Input`, `locks.verify_file`, `locks.fetch_input`
  and `buildlib.targets.target_for_triple`, imported from the pinned wheel by
  `scripts/fetch_ffmpeg_runtime.py` through `bootstrap/buildlib-run`.
- **Supported targets.** Buildlib's set: `aarch64-apple-darwin`,
  `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`. `riscv64-unknown-linux-musl`
  now fails with `target not supported` rather than an incidental missing-digest error.
- **Output and error contracts.** The archive is still published atomically under
  `.work/tmp/ffmpeg-runtime`, extracted through a fresh staging directory, checked for
  exactly one top-level directory named after the archive, validated against the pin,
  and moved to `.work/deps/ffmpeg-runtime/<target>`. A prefix whose manifest already
  matches is reused. Failures print `error: ...` to standard error and exit 1.
- **Consumer policy that stays local.** The lock contract (`contract` 2, profile
  `runtime`), the `release-<40 hex commit>` tag shape, the archive name, the FFmpeg
  version, the optional per-target `size`, the `libraries` list, every manifest field
  and the installed layout are Player's; buildlib does not validate them. The FFmpeg
  profile/library audit in `scripts/ffmpeg-manifest.sh` is untouched.

## 2. Pins and provenance

Vendored from `laputa-systems/buildlib` at
`30ad4a0a8b697031a008b1de3b5359fc5cf6f2fe` (release 0.2.0), both files byte-identical to
that revision:

| File | SHA-256 |
| --- | --- |
| `bootstrap/buildlib-run` | `e7f957ca6af0b151d25a9ff5bedb5deca0bfc86ba05ff3225a5405d56fa95957` |
| `bootstrap/python.lock` | `cab50541ce8b2c2a6a41829f26aac19344532d239535f4d6795d80d5af933832` |

`bootstrap/python.lock` pins the interpreter archives, uv 0.13.7 with Python 3.15.2, and
wheel `laputa_buildlib-0.2.0-py3-none-any.whl` with SHA-256
`aaee620f0478544b1d32075b44de48e69bbcbd8b974d5f599d037494d59f4b4a`. The interpreter on
macOS arm64 is `cpython-3.15.1-aarch64-apple-darwin.tar.xz` (7,382,296 bytes,
`81b336d7f1813fac0803316c150b67de295880c4eebb22755fb1e186f5bd5ef2`).

`scripts/fetch_ffmpeg_runtime.py` refuses to run when `buildlib.__file__` is not under a
`.whl` path, so a checkout or an owner `PYTHONPATH` cannot stand in for the pinned wheel.
The import check printed:

```
$ BUILDLIB_CACHE=.work/tmp/buildlib-cache-probe bootstrap/buildlib-run -- \
    -c 'import buildlib; print(buildlib.__file__)'
.../.work/tmp/buildlib-cache-probe/aaee620f0478544b1d32075b44de48e69bbcbd8b974d5f599d037494d59f4b4a/buildlib.whl/buildlib/__init__.py
```

The source revision is recorded separately from the package version. There are no local
differences in either vendored file. Reverting this slice restores the base script and
leaves every pin untouched; no release or lock format changed.

## 3. Boundary tests

`tests/test_ffmpeg_fetch.py` runs the real wrapper boundary through
`bootstrap/buildlib-run` (10 tests, `python3 -m unittest tests.test_ffmpeg_fetch -v`,
exit 0, `Ran 10 tests ... OK`). The injected faults each fail before installation:

| Fault | Injected by | Observed error |
| --- | --- | --- |
| wrong digest | pin digest zeroed, valid archive | `error: ... sha256 <hex> != pinned 000...` |
| wrong size | pin `size` one byte over the archive | `error: ... size 3122496 != pinned 3122497` |
| truncated download | HTTPS mirror behind a CONNECT proxy serves half the archive | `error: ... no URL served the pinned bytes: ... sha256 ...` |
| wrong target | `riscv64-unknown-linux-musl` | `error: target not supported: 'riscv64-unknown-linux-musl' (supported: [...])` |
| truncated local archive | half the archive, original digest | `error: ... sha256 ... != pinned ...` |
| manifest disagreement | archive target differs from the pin | `error: ... disagrees with ffmpeg-runtime.lock.json: ...` |
| wrong archive name | local archive for another product version | `error: ... is not the pinned archive ...` |

The suite also proves cache reuse without network, reuse of an installed prefix without
a cache, and the wrapper's preserved usage exit code. Existing checks kept their
coverage: the manifest profile, target, FFmpeg version, producer commit, archive name
and library set, the exactly-one-directory extraction, and the existing
`tests/run.sh ffmpeg-audit` contract are unchanged. Buildlib's own tests cover the
download internals; this suite covers Player's wiring.

## 4. Outcome comparison

Both scripts were run cold for `aarch64-apple-darwin`, the old one from a copy of the
base script in a scratch root with the same pin:

| Measurement | Base script | New script |
| --- | --- | --- |
| Archive SHA-256 | `380471e07416eb636836d8428e8b2c6bf8052b8df030cff85e4008ccab70369d` | same |
| Runtime tree, 200 entries (type, mode, size, content, symlink) | `4a4d2af1b7c5eb4e267308a827501c84f06a5e1d8f15bd08985bb81a3fbd970a` | same |
| `diff -r` | identical | identical |

The tree hash covers every entry's relative path, type, permission bits, byte size and
content digest (or symlink target). The installed layout is byte-identical, so packaging
and linkage inputs do not change.

## 5. Environment

| Environment | Commands | Result |
| --- | --- | --- |
| macOS 27.2 arm64 (native) | `TMPDIR=.work/tmp make lint` | exit 0 |
| macOS 27.2 arm64 (native) | `TMPDIR=.work/tmp make test` | exit 2: `TOTAL 493 passed, 2 failed`; both failures are the pre-existing PLAY-55 fixture byte checks, not this slice |
| macOS 27.2 arm64 (native) | `TMPDIR=.work/tmp tests/run.sh ffmpeg-audit` | exit 0, 16 passed |
| macOS 27.2 arm64 (native) | `python3 -m unittest tests.test_ffmpeg_fetch -v` | exit 0, 10 tests |
| `ubuntu:24.04`, amd64, cold and emulated | setup, acquisition, `make linux`, `make test`, `make lint` | all exit 0; bootstrap took `uv`; suite 464 passed, 0 failed |
| `ubuntu:26.04` + `musl` 1.2.5, arm64, cold and native | setup, acquisition, `make linux`, `make test`, `make lint` | all exit 0; bootstrap took `python-build`; suite 464 passed, 0 failed |

**macOS red, pre-existing.** `make test` fails only
`fixtures: generated manifest matches the tracked manifest` and `fixtures: 30 file hashes`
(`corrupt`, `hevc`, `mpeg4`). `tests/fixtures/gen.sh` uses the host's Homebrew libx265
and libmp3lame; this host has x265 4.3 and lame 4.0, while the tracked manifest records
the Nov 5 output. The same failure occurred at base `a4d901f`.
This slice does not edit `tests/fixtures/gen.sh`, `tests/fixtures/manifest.json` or
`gate_fixtures`; every other macOS gate, including `rust-link`, `release`, `unit`, `meta`
file presence and `ffmpeg-audit`, passes. Re-baselining the manifest alone is explicitly
not the fix (PLAY-55), and weakening the byte check was out of scope, so macOS
`make test` still exits nonzero on this host.

**Capsules.** Each capsule mounted the worktree read-only (plus the linked worktree's
gitdir), cloned it inside, and ran as an unprivileged user with `dockerd` on `vfs`.
Cold clone of `91940ca64020ba58c2dbb68178ec14481746d266`; no scratch ref was pushed.
The only later commit is this document, which the gates do not execute.

| Capsule | Bootstrap branch | acquire | `make linux` | `make test` | `make lint` | `git status` | Cold wall time |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ubuntu:24.04`, amd64 under Rosetta | `uv` (0.13.7, CPython 3.15.2) | 0 (5 s) | 0 (5 m 48 s) | 0, 464 passed | 0 (1 m 27 s) | empty | 13 m 33 s |
| `ubuntu:26.04` + `musl`, arm64 native | `python-build` (CPython 3.15.1 aarch64) | 0 (4 s) | 0 (5 m 46 s) | 0, 464 passed | 0 (45 s) | empty | 15 m 1 s |

The 26.04 capsule ran the repository's amd64 Linux gate in nested Docker
(`--platform linux/amd64` reported `x86_64`) while its own acquisition used the native
aarch64 interpreter and fetched the `x86_64-unknown-linux-musl` runtime. The 24.04
capsule is emulated throughout. Cold setup downloaded about 59.6 MB on 24.04 (uv
16,671,812 B, uv-managed CPython 36.4 MiB as reported, wheel 25,193 B, FFmpeg
4,735,840 B) and 13,186,781 B on 26.04 (python-build 8,425,748 B, wheel 25,193 B,
FFmpeg 4,735,840 B). These durations are operational cold-capsule cost, not benchmarks.

One environment limit is recorded rather than hidden: an amd64-emulated `ubuntu:26.04`
capsule could not run the pinned `python-build` runtime on this host. GNU tar 1.35 returns
`ENOSYS` for extraction under Rosetta, and the pinned rustls TLS path takes an illegal
instruction during the HTTPS acquisition. The arm64 capsule is the qualified python-build
path here; the amd64-musl `python-build` path under emulation is not proven. The uv path
on emulated amd64 is proven. macOS signing and application execution are unaffected and
were not re-qualified by the Linux runs.

## 6. Benefit and cost

- **Duplicate code removed.** The acquisition shell shrank from 127 lines to an
  11-line wrapper. The deleted code implemented SHA-256 on two platforms, retry
  downloads, a `.part` rename and a post-hoc digest check. The 189-line helper keeps
  only Player policy, extraction and layout.
- **Shared operations used.** Verified fetch with per-URL fallback, streamed
  digest/size comparison, atomic publish, cache verification and supported-target
  mapping.
- **Correctness gain.** A download is published only after the pinned size and digest
  match, so an interrupted or corrupt file never enters the cache; the old script
  published first and deleted on mismatch. An unsupported target now has a direct
  error. The optional `size` pin can reject a wrong-sized archive before reading it.
- **Cold setup.** The first acquisition on macOS downloaded 7,382,296 bytes of Python,
  25,193 bytes of wheel and 3,122,496 bytes of FFmpeg archive: 10,529,985 bytes, with a
  41 MiB cache after extraction (`.work/tmp/buildlib-cache-cold`). Wall time was 3.46 s
  against 0.91 s for the base script, because the base used the system Python. Later
  runs reuse both caches.
- **Warm no-op.** 0.19/0.13/0.13 s over three runs versus 0.12/0.12/0.13 s for the base
  script. The pinned interpreter is already unpacked; the extra exec and import cost is
  at most about 60 ms and does not appear in `make lint`, which never fetches.
- **Not justified / not done.** No release manifest, archive or `SHA256SUMS` helper was
  imported: `play` consumes a runtime release, it does not produce one.
