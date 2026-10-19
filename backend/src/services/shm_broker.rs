//! Shared Memory Broker (Spec §1.5 data plane) — `memmap2`-backed named
//! shared-memory arena holding a slot-based ring buffer readable and writable
//! across processes (default arena: **64 MB**).
//!
//! Wire/layout contract (must stay stable — Phase 4 `worker/cpp-compiler/src/shm_arena.hpp`
//! attaches to this exact layout, zero-copy):
//!
//! ```text
//! offset  size  field                              (all little-endian)
//!      0     8  magic  = b"TXCSHM\x01"
//!      8     8  layout version = 1
//!     16     8  slot_size (payload bytes per slot)
//!     24     8  slot_count
//!     32     8  push_pos  (monotonic slot index, wraps by modulo slot_count)
//!     40     8  pop_pos   (monotonic slot index)
//!     48     4  spinlock (0 = unlocked, 1 = held) — cross-process CAS
//!     52    12  reserved (zero)
//!     64  ...  slots, each: [state u32][len u32][pad u64][slot_size payload bytes]
//!          state: 0 EMPTY, 1 WRITING, 2 FULL
//! ```
//!
//! Frames never span slots, so consumers never need to reassemble across the wrap
//! boundary. The arena occupies exactly `HEADER_BYTES + slot_count * (SLOT_META_BYTES + slot_size)`
//! bytes ≤ requested capacity.

use std::fmt;
use std::fs::OpenOptions;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{fence, AtomicU32, Ordering};

use memmap2::{MmapMut, MmapOptions};

/// Default arena size required by Spec Phase 1 step 4.
pub const DEFAULT_ARENA_BYTES: usize = 64 * 1024 * 1024;
/// Default per-slot payload size for the 64 MB arena (~63 slots).
pub const DEFAULT_SLOT_SIZE: usize = 1024 * 1024;

const MAGIC_BYTES: [u8; 8] = *b"TXCSHM\x01\x00";
const MAGIC: u64 = u64::from_le_bytes(MAGIC_BYTES);
const LAYOUT_VERSION: u64 = 1;

const HEADER_BYTES: usize = 64;
const SLOT_META_BYTES: usize = 16;

const SLOT_EMPTY: u32 = 0;
const SLOT_WRITING: u32 = 1;
const SLOT_FULL: u32 = 2;

const LOCK_ATOMIC_INDEX: usize = 48 / 4; // header offset 48 → u32 index 12

/// Shared-memory descriptor names are POSIX-style (leading `/`), e.g.
/// `/texcraft_shm_buf_01` (Spec §1.4 `data.shm_descriptor` example).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShmName(pub String);

impl ShmName {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let name = if name.starts_with('/') {
            name
        } else {
            format!("/{}", name)
        };
        ShmName(name)
    }
}

impl fmt::Display for ShmName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
pub enum ShmError {
    NotFound(String),
    AlreadyExists(String),
    CapacityTooSmall(usize),
    FrameTooLarge { frame: usize, slot_size: usize },
    RingFull,
    Corrupted(String),
    Io(std::io::Error),
}

impl fmt::Display for ShmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShmError::NotFound(name) => write!(f, "shared memory region {name} not found"),
            ShmError::AlreadyExists(name) => {
                write!(f, "shared memory region {name} already exists")
            }
            ShmError::CapacityTooSmall(c) => {
                write!(f, "capacity {c} bytes too small for a shm arena")
            }
            ShmError::FrameTooLarge { frame, slot_size } => {
                write!(f, "frame of {frame} bytes exceeds slot_size {slot_size}")
            }
            ShmError::RingFull => write!(f, "shared-memory ring buffer is full"),
            ShmError::Corrupted(detail) => write!(f, "shared-memory arena corrupted: {detail}"),
            ShmError::Io(e) => write!(f, "shm io error: {e}"),
        }
    }
}

impl std::error::Error for ShmError {}

impl From<std::io::Error> for ShmError {
    fn from(e: std::io::Error) -> Self {
        ShmError::Io(e)
    }
}

