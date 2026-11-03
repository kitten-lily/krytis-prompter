//! `sx-aes-1`, the prompter side, through libgcr-4's `GcrSecretExchange`.
//!
//! No crypto lives in this crate: gcr does the Diffie-Hellman, the key
//! derivation, AES and the payload format, exactly as it does for gnome-shell's
//! prompter. The prompter side needs four calls, all plain C with no callbacks:
//! `new`, `begin` (the first `PromptReady`), `receive` (each `PerformPrompt`)
//! and `send` (the answer). See `gcr/gcr-system-prompter.c`, which this mirrors.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use gtk::glib::{ffi::g_free, gobject_ffi::g_object_unref};

type GcrSecretExchange = c_void;

#[link(name = "gcr-4")]
unsafe extern "C" {
    fn gcr_secret_exchange_new(protocol: *const c_char) -> *mut GcrSecretExchange;
    fn gcr_secret_exchange_begin(this: *mut GcrSecretExchange) -> *mut c_char;
    fn gcr_secret_exchange_receive(this: *mut GcrSecretExchange, exchange: *const c_char) -> c_int;
    fn gcr_secret_exchange_send(this: *mut GcrSecretExchange, secret: *const c_char, len: isize) -> *mut c_char;
}

/// One exchange per prompting session: the same key pair answers every prompt
/// in it, which both gcr's client and oo7 rely on.
pub struct Exchange(*mut GcrSecretExchange);

impl Exchange {
    pub fn new() -> Self {
        // A NULL protocol is gcr's default, sx-aes-1.
        // SAFETY: plain constructor; returns a new reference or aborts on OOM.
        let ptr = unsafe { gcr_secret_exchange_new(std::ptr::null()) };
        assert!(!ptr.is_null(), "gcr_secret_exchange_new returned NULL");
        Self(ptr)
    }

    /// The payload for the first `PromptReady`: our public key.
    pub fn begin(&self) -> String {
        // SAFETY: self.0 is a live GcrSecretExchange; the result is transfer-full.
        unsafe { take_string(gcr_secret_exchange_begin(self.0)) }
    }

    /// Takes the caller's payload from `PerformPrompt`. False if it doesn't parse
    /// or carries no usable public key.
    pub fn receive(&mut self, exchange: &str) -> bool {
        let Ok(c) = CString::new(exchange) else { return false };
        // SAFETY: self.0 is live; gcr copies what it keeps from the string.
        unsafe { gcr_secret_exchange_receive(self.0, c.as_ptr()) != 0 }
    }

    /// The payload for the answering `PromptReady`. `None` yields gcr's
    /// public-key-only payload, which is what gcr's prompter sends for a
    /// cancelled or confirm prompt.
    pub fn send(&self, secret: Option<&[u8]>) -> String {
        let (ptr, len) = match secret {
            Some(s) => (s.as_ptr().cast(), isize::try_from(s.len()).expect("secret length fits isize")),
            None => (std::ptr::null(), -1),
        };
        // SAFETY: self.0 is live; gcr reads `len` bytes of `ptr` (or nothing for
        // NULL) and copies them into its own secure memory before returning.
        unsafe { take_string(gcr_secret_exchange_send(self.0, ptr, len)) }
    }
}

impl Default for Exchange {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        // SAFETY: we own the one reference gcr_secret_exchange_new returned.
        unsafe { g_object_unref(self.0.cast()) }
    }
}

/// Copies a transfer-full `gchar *` into a `String` and frees it.
///
/// # Safety
/// `ptr` must be a NUL-terminated string allocated with GLib's allocator.
unsafe fn take_string(ptr: *mut c_char) -> String {
    assert!(!ptr.is_null(), "gcr returned NULL");
    // SAFETY: per the contract above.
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    // SAFETY: per the contract above; we hold the only reference.
    unsafe { g_free(ptr.cast()) };
    s
}
