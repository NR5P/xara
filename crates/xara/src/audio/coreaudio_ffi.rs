use std::ffi::{CString, c_char, c_void};
use std::io;
use std::ptr::{NonNull, null, null_mut};

type OsStatus = i32;
type ObjectId = u32;
type CfIndex = isize;
pub type QueueRef = *mut c_void;

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const SYSTEM_OBJECT: ObjectId = 1;
const HARDWARE_DEVICES: u32 = fourcc(b"dev#");
const DEVICE_UID: u32 = fourcc(b"uid ");
const OBJECT_NAME: u32 = fourcc(b"lnam");
const DEVICE_STREAMS: u32 = fourcc(b"stm#");
const DEVICE_TRANSPORT: u32 = fourcc(b"tran");
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_INPUT: u32 = fourcc(b"inpt");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const ELEMENT_MAIN: u32 = 0;
pub const TRANSPORT_VIRTUAL: u32 = fourcc(b"virt");
pub const TRANSPORT_AGGREGATE: u32 = fourcc(b"grup");

const FORMAT_LINEAR_PCM: u32 = fourcc(b"lpcm");
const PCM_SIGNED_INTEGER: u32 = 1 << 2;
const PCM_PACKED: u32 = 1 << 3;
const QUEUE_CURRENT_DEVICE: u32 = fourcc(b"aqcd");
const UTF8: u32 = 0x0800_0100;

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
pub struct StreamFormat {
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

impl StreamFormat {
    pub fn s16_mono(rate: u32) -> Self {
        Self {
            sample_rate: f64::from(rate),
            format_id: FORMAT_LINEAR_PCM,
            format_flags: PCM_SIGNED_INTEGER | PCM_PACKED,
            bytes_per_packet: 2,
            frames_per_packet: 1,
            bytes_per_frame: 2,
            channels_per_frame: 1,
            bits_per_channel: 16,
            reserved: 0,
        }
    }
}

#[repr(C)]
pub struct QueueBuffer {
    pub capacity: u32,
    pub data: *mut c_void,
    pub size: u32,
    _user: *mut c_void,
    _desc_capacity: u32,
    _descs: *mut c_void,
    _desc_count: u32,
}

pub type InputCb = extern "C" fn(*mut c_void, QueueRef, *mut QueueBuffer, *const c_void, u32, *const c_void);
pub type OutputCb = extern "C" fn(*mut c_void, QueueRef, *mut QueueBuffer);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, enc: u32) -> *const c_void;
    fn CFStringGetLength(s: *const c_void) -> CfIndex;
    fn CFStringGetMaximumSizeForEncoding(len: CfIndex, enc: u32) -> CfIndex;
    fn CFStringGetCString(s: *const c_void, buf: *mut c_char, size: CfIndex, enc: u32) -> u8;
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        id: ObjectId,
        addr: *const PropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        size: *mut u32,
    ) -> OsStatus;
    fn AudioObjectGetPropertyData(
        id: ObjectId,
        addr: *const PropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> OsStatus;
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewInput(
        format: *const StreamFormat,
        cb: InputCb,
        user: *mut c_void,
        run_loop: *const c_void,
        mode: *const c_void,
        flags: u32,
        out: *mut QueueRef,
    ) -> OsStatus;
    fn AudioQueueNewOutput(
        format: *const StreamFormat,
        cb: OutputCb,
        user: *mut c_void,
        run_loop: *const c_void,
        mode: *const c_void,
        flags: u32,
        out: *mut QueueRef,
    ) -> OsStatus;
    fn AudioQueueAllocateBuffer(q: QueueRef, bytes: u32, out: *mut *mut QueueBuffer) -> OsStatus;
    fn AudioQueueEnqueueBuffer(q: QueueRef, b: *mut QueueBuffer, ndesc: u32, descs: *const c_void) -> OsStatus;
    fn AudioQueueSetProperty(q: QueueRef, id: u32, data: *const c_void, size: u32) -> OsStatus;
    fn AudioQueueStart(q: QueueRef, at: *const c_void) -> OsStatus;
    fn AudioQueueDispose(q: QueueRef, immediate: u8) -> OsStatus;
}