/// Backing-file path resolution:
/// 1. `$TEXCRAFT_SHM_DIR/<name>` when the env var is set (test overrides, sandboxed CI);
/// 2. `/dev/shm/<name>` on Linux when writable (true POSIX `shm_open` backing);
/// 3. OS temp dir fallback (Windows and restricted systems) — still mapped via
///    `CreateFileMapping`/`MapViewOfFile`-equivalent page-backed semantics by `memmap2`.
fn shm_file_path(name: &ShmName) -> PathBuf {
    let file_name = name.0.trim_start_matches('/');
    if let Ok(dir) = std::env::var("TEXCRAFT_SHM_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir).join(file_name);
        }
    }
    let dev_shm = PathBuf::from("/dev/shm").join(file_name);
    if cfg!(target_os = "linux") && std::fs::metadata("/dev/shm").is_ok() {
        return dev_shm;
    }
    std::env::temp_dir().join(file_name)
}

/// A mapped shared-memory arena holding one slot ring. Owns the mapping only;
/// the backing name persists so other processes (`open`) can attach concurrently.
pub struct ShmRegion {
    name: ShmName,
    map: MmapMut,
    slot_size: usize,
    slot_count: usize,
}

/// Cross-process spinlock guard; releases on drop, including panic unwind.
/// Holds a raw pointer (not a reference) so exclusive borrows of the mapping
/// remain available to the surrounding push/pop body while it is alive.
struct RingGuard {
    lock: *mut AtomicU32,
}

impl Drop for RingGuard {
    fn drop(&mut self) {
        fence(Ordering::SeqCst);
        // SAFETY: pointer is derived from the arena mapping still held by the
        // region for the entire critical section during which the guard exists.
        unsafe {
            (*self.lock).store(0, Ordering::Release);
        }
    }
}

impl ShmRegion {
    /// Create a fresh named region with the **64 MB default arena**.
    pub fn create_default(name: impl Into<ShmName>) -> Result<Self, ShmError> {
        Self::create_with_slot_size(name, DEFAULT_ARENA_BYTES, DEFAULT_SLOT_SIZE)
    }

    /// Create a fresh named region with an explicit capacity.
    pub fn create(name: impl Into<ShmName>, capacity: usize) -> Result<Self, ShmError> {
        let slot_size = if capacity >= DEFAULT_ARENA_BYTES {
            DEFAULT_SLOT_SIZE
        } else {
            // Small arenas (tests, embedded CI): 64-byte payload slots keep the
            // geometry identical to production while shrinking the footprint.
            64
        };
        Self::create_with_slot_size(name, capacity, slot_size)
    }

    pub fn create_with_slot_size(
        name: impl Into<ShmName>,
        capacity: usize,
        slot_size: usize,
    ) -> Result<Self, ShmError> {
        let name = name.into();
        let slot_size = slot_size.max(64);
        if capacity < HEADER_BYTES + 2 * (SLOT_META_BYTES + slot_size) {
            return Err(ShmError::CapacityTooSmall(capacity));
        }
        let slot_count = (capacity - HEADER_BYTES) / (SLOT_META_BYTES + slot_size);
        let path = shm_file_path(&name);
        if path.exists() {
            return Err(ShmError::AlreadyExists(name.0));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.set_len((HEADER_BYTES + slot_count * (SLOT_META_BYTES + slot_size)) as u64)?;
        let map = unsafe { MmapOptions::new().map_mut(&file)? };
        let mut region = ShmRegion {
            name,
            map,
            slot_size,
            slot_count,
        };
        region.write_header();
        region.flush_all_slots_to_empty();
        Ok(region)
    }

    /// Attach as a reader/writer to an existing named region created by
    /// `create*` (possibly from another process).
    pub fn open(name: impl Into<ShmName>) -> Result<Self, ShmError> {
        let name = name.into();
        let path = shm_file_path(&name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ShmError::NotFound(name.0.clone()),
                _ => ShmError::Io(e),
            })?;
        let map = unsafe { MmapOptions::new().map_mut(&file)? };
        let header = map.deref();
        if header.len() < HEADER_BYTES {
            return Err(ShmError::Corrupted("arena shorter than header".into()));
        }
        let magic = read_u64(header, 0);
        if magic.to_le_bytes() != MAGIC_BYTES {
            return Err(ShmError::Corrupted("bad magic".into()));
        }
        if read_u64(header, 8) != LAYOUT_VERSION {
            return Err(ShmError::Corrupted("unsupported layout version".into()));
        }
        let slot_size = read_u64(header, 16) as usize;
        let slot_count = read_u64(header, 24) as usize;
        if slot_size == 0 || slot_count < 2 {
            return Err(ShmError::Corrupted("degenerate slot geometry".into()));
        }
        Ok(ShmRegion {
            name,
            map,
            slot_size,
            slot_count,
        })
    }

