//! Output reports to a USB HID device through IOKit, for CM108-style PTT on macOS (which has no hidraw).

use std::ffi::{c_char, c_void};
use std::io;
use std::ptr::{NonNull, null};

type CfIndex = isize;

const NUMBER_SINT32: CfIndex = 3;
const UTF8: u32 = 0x0800_0100;
const REPORT_OUTPUT: u32 = 1;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFRelease(cf: *const c_void);
    fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, enc: u32) -> *const c_void;
    fn CFNumberCreate(alloc: *const c_void, kind: CfIndex, v: *const c_void) -> *const c_void;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: CfIndex,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> *const c_void;
    fn CFSetGetCount(s: *const c_void) -> CfIndex;
    fn CFSetGetValues(s: *const c_void, values: *mut *const c_void);
    fn CFRetain(cf: *const c_void) -> *const c_void;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(alloc: *const c_void, options: u32) -> *const c_void;
    fn IOHIDManagerSetDeviceMatching(m: *const c_void, matching: *const c_void);
    fn IOHIDManagerOpen(m: *const c_void, options: u32) -> i32;
    fn IOHIDManagerCopyDevices(m: *const c_void) -> *const c_void;
    fn IOHIDDeviceSetReport(d: *const c_void, kind: u32, id: CfIndex, report: *const u8, len: CfIndex) -> i32;
}

struct Cf(NonNull<c_void>);

impl Cf {
    fn new(p: *const c_void, what: &str) -> io::Result<Self> {
        NonNull::new(p.cast_mut()).map(Self).ok_or_else(|| io::Error::other(format!("{what} failed")))
    }

    fn ptr(&self) -> *const c_void {
        self.0.as_ptr()
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: we own one reference.
        unsafe { CFRelease(self.ptr()) }
    }
}

// SAFETY: the IOHIDManager and IOHIDDevice references are only used from the thread that owns them.
unsafe impl Send for Cf {}

fn number(v: i32) -> io::Result<Cf> {
    // SAFETY: v is a valid SInt32 for the duration of the call.
    Cf::new(unsafe { CFNumberCreate(null(), NUMBER_SINT32, (&raw const v).cast()) }, "CFNumberCreate")
}

fn key(s: &std::ffi::CStr) -> io::Result<Cf> {
    // SAFETY: s is NUL-terminated ASCII.
    Cf::new(unsafe { CFStringCreateWithCString(null(), s.as_ptr(), UTF8) }, "CFStringCreateWithCString")
}

/// The first HID device matching a USB vendor and product ID.
pub struct HidDevice {
    dev: Cf,
    _manager: Cf,
}

impl HidDevice {
    pub fn open(vendor: u16, product: u16) -> io::Result<Self> {
        let (kv, kp) = (key(c"VendorID")?, key(c"ProductID")?);
        let (vv, vp) = (number(vendor.into())?, number(product.into())?);
        let keys = [kv.ptr(), kp.ptr()];
        let values = [vv.ptr(), vp.ptr()];
        // SAFETY: keys and values are live CF objects; the callbacks are CoreFoundation's own.
        let matching = Cf::new(
            unsafe {
                CFDictionaryCreate(
                    null(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    2,
                    (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
                    (&raw const kCFTypeDictionaryValueCallBacks).cast(),
                )
            },
            "CFDictionaryCreate",
        )?;
        // SAFETY: plain IOKit calls on live references; the copied set and its members are retained as needed.
        unsafe {
            let manager = Cf::new(IOHIDManagerCreate(null(), 0), "IOHIDManagerCreate")?;
            IOHIDManagerSetDeviceMatching(manager.ptr(), matching.ptr());
            let r = IOHIDManagerOpen(manager.ptr(), 0);
            if r != 0 {
                return Err(io::Error::other(format!("opening HID devices: IOKit error {r:#x}")));
            }
            let none = || io::Error::other(format!("no USB HID device {vendor:04x}:{product:04x} found"));
            let set = Cf::new(IOHIDManagerCopyDevices(manager.ptr()), "IOHIDManagerCopyDevices").map_err(|_| none())?;
            let n = CFSetGetCount(set.ptr());
            if n < 1 {
                return Err(none());
            }
            let mut devs = vec![null(); n as usize];
            CFSetGetValues(set.ptr(), devs.as_mut_ptr());
            let dev = Cf::new(CFRetain(devs[0]), "CFRetain")?;
            Ok(Self { dev, _manager: manager })
        }
    }

    /// Sends an output report; `report[0]` is the report ID, as with hidraw.
    pub fn write(&mut self, report: &[u8]) -> io::Result<()> {
        let (id, data) = report.split_first().ok_or_else(|| io::Error::other("empty HID report"))?;
        // SAFETY: the device is live and data is valid for its length.
        let r = unsafe {
            IOHIDDeviceSetReport(
                self.dev.ptr(),
                REPORT_OUTPUT,
                CfIndex::from(*id),
                data.as_ptr(),
                data.len() as CfIndex,
            )
        };
        if r != 0 {
            return Err(io::Error::other(format!("HID output report: IOKit error {r:#x}")));
        }
        Ok(())
    }
}
