//! JNI entry points for the Android app's `io.github.x0rro.xara.Native` object. They wrap the
//! same functions as the C interface, through the JNI function table directly.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;

use crate::XaraModem;

type Env = *mut *const *const c_void;
type JObject = *mut c_void;

// Slots in JNINativeInterface (jni.h).
const DELETE_LOCAL_REF: usize = 23;
const NEW_STRING_UTF: usize = 167;
const GET_STRING_UTF_CHARS: usize = 169;
const RELEASE_STRING_UTF_CHARS: usize = 170;
const GET_ARRAY_LENGTH: usize = 171;
const GET_OBJECT_ARRAY_ELEMENT: usize = 173;

/// # Safety
/// `env` must be a live JNIEnv and `F` the slot's C signature.
unsafe fn slot<F: Copy>(env: Env, i: usize) -> F {
    // SAFETY: guaranteed by the caller; every slot holds a function pointer.
    unsafe { std::mem::transmute_copy(&*(*env).add(i)) }
}

/// # Safety
/// `env` must be a live JNIEnv.
unsafe fn new_string(env: Env, s: &str) -> JObject {
    // JNI wants modified UTF-8: no NULs, and nothing outside the BMP.
    let clean: String = s.chars().map(|c| if c == '\0' || c as u32 > 0xFFFF { '?' } else { c }).collect();
    let c = CString::new(clean).unwrap_or_default();
    // SAFETY: NewStringUTF(env, const char*).
    unsafe { slot::<unsafe extern "C" fn(Env, *const c_char) -> JObject>(env, NEW_STRING_UTF)(env, c.as_ptr()) }
}

/// # Safety
/// `env` must be a live JNIEnv and `array` a String[].
unsafe fn strings(env: Env, array: JObject) -> Vec<String> {
    // SAFETY: the slots' signatures are as named in jni.h.
    unsafe {
        let len = slot::<unsafe extern "C" fn(Env, JObject) -> i32>(env, GET_ARRAY_LENGTH)(env, array);
        let element = slot::<unsafe extern "C" fn(Env, JObject, i32) -> JObject>(env, GET_OBJECT_ARRAY_ELEMENT);
        let chars = slot::<unsafe extern "C" fn(Env, JObject, *mut u8) -> *const c_char>(env, GET_STRING_UTF_CHARS);
        let release = slot::<unsafe extern "C" fn(Env, JObject, *const c_char)>(env, RELEASE_STRING_UTF_CHARS);
        let delete = slot::<unsafe extern "C" fn(Env, JObject)>(env, DELETE_LOCAL_REF);
        (0..len)
            .map(|i| {
                let s = element(env, array, i);
                let p = chars(env, s, null_mut());
                let v = CStr::from_ptr(p).to_string_lossy().into_owned();
                release(env, s, p);
                delete(env, s);
                v
            })
            .collect()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_version(env: Env, _: JObject) -> JObject {
    // SAFETY: the JVM passes a live env.
    unsafe { new_string(env, env!("CARGO_PKG_VERSION")) }
}

/// Returns a handle, or 0 when a modem is already running.
#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_start(env: Env, _: JObject, args: JObject) -> i64 {
    // SAFETY: the JVM passes a live env and the String[] the Kotlin signature declares.
    let args = unsafe { strings(env, args) };
    crate::start(args).map_or(0, |m| Box::into_raw(Box::new(m)) as i64)
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_running(_: Env, _: JObject, handle: i64) -> u8 {
    // SAFETY: handle came from start and has not been stopped.
    unsafe { (handle as *const XaraModem).as_ref() }.is_some_and(XaraModem::running).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_stop(_: Env, _: JObject, handle: i64) -> i32 {
    if handle == 0 {
        return 0;
    }
    // SAFETY: handle came from start and has not been stopped; it is freed here.
    i32::from(unsafe { Box::from_raw(handle as *mut XaraModem) }.stop())
}

/// The next piece of received data as text, or null.
#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_nextReceived(env: Env, _: JObject) -> JObject {
    // SAFETY: the JVM passes a live env.
    crate::next_received().map_or(null_mut(), |s| unsafe { new_string(env, &s) })
}

/// The next logged line, or null.
#[unsafe(no_mangle)]
pub extern "C" fn Java_io_github_x0rro_xara_Native_nextLog(env: Env, _: JObject) -> JObject {
    // SAFETY: the JVM passes a live env.
    crate::next_log().map_or(null_mut(), |l| unsafe { new_string(env, &l) })
}