    pub fn name(&self) -> &ShmName {
        &self.name
    }

    /// POSIX-style descriptor surfaced in envelope `data.shm_descriptor`.
    pub fn descriptor(&self) -> String {
        self.name.0.clone()
    }

    pub fn capacity(&self) -> usize {
        HEADER_BYTES + self.slot_count * (SLOT_META_BYTES + self.slot_size)
    }

    pub fn slot_size(&self) -> usize {
        self.slot_size
    }

    pub fn slot_count(&self) -> usize {
        self.slot_count
    }

    /// Remove the backing file for a name (call once all consumers are done).
    pub fn unlink(name: &ShmName) {
        let _ = std::fs::remove_file(shm_file_path(name));
    }

    // ---- header / slot primitives -------------------------------------------

    fn write_header(&mut self) {
        let header = &mut self.map[..HEADER_BYTES];
        write_u64(header, 0, MAGIC);
        write_u64(header, 8, LAYOUT_VERSION);
        write_u64(header, 16, self.slot_size as u64);
        write_u64(header, 24, self.slot_count as u64);
        write_u64(header, 32, 0); // push_pos
        write_u64(header, 40, 0); // pop_pos
        write_u32(header, 48, 0); // spinlock
                                  // rest reserved-zero (fresh file is zero-filled)
    }

    fn flush_all_slots_to_empty(&mut self) {
        // Fresh file pages are zero-filled, but normalize defensively.
        for k in 0..self.slot_count {
            self.set_slot_state(k as u64, SLOT_EMPTY);
        }
    }

    fn acquire(&self) -> RingGuard {
        // SAFETY: offset 48 lies inside the mapped header; mapping stays alive
        // for the guard's whole lifetime (guard never outlives its method call).
        let lock = unsafe {
            let base = self.map.as_ptr() as *mut AtomicU32;
            base.add(LOCK_ATOMIC_INDEX)
        };
        loop {
            // SAFETY: same arena-backed atomic as above.
            match unsafe { (*lock).compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed) } {
                Ok(_) => break,
                Err(_) => std::hint::spin_loop(),
            }
        }
        RingGuard { lock }
    }

    fn slot_base(&self, slot: u64) -> usize {
        HEADER_BYTES + (slot as usize) * (SLOT_META_BYTES + self.slot_size)
    }

    fn read_push_pos(&self) -> u64 {
        read_u64(self.map.deref(), 32)
    }

    fn read_pop_pos(&self) -> u64 {
        read_u64(self.map.deref(), 40)
    }

    fn set_slot_state(&mut self, slot: u64, state: u32) {
        let base = self.slot_base(slot);
        write_u32(self.map.deref_mut(), base, state);
    }

    // ---- public data-plane API ----------------------------------------------

    /// Push a frame (up to `slot_size` bytes) into the ring. Fails with
    /// `ShmError::RingFull` rather than ever overwriting an unread frame —
    /// Priority Protection for the data plane.
    pub fn push(&mut self, frame: &[u8]) -> Result<(), ShmError> {
        if frame.len() > self.slot_size {
            return Err(ShmError::FrameTooLarge {
                frame: frame.len(),
                slot_size: self.slot_size,
            });
        }
        let _guard = self.acquire();
        let push_pos = self.read_push_pos();
        let pop_pos = self.read_pop_pos();
        if push_pos - pop_pos >= self.slot_count as u64 {
            return Err(ShmError::RingFull);
        }
        let slot = push_pos % self.slot_count as u64;
        let base = self.slot_base(slot);
        map_deref_mut(&mut self.map, |buf| {
            // state = WRITING first so a torn reader can never see a half frame.
            write_u32(buf, base, SLOT_WRITING);
            write_u32(buf, base + 4, frame.len() as u32);
            let payload = &mut buf[base + SLOT_META_BYTES..base + SLOT_META_BYTES + frame.len()];
            payload.copy_from_slice(frame);
        });
        fence(Ordering::SeqCst);
        self.set_slot_state(slot, SLOT_FULL);
        // publish push_pos last: readers may consume the slot only after this.
        map_deref_mut(&mut self.map, |buf| {
            write_u64(buf, 32, push_pos + 1);
        });
        fence(Ordering::SeqCst);
        Ok(())
    }

    /// Pop the oldest frame, or `Ok(None)` when the ring is empty.
    pub fn pop(&mut self) -> Result<Option<Vec<u8>>, ShmError> {
        let _guard = self.acquire();
        let push_pos = self.read_push_pos();
        let pop_pos = self.read_pop_pos();
        if pop_pos == push_pos {
            return Ok(None);
        }
        let slot = pop_pos % self.slot_count as u64;
        let base = self.slot_base(slot);
        let state = read_u32(self.map.deref(), base);
        if state != SLOT_FULL {
            return Err(ShmError::Corrupted(format!(
                "slot {slot} expected FULL, found state {state}"
            )));
        }
        let len = read_u32(self.map.deref(), base + 4) as usize;
        if len > self.slot_size {
            return Err(ShmError::Corrupted(format!(
                "slot {slot} len {len} > slot_size"
            )));
        }
        let frame = self.map[base + SLOT_META_BYTES..base + SLOT_META_BYTES + len].to_vec();
        fence(Ordering::SeqCst);
        self.set_slot_state(slot, SLOT_EMPTY);
        map_deref_mut(&mut self.map, |buf| {
            write_u64(buf, 40, pop_pos + 1);
        });
        fence(Ordering::SeqCst);
        Ok(Some(frame))
    }

    /// Frames currently queued (push_pos − pop_pos).
    pub fn len(&self) -> u64 {
        self.read_push_pos().saturating_sub(self.read_pop_pos())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for ShmRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShmRegion")
            .field("name", &self.name.0)
            .field("slot_size", &self.slot_size)
            .field("slot_count", &self.slot_count)
            .field("queued", &self.len())
            .finish()
    }
}