fn status(what: &str, s: OsStatus) -> io::Result<()> {
    if s == 0 {
        return Ok(());
    }
    let b = s.to_be_bytes();
    let code = if b.iter().all(|c| c.is_ascii_graphic()) {
        format!("'{}'", String::from_utf8_lossy(&b))
    } else {
        s.to_string()
    };
    Err(io::Error::other(format!("{what}: CoreAudio error {code}")))
}

struct CfString(NonNull<c_void>);

impl Drop for CfString {
    fn drop(&mut self) {
        // SAFETY: we own one reference to this CFString.
        unsafe { CFRelease(self.0.as_ptr()) }
    }
}

impl CfString {
    fn new(s: &str) -> io::Result<Self> {
        let c = CString::new(s).map_err(|_| io::Error::other(format!("{s:?}: contains a NUL byte")))?;
        // SAFETY: c is a valid NUL-terminated UTF-8 string; a null allocator means the default one.
        let p = unsafe { CFStringCreateWithCString(null(), c.as_ptr(), UTF8) };
        NonNull::new(p.cast_mut()).map(Self).ok_or_else(|| io::Error::other("CFStringCreateWithCString failed"))
    }

    fn string(&self) -> Option<String> {
        // SAFETY: self.0 is a live CFString and buf holds size bytes.
        unsafe {
            let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(self.0.as_ptr()), UTF8) + 1;
            let mut buf = vec![0u8; usize::try_from(size).ok()?];
            if CFStringGetCString(self.0.as_ptr(), buf.as_mut_ptr().cast(), size, UTF8) == 0 {
                return None;
            }
            let n = buf.iter().position(|&b| b == 0)?;
            buf.truncate(n);
            String::from_utf8(buf).ok()
        }
    }
}

fn address(selector: u32, scope: u32) -> PropertyAddress {
    PropertyAddress { selector, scope, element: ELEMENT_MAIN }
}

fn property_size(id: ObjectId, selector: u32, scope: u32) -> io::Result<u32> {
    let mut size = 0;
    // SAFETY: the address is valid for the call and size is writable.
    status("AudioObjectGetPropertyDataSize", unsafe {
        AudioObjectGetPropertyDataSize(id, &address(selector, scope), 0, null(), &mut size)
    })?;
    Ok(size)
}

fn property<T: Copy>(id: ObjectId, selector: u32, scope: u32, zero: T) -> io::Result<T> {
    let mut v = zero;
    let mut size = size_of::<T>() as u32;
    // SAFETY: v has room for size bytes of a plain-data property value.
    status("AudioObjectGetPropertyData", unsafe {
        AudioObjectGetPropertyData(id, &address(selector, scope), 0, null(), &mut size, (&raw mut v).cast())
    })?;
    Ok(v)
}

fn string_property(id: ObjectId, selector: u32) -> io::Result<String> {
    let p: *const c_void = property(id, selector, SCOPE_GLOBAL, null())?;
    let s = NonNull::new(p.cast_mut()).map(CfString).ok_or_else(|| io::Error::other("CoreAudio returned no string"))?;
    s.string().ok_or_else(|| io::Error::other("CoreAudio returned a string that is not UTF-8"))
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub uid: String,
    pub name: String,
    pub inputs: bool,
    pub outputs: bool,
    pub transport: u32,
}