// ---- little-endian scalar helpers (offsets are ABI, so keep these explicit) ----

fn read_u64(buf: &[u8], off: usize) -> u64 {
    let bytes = buf[off..off + 8].try_into().expect("checked offsets");
    u64::from_le_bytes(bytes)
}

fn write_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    let bytes = buf[off..off + 4].try_into().expect("checked offsets");
    u32::from_le_bytes(bytes)
}

fn write_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn map_deref_mut<R>(map: &mut MmapMut, f: impl FnOnce(&mut [u8]) -> R) -> R {
    f(map.deref_mut())
}

#[cfg(test)]
mod tests {
    use super::{
        ShmError, ShmName, ShmRegion, DEFAULT_ARENA_BYTES, DEFAULT_SLOT_SIZE, HEADER_BYTES,
        SLOT_META_BYTES,
    };

    fn test_name(tag: &str) -> ShmName {
        ShmName::new(format!("/texcraft_test_shm_{}_{}", tag, std::process::id()))
    }

    #[test]
    fn default_arena_is_64_mib() {
        assert_eq!(DEFAULT_ARENA_BYTES, 64 * 1024 * 1024);
        let name = test_name("default");
        let region = ShmRegion::create_default(name.clone()).unwrap();
        // Working arena must fit inside the requested 64 MiB with 1 MiB slots.
        assert!(region.capacity() <= DEFAULT_ARENA_BYTES);
        assert_eq!(region.slot_size(), DEFAULT_SLOT_SIZE);
        assert_eq!(
            region.slot_count(),
            (DEFAULT_ARENA_BYTES - HEADER_BYTES) / (SLOT_META_BYTES + DEFAULT_SLOT_SIZE)
        );
        assert_eq!(region.descriptor(), name.0);
        ShmRegion::unlink(&name);
    }

    #[test]
    fn write_read_roundtrip_single_handle() {
        let name = test_name("roundtrip");
        let mut region = ShmRegion::create(name.clone(), 4096).unwrap();
        region.push(b"alpha").unwrap();
        region.push(b"beta-token").unwrap();
        region.push(&[7u8; 64]).unwrap(); // exactly slot_size
        assert_eq!(region.len(), 3);
        assert_eq!(region.pop().unwrap().unwrap(), b"alpha");
        assert_eq!(region.pop().unwrap().unwrap(), b"beta-token");
        assert_eq!(region.pop().unwrap().unwrap(), vec![7u8; 64]);
        assert_eq!(region.pop().unwrap(), None);
        ShmRegion::unlink(&name);
    }