pub fn devices() -> io::Result<Vec<DeviceInfo>> {
    let size = property_size(SYSTEM_OBJECT, HARDWARE_DEVICES, SCOPE_GLOBAL)?;
    let mut ids = vec![0 as ObjectId; size as usize / size_of::<ObjectId>()];
    let mut got = (ids.len() * size_of::<ObjectId>()) as u32;
    // SAFETY: ids has room for got bytes.
    status("listing audio devices", unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address(HARDWARE_DEVICES, SCOPE_GLOBAL),
            0,
            null(),
            &mut got,
            ids.as_mut_ptr().cast(),
        )
    })?;
    ids.truncate(got as usize / size_of::<ObjectId>());
    let mut v = Vec::new();
    for id in ids {
        let Ok(uid) = string_property(id, DEVICE_UID) else { continue };
        v.push(DeviceInfo {
            name: string_property(id, OBJECT_NAME).unwrap_or_default(),
            inputs: property_size(id, DEVICE_STREAMS, SCOPE_INPUT).is_ok_and(|n| n > 0),
            outputs: property_size(id, DEVICE_STREAMS, SCOPE_OUTPUT).is_ok_and(|n| n > 0),
            transport: property(id, DEVICE_TRANSPORT, SCOPE_GLOBAL, 0u32).unwrap_or(0),
            uid,
        });
    }
    Ok(v)
}

/// An audio queue bound to one device, delivering callbacks on CoreAudio's own thread.
pub struct Queue(QueueRef);

// SAFETY: AudioQueue references may be used from any thread.
unsafe impl Send for Queue {}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: immediate disposal returns only once no callback is running or will run.
        unsafe { AudioQueueDispose(self.0, 1) };
    }
}

impl Queue {
    /// # Safety
    /// `user` must stay valid until the queue is dropped.
    pub unsafe fn input(format: &StreamFormat, cb: InputCb, user: *mut c_void) -> io::Result<Self> {
        let mut q = null_mut();
        // SAFETY: null run loop means CoreAudio's internal thread; the caller keeps user alive.
        status("AudioQueueNewInput", unsafe { AudioQueueNewInput(format, cb, user, null(), null(), 0, &mut q) })?;
        Ok(Self(q))
    }

    /// # Safety
    /// `user` must stay valid until the queue is dropped.
    pub unsafe fn output(format: &StreamFormat, cb: OutputCb, user: *mut c_void) -> io::Result<Self> {
        let mut q = null_mut();
        // SAFETY: as for input.
        status("AudioQueueNewOutput", unsafe { AudioQueueNewOutput(format, cb, user, null(), null(), 0, &mut q) })?;
        Ok(Self(q))
    }

    pub fn set_device(&self, uid: &str) -> io::Result<()> {
        let s = CfString::new(uid)?;
        let p = s.0.as_ptr().cast_const();
        // SAFETY: the property value is a CFStringRef, which the queue retains.
        status(&format!("selecting {uid:?}"), unsafe {
            AudioQueueSetProperty(self.0, QUEUE_CURRENT_DEVICE, (&raw const p).cast(), size_of_val(&p) as u32)
        })
    }

    pub fn allocate(&self, bytes: u32) -> io::Result<*mut QueueBuffer> {
        let mut b = null_mut();
        // SAFETY: the queue is live; the buffer is owned by it and freed on dispose.
        status("AudioQueueAllocateBuffer", unsafe { AudioQueueAllocateBuffer(self.0, bytes, &mut b) })?;
        Ok(b)
    }

    pub fn start(&self) -> io::Result<()> {
        // SAFETY: the queue is live; a null start time means now.
        status("AudioQueueStart", unsafe { AudioQueueStart(self.0, null()) })
    }

    pub fn raw(&self) -> QueueRef {
        self.0
    }
}

/// # Safety
/// `q` and `b` must be a live queue and one of its buffers.
pub unsafe fn enqueue(q: QueueRef, b: *mut QueueBuffer) -> io::Result<()> {
    // SAFETY: guaranteed by the caller; PCM needs no packet descriptions.
    status("AudioQueueEnqueueBuffer", unsafe { AudioQueueEnqueueBuffer(q, b, 0, null()) })
}