    #[test]
    fn cross_handle_roundtrip_matches_open_attach() {
        let name = test_name("crosshandle");
        {
            let mut producer = ShmRegion::create(name.clone(), 8192).unwrap();
            producer.push(b"frame-from-producer-process").unwrap();
            producer.push(b"second").unwrap();
            // `producer` drops without unlinking; descriptor stays attachable.
        }
        let mut consumer = ShmRegion::open(name.clone()).unwrap();
        assert_eq!(
            consumer.pop().unwrap().unwrap(),
            b"frame-from-producer-process"
        );
        assert_eq!(consumer.pop().unwrap().unwrap(), b"second");
        assert!(consumer.pop().unwrap().is_none());
        ShmRegion::unlink(&name);
    }

    #[test]
    fn ring_wraps_many_laps_preserving_fifo_order() {
        let name = test_name("wrap");
        let mut region = ShmRegion::create(name.clone(), 4096).unwrap();
        let slot_count = region.slot_count() as u64;
        assert!(slot_count >= 2);

        // Push/pop `(slot_count + 7)` frames: forces two full wraps with a
        // visible offset, verifying modulo rotation and FIFO survival.
        for i in 0..(slot_count + 7) {
            let payload = format!("f{i:06}");
            // Interleave occasional reads to stress partial-occupancy wraps.
            if i >= 3 {
                let _ = region.pop().unwrap().unwrap();
            }
            region.push(payload.as_bytes()).unwrap();
        }
        // Drain whatever is left in queue order and check contiguity.
        let mut drained = Vec::new();
        while let Some(frame) = region.pop().unwrap() {
            drained.push(String::from_utf8(frame).unwrap());
        }
        assert!(!drained.is_empty());
        for pair in drained.windows(2) {
            let na: u64 = pair[0].trim_start_matches('f').parse().unwrap();
            let nb: u64 = pair[1].trim_start_matches('f').parse().unwrap();
            assert_eq!(nb, na + 1, "FIFO order must survive wrap");
        }
        ShmRegion::unlink(&name);
    }

    #[test]
    fn push_fails_closed_when_ring_full() {
        let name = test_name("full");
        let mut region = ShmRegion::create(name.clone(), 4096).unwrap();
        let slot_count = region.slot_count();
        for i in 0..slot_count {
            region.push(&[i as u8]).unwrap();
        }
        assert!(matches!(region.push(b"overflow"), Err(ShmError::RingFull)));
        // Drain the oldest frame, then push — it wraps into the freed slot.
        assert_eq!(region.pop().unwrap().unwrap(), vec![0u8]);
        region.push(b"fits-after-drain").unwrap();
        // FIFO: the successor frames [1..slot_count) ride ahead of the newcomer.
        for i in 1..slot_count {
            assert_eq!(region.pop().unwrap().unwrap(), vec![i as u8]);
        }
        assert_eq!(region.pop().unwrap().unwrap(), b"fits-after-drain");
        assert!(region.pop().unwrap().is_none());
        ShmRegion::unlink(&name);
    }

    #[test]
    fn oversized_frames_rejected_before_mapping_writes() {
        let name = test_name("oversize");
        let mut region = ShmRegion::create(name.clone(), 4096).unwrap();
        let err = region
            .push(vec![0u8; region.slot_size() + 1].as_slice())
            .unwrap_err();
        assert!(matches!(err, ShmError::FrameTooLarge { .. }));
        assert!(region.is_empty());
        ShmRegion::unlink(&name);
    }

    #[test]
    fn double_create_fails_and_open_missing_fails() {
        let name = test_name("conflict");
        let _region = ShmRegion::create(name.clone(), 4096).unwrap();
        assert!(matches!(
            ShmRegion::create(name.clone(), 4096),
            Err(ShmError::AlreadyExists(_))
        ));
        ShmRegion::unlink(&name);
        let missing = test_name("missing");
        let _ = ShmRegion::unlink(&missing); // ensure absent
        assert!(matches!(
            ShmRegion::open(missing),
            Err(ShmError::NotFound(_))
        ));
    }

    #[test]
    fn interleaved_push_pop_never_hits_ring_full() {
        let name = test_name("interleave");
        let mut region = ShmRegion::create(name.clone(), 8192).unwrap();
        let slot_count = region.slot_count();
        // Long push/pop interleave: ledger keeps occupancy at 1 after the first
        // frame; validates guard release across every early/regular path.
        for i in 0..(slot_count * 4) {
            region.push(i.to_le_bytes().as_slice()).unwrap();
            if i > 0 {
                let frame = region.pop().unwrap().unwrap();
                assert_eq!(frame, (i - 1).to_le_bytes());
            }
        }
        assert_eq!(region.len(), 1);
        ShmRegion::unlink(&name);
    }
}
