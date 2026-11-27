// SPDX-License-Identifier: GPL-3.0-only
//! The local decomp backend as a reference peer of [`Diff`] (nv-rs#928, ADR-0020).
//!
//! The backend is built on the machine that runs the tests and is never part of the repository. The
//! library it exports shares the engine's guest memory; calls to functions the module does not
//! contain reach the engine's by-address dispatch through the import hook. Without the backend
//! this module makes no peer at all, so public CI runs as before. The private run reads the
//! `backend-pass:` and `backend-fail:` lines this module prints when it writes its ledger evidence.

#[cfg(any(test, feature = "test-support"))]
use super::diff::{Branch, Difference, Found, ReferencePeer, Report};
use crate::{Engine, Ret};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(any(test, feature = "test-support"))]
use std::sync::OnceLock;

/// What the export's function returns, as the build's `exports.tsv` spells it. Every kind has the
/// one word the wrapper returns (`EAX`); the others add what a caller of the original reads next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResultKind {
    /// `EAX` alone: an integer, a pointer, a bool, or nothing.
    #[default]
    Word,
    /// A `float`: the word is its bits, and `ST0` holds the value.
    F32,
    /// A `double`: `ST0` holds it; the word is zero.
    F64,
    /// A 64-bit integer: the word is the low half (`EAX`), `EDX` the high half.
    U64,
}

impl ResultKind {
    fn from_word(word: Option<&str>) -> Self {
        match word {
            Some("f32") => Self::F32,
            Some("f64") => Self::F64,
            Some("u64") => Self::U64,
            _ => Self::Word,
        }
    }
}

/// The widest export a wrapper is built for: `decomp_backend.py` gives up on anything wider.
pub const MAX_ARITY: usize = 12;

/// One callable export: its original address, the shim's wrapper and the result's spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub address: u32,
    pub wrapper: String,
    pub arity: usize,
    pub kind: ResultKind,
}

/// The host side of the module's imports: an import's name and its guest words in, its result out.
pub type ImportHook = Box<dyn Fn(&str, &[u32]) -> u32>;

/// The part of the generated shim the peer uses. Kept separate from the loader so the routing, the
/// trap and the return conversion can be tested without a built backend.
pub trait Backend {
    /// Bind guest memory and the original-address table.
    fn init(&self, guest: *mut u8, size: u64, entries: u32, globals: &[u32]) -> bool;
    /// Call the export's wrapper with guest words.
    fn call(&self, wrapper: &str, args: &[u32]) -> u32;
    /// The rest of the last call's result: `EDX` (the high word of a 64-bit integer) and `ST0` (a
    /// `double`). A backend that cannot say returns zeros.
    fn wide(&self) -> (u32, f64) {
        (0, 0.0)
    }
    /// Clear the last trap, then report the trap the last call left, if any.
    fn trap(&self, clear: bool) -> Option<String>;
    /// Route the module's imports to the host.
    fn set_import_hook(&self, hook: ImportHook);
    /// Whether the library has a symbol of this name. A backend that cannot tell says yes.
    fn has_symbol(&self, _name: &str) -> bool {
        true
    }
}

thread_local! {
    /// The engine an import call reaches while the backend runs. Set for one call only: the module's
    /// trap state is process-global too, so one call happens at a time.
    static ACTIVE_ENGINE: std::cell::Cell<*mut Engine> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    static IMPORT_HOOK: RefCell<Option<ImportHook>> = const { RefCell::new(None) };
    static LAST_TRAP: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn hook_engine(addr: u32, args: &[u32]) -> u32 {
    let words: Vec<u32> = args.to_vec();
    ACTIVE_ENGINE.with(|cell| {
        let p = cell.get();
        if p.is_null() {
            0
        } else {
            // SAFETY: set just before the backend call and cleared right after it, on one thread.
            unsafe { (*p).call(addr, &words) }.eax
        }
    })
}

extern "C" fn import_hook(name: *const c_char, args: *mut u32, n: u32) -> u32 {
    if name.is_null() || args.is_null() {
        return 0;
    }
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let words = unsafe { std::slice::from_raw_parts(args, n as usize) };
    IMPORT_HOOK.with(|h| h.borrow().as_ref().map_or(0, |f| f(&name, words)))
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Leaves one level of [`DEPTH`] when a backend call ends, however it ends.
struct Leave;

impl Drop for Leave {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// The peer for the cases of one process. Every case gets a fresh engine, but the library, its
/// table and its trap state are shared; only the guest bytes change between cases.
pub struct BackendPeer<B: Backend> {
    backend: Arc<B>,
    exports: Arc<Vec<Export>>,
    globals: Arc<Vec<u32>>,
    imports: Arc<HashMap<String, u32>>,
    initialized: Arc<AtomicBool>,
}

impl<B: Backend> Clone for BackendPeer<B> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            exports: self.exports.clone(),
            globals: self.globals.clone(),
            imports: self.imports.clone(),
            initialized: self.initialized.clone(),
        }
    }
}

impl<B: Backend> BackendPeer<B> {
    pub fn new(
        backend: B,
        exports: Vec<Export>,
        globals: Vec<u32>,
        imports: HashMap<String, u32>,
    ) -> Self {
        Self {
            backend: Arc::new(backend),
            exports: Arc::new(exports),
            globals: Arc::new(globals),
            imports: Arc::new(imports),
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    fn init(&self, e: &mut Engine) {
        if !self.initialized.load(Ordering::Relaxed) {
            // The module's linear memory is the engine's reservation: guest address `a` is at
            // `guest + a`, the same bytes the translations read and write.
            let ok = self
                .backend
                .init(e.mem.guest_base(), 1 << 32, 0x0100_0000, &self.globals);
            self.initialized.store(ok, Ordering::Relaxed);
        }
        let imports = self.imports.clone();
        self.backend.set_import_hook(Box::new(move |name, args| {
            let addr = imports.get(name).copied().unwrap_or(0);
            if addr == 0 {
                return 0;
            }
            hook_engine(addr, args)
        }));
    }
}

impl<B: Backend> BackendPeer<B> {
    /// The export's result for `args` on `e`'s guest memory, or `None` when the module has no
    /// function at `addr`. The trap the call left is kept for [`last_trap`].
    pub(crate) fn run_export(&self, e: &mut Engine, addr: u32, args: &[u32]) -> Option<Ret> {
        let export = self.exports.iter().find(|x| x.address == addr)?;
        // The module's trap state and import hook are process-global: one call at a time across
        // threads. A call the module makes back into a gap on the same thread must not wait on itself.
        let _serial = DEPTH.with(|d| {
            let outer = d.get() == 0;
            d.set(d.get() + 1);
            outer.then(|| SERIAL.lock().unwrap_or_else(|p| p.into_inner()))
        });
        let _leave = Leave;
        self.init(e);
        self.backend.trap(true);
        ACTIVE_ENGINE.with(|cell| cell.set(e as *mut Engine));
        let bits = self.backend.call(&export.wrapper, args);
        ACTIVE_ENGINE.with(|cell| cell.set(std::ptr::null_mut()));
        let trap = self.backend.trap(false);
        LAST_TRAP.with(|t| *t.borrow_mut() = trap);
        let (edx, st0) = self.backend.wide();
        let mut ret = Ret {
            eax: bits,
            edx: 0,
            st0: 0.0,
        };
        match export.kind {
            ResultKind::Word => {}
            ResultKind::F32 => ret.st0 = f32::from_bits(bits) as f64,
            ResultKind::F64 => ret.st0 = st0,
            ResultKind::U64 => ret.edx = edx,
        }
        Some(ret)
    }

    /// Whether the library has every wrapper the `.tsv` lists. A `.tsv` left over from another build
    /// names exports the library lacks (or has moved); the first one missing is the error.
    pub(crate) fn verify(&self) -> Result<(), String> {
        if !self.backend.has_symbol("decomp_init") {
            return Err("the library has no decomp_init: it is not a backend build".into());
        }
        match self
            .exports
            .iter()
            .find(|x| !self.backend.has_symbol(&x.wrapper))
        {
            Some(x) => Err(format!(
                "the .tsv lists {} at {:08x} but the library has no such symbol: it is from another build",
                x.wrapper, x.address
            )),
            None => Ok(()),
        }
    }

    /// Whether the module has a function at `addr`.
    pub(crate) fn has_export(&self, addr: u32) -> bool {
        self.exports.iter().any(|x| x.address == addr)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<B: Backend> ReferencePeer for BackendPeer<B> {
    fn run(&self, e: &mut Engine, addr: u32, args: &[u32]) -> Ret {
        // A unit the build could not compile stays on its Rust translation.
        self.run_export(e, addr, args)
            .unwrap_or_else(|| e.call(addr, args))
    }
}

#[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
/// The trap the last backend call left, for the harness to record on the report.
pub fn last_trap() -> Option<String> {
    LAST_TRAP.with(|t| t.borrow().clone())
}

#[cfg(any(test, feature = "test-support"))]
/// The class a backend mismatch is recorded under in the ledger, empty when the peer passed.
pub fn classify(report: &Report) -> &'static str {
    if report.reference_found == 0 {
        return "";
    }
    let fault = report.backend_fault.as_deref().unwrap_or("");
    if fault.to_ascii_lowercase().contains("uninit") {
        return "uninitialised read";
    }
    let reference: Vec<&Difference> = report
        .first
        .iter()
        .filter(|d| d.branch == Branch::Reference)
        .collect();
    let stopped = reference.iter().find(|d| {
        matches!(
            d.what,
            Found::Stop { .. } | Found::StoppedAlike { .. } | Found::ExpectedStop
        )
    });
    if let Some(stop) = stopped {
        let message = match &stop.what {
            Found::Stop { translation, typed } => format!("{translation:?} {typed:?}"),
            Found::StoppedAlike { message } => message.clone(),
            _ => String::new(),
        };
        return if message.contains("call budget spent") || message.contains("timeout") {
            "hang"
        } else {
            "fault"
        };
    }
    if !fault.is_empty() {
        return "fault";
    }
    // A difference only in a value the test already tolerates as a float is a translation
    // difference, not a typed one.
    if report.tolerated > 0
        && reference.iter().all(|d| {
            matches!(
                d.what,
                Found::Return { .. } | Found::Call { .. } | Found::Memory { .. }
            )
        })
    {
        return "float-only";
    }
    "real"
}

#[cfg(any(test, feature = "test-support"))]
/// Print one machine-readable line per function, read by `backend_evidence.py` after the private
/// run. Without a backend nothing is printed, so public CI's test output is unchanged.
pub fn report(addr: u32, report: &Report) {
    if !active() {
        return;
    }
    let class = classify(report);
    if class.is_empty() {
        println!("backend-pass: {addr:08x}");
    } else {
        println!("backend-fail: {addr:08x} {class}");
    }
}

#[cfg(any(test, feature = "test-support"))]
fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| std::env::var_os("NV_BACKEND_LIBRARY").is_some_and(|v| !v.is_empty()))
}

/// Parse the build's `exports.tsv`: `#` comments, `export <va> <wrapper> <arity> [f32|f64|u64]`,
/// `import <name> <va>` and `global <va>` lines. No game or reconstruction bytes are read.
pub fn parse_exports(text: &str) -> (Vec<Export>, Vec<u32>, HashMap<String, u32>) {
    let (mut exports, mut globals, mut imports) = (Vec::new(), Vec::new(), HashMap::new());
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        match words.next() {
            Some("export") => {
                let (Some(va), Some(wrapper), Some(arity)) =
                    (words.next(), words.next(), words.next())
                else {
                    continue;
                };
                if let (Ok(address), Ok(arity)) = (u32::from_str_radix(va, 16), arity.parse()) {
                    if arity > MAX_ARITY {
                        continue;
                    }
                    exports.push(Export {
                        address,
                        wrapper: wrapper.into(),
                        arity,
                        kind: ResultKind::from_word(words.next()),
                    });
                }
            }
            Some("import") => {
                if let (Some(name), Some(va)) = (words.next(), words.next()) {
                    if let Ok(address) = u32::from_str_radix(va, 16) {
                        imports.insert(name.into(), address);
                    }
                }
            }
            Some("global") => {
                if let Some(va) = words.next() {
                    if let Ok(address) = u32::from_str_radix(va, 16) {
                        globals.push(address);
                    }
                }
            }
            _ => {}
        }
    }
    (exports, globals, imports)
}

/// The file the build writes its exports to, beside the library.
pub(crate) fn exports_file(library: &std::path::Path) -> std::path::PathBuf {
    library.with_file_name("exports.tsv")
}

/// The backend loaded from `library` and the `exports.tsv` beside it, or `None` when either is
/// missing.
pub(crate) fn load(library: &std::path::Path) -> Option<BackendPeer<LibBackend>> {
    let text = std::fs::read_to_string(exports_file(library)).ok()?;
    let (exports, globals, imports) = parse_exports(&text);
    LibBackend::open(library)
        .ok()
        .map(|backend| BackendPeer::new(backend, exports, globals, imports))
}

/// The peer from the private run's inputs, or `None` in public CI. One gate line per process says
/// whether the backend was present.
#[cfg(any(test, feature = "test-support"))]
pub fn from_env() -> Option<Box<dyn ReferencePeer>> {
    static PEER: OnceLock<Option<BackendPeer<LibBackend>>> = OnceLock::new();
    let peer = PEER.get_or_init(|| load(&gate::require_data("NV_BACKEND_LIBRARY")?));
    peer.clone().map(|p| Box::new(p) as Box<dyn ReferencePeer>)
}

pub(crate) struct LibBackend {
    library: Arc<libloading::Library>,
}

impl LibBackend {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        unsafe { libloading::Library::new(path) }
            .map(|library| Self {
                library: Arc::new(library),
            })
            .map_err(|e| e.to_string())
    }
}

impl Backend for LibBackend {
    fn init(&self, guest: *mut u8, size: u64, entries: u32, globals: &[u32]) -> bool {
        type Init = unsafe extern "C" fn(*mut u8, u64, u32, *const u32, u32) -> i32;
        unsafe {
            let Ok(f) = self.library.get::<Init>(b"decomp_init\0") else {
                return false;
            };
            f(
                guest,
                size,
                entries,
                globals.as_ptr(),
                u32::try_from(globals.len()).unwrap_or(u32::MAX),
            ) == 0
        }
    }

    fn call(&self, wrapper: &str, args: &[u32]) -> u32 {
        let name = format!("{wrapper}\0");
        let bytes = name.as_bytes();
        // The shim's wrapper is named for the export, not its arity: its signature comes from
        // `exports.tsv`.
        // The wrapper's signature is `u32 (u32 x arity)`, spelled out per arity.
        macro_rules! call_with {
            ($($arg:ident),*) => {{
                type F = unsafe extern "C" fn($(call_with!(@word $arg)),*) -> u32;
                let [$($arg),*] = args else { unreachable!() };
                unsafe { self.library.get::<F>(bytes).map(|func| func($(*$arg),*)) }
            }};
            (@word $arg:ident) => { u32 };
        }
        match args.len() {
            0 => unsafe { self.library.get::<unsafe extern "C" fn() -> u32>(bytes).map(|f| f()) },
            1 => call_with!(a),
            2 => call_with!(a, b),
            3 => call_with!(a, b, c),
            4 => call_with!(a, b, c, d),
            5 => call_with!(a, b, c, d, e),
            6 => call_with!(a, b, c, d, e, f),
            7 => call_with!(a, b, c, d, e, f, g),
            8 => call_with!(a, b, c, d, e, f, g, h),
            9 => call_with!(a, b, c, d, e, f, g, h, i),
            10 => call_with!(a, b, c, d, e, f, g, h, i, j),
            11 => call_with!(a, b, c, d, e, f, g, h, i, j, k),
            12 => call_with!(a, b, c, d, e, f, g, h, i, j, k, l),
            _ => Ok(0),
        }
        .unwrap_or(0)
    }

    fn wide(&self) -> (u32, f64) {
        unsafe {
            let edx = self
                .library
                .get::<unsafe extern "C" fn() -> u32>(b"decomp_last_edx\0")
                .map_or(0, |f| f());
            let st0 = self
                .library
                .get::<unsafe extern "C" fn() -> f64>(b"decomp_last_st0\0")
                .map_or(0.0, |f| f());
            (edx, st0)
        }
    }

    fn trap(&self, clear: bool) -> Option<String> {
        unsafe {
            if clear {
                if let Ok(f) = self
                    .library
                    .get::<unsafe extern "C" fn()>(b"decomp_clear_trap\0")
                {
                    f();
                }
                return None;
            }
            let code = self
                .library
                .get::<unsafe extern "C" fn() -> u32>(b"decomp_last_trap_code\0")
                .ok()?;
            if code() == 0 {
                return None;
            }
            let reason = self
                .library
                .get::<unsafe extern "C" fn() -> *const c_char>(b"decomp_last_trap_reason\0")
                .ok()?;
            let p = reason();
            Some(if p.is_null() {
                "trap".into()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            })
        }
    }

    fn has_symbol(&self, name: &str) -> bool {
        let name = format!("{name}\0");
        unsafe { self.library.get::<*const ()>(name.as_bytes()) }.is_ok()
    }

    fn set_import_hook(&self, hook: ImportHook) {
        IMPORT_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        unsafe {
            if let Ok(f) = self.library.get::<unsafe extern "C" fn(
                unsafe extern "C" fn(*const c_char, *mut u32, u32) -> u32,
            )>(b"decomp_set_import_hook\0")
            {
                f(import_hook);
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code)]
pub(super) mod gate {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/fork/test_gate.rs"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::diff::{Branch, Case, Input};

    struct Fake {
        guest: std::cell::Cell<*mut u8>,
        trap: RefCell<Option<String>>,
    }

    impl Backend for Fake {
        fn init(&self, guest: *mut u8, _size: u64, _entries: u32, _globals: &[u32]) -> bool {
            self.guest.set(guest);
            true
        }
        fn call(&self, wrapper: &str, args: &[u32]) -> u32 {
            assert_eq!(wrapper, "decomp_call_0");
            // SAFETY: the guest reservation is mapped and this test writes inside it.
            unsafe { std::ptr::write(self.guest.get().add(args[0] as usize), 0x5a) };
            7
        }
        fn trap(&self, clear: bool) -> Option<String> {
            if clear {
                None
            } else {
                self.trap.borrow_mut().take()
            }
        }
        fn set_import_hook(&self, _hook: ImportHook) {}
    }

    fn fake() -> BackendPeer<Fake> {
        BackendPeer::new(
            Fake {
                guest: std::cell::Cell::new(std::ptr::null_mut()),
                trap: RefCell::new(None),
            },
            vec![Export {
                address: 0x00ef_1000,
                wrapper: "decomp_call_0".into(),
                arity: 1,
                kind: ResultKind::Word,
            }],
            vec![],
            HashMap::new(),
        )
    }

    #[test]
    fn exports_carry_original_addresses() {
        let (exports, globals, imports) = parse_exports(
            "# generated\nexport 00ef1000 decomp_call_0 1 f32\nimport env_get 00aa0000\nglobal 00bb0000\n",
        );
        assert_eq!(
            exports,
            vec![Export {
                address: 0x00ef_1000,
                wrapper: "decomp_call_0".into(),
                arity: 1,
                kind: ResultKind::F32,
            }]
        );
        assert_eq!(globals, vec![0x00bb_0000]);
        assert_eq!(imports["env_get"], 0x00aa_0000);
    }

    #[test]
    fn the_backend_runs_on_the_engines_guest_memory() {
        let mut e = Engine::new_untyped_for_test();
        e.map(0x0200_1000, 0x40);
        let ret = ReferencePeer::run(&fake(), &mut e, 0x00ef_1000, &[0x0200_1010]);
        assert_eq!(ret.eax, 7);
        assert_eq!(e.mem.u8(0x0200_1010), 0x5a);
    }

    #[test]
    fn a_float_result_keeps_the_f32_bits() {
        let mut e = Engine::new_untyped_for_test();
        e.map(0x0200_1000, 0x40);
        let mut peer = fake();
        peer.exports = Arc::new(vec![Export {
            address: 0x00ef_1000,
            wrapper: "decomp_call_0".into(),
            arity: 1,
            kind: ResultKind::F32,
        }]);
        let ret = ReferencePeer::run(&peer, &mut e, 0x00ef_1000, &[0x0200_1010]);
        assert_eq!(ret.st0, f32::from_bits(7) as f64);
    }

    /// A backend whose last call left `EDX` and `ST0` behind, as the shim's accessors report them.
    struct Wide;
    impl Backend for Wide {
        fn init(&self, _: *mut u8, _: u64, _: u32, _: &[u32]) -> bool {
            true
        }
        fn call(&self, _: &str, _: &[u32]) -> u32 {
            0x1111_2222
        }
        fn wide(&self) -> (u32, f64) {
            (0x3333_4444, 2.5)
        }
        fn trap(&self, _: bool) -> Option<String> {
            None
        }
        fn set_import_hook(&self, _: ImportHook) {}
    }

    fn wide_peer(kind: ResultKind) -> BackendPeer<Wide> {
        BackendPeer::new(
            Wide,
            vec![Export {
                address: 0x00ef_1000,
                wrapper: "decomp_call_0".into(),
                arity: 0,
                kind,
            }],
            vec![],
            HashMap::new(),
        )
    }

    #[test]
    fn a_wide_result_hands_back_eax_edx_and_st0() {
        let mut e = Engine::new_untyped_for_test();
        let ret = ReferencePeer::run(&wide_peer(ResultKind::U64), &mut e, 0x00ef_1000, &[]);
        assert_eq!((ret.eax, ret.edx, ret.st0), (0x1111_2222, 0x3333_4444, 0.0));
        let ret = ReferencePeer::run(&wide_peer(ResultKind::F64), &mut e, 0x00ef_1000, &[]);
        assert_eq!((ret.eax, ret.edx, ret.st0), (0x1111_2222, 0, 2.5));
        // A one-word export keeps EDX and ST0 as they were: what the callee left is not its result.
        let ret = ReferencePeer::run(&wide_peer(ResultKind::Word), &mut e, 0x00ef_1000, &[]);
        assert_eq!((ret.eax, ret.edx, ret.st0), (0x1111_2222, 0, 0.0));
    }

    #[test]
    fn the_exports_are_read_from_the_file_the_build_writes() {
        assert_eq!(
            exports_file(std::path::Path::new("/b/out/libfnv_decomp.so")),
            std::path::PathBuf::from("/b/out/exports.tsv")
        );
        assert_eq!(
            exports_file(std::path::Path::new("fnv_decomp.dll")),
            std::path::PathBuf::from("exports.tsv")
        );
    }

    #[test]
    fn the_tsv_says_what_an_export_returns_and_drops_what_is_too_wide() {
        let (exports, _, _) = parse_exports(
            "export 00001000 decomp_call_0 2\nexport 00002000 decomp_call_1 1 f32\n\
             export 00003000 decomp_call_2 3 f64\nexport 00004000 decomp_call_3 12 u64\n\
             export 00005000 decomp_call_4 13\n",
        );
        let kinds: Vec<_> = exports.iter().map(|x| (x.address, x.arity, x.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (0x1000, 2, ResultKind::Word),
                (0x2000, 1, ResultKind::F32),
                (0x3000, 3, ResultKind::F64),
                (0x4000, 12, ResultKind::U64),
            ]
        );
    }

    #[test]
    fn an_address_without_an_export_stays_on_the_translation() {
        let mut e = Engine::new_untyped_for_test();
        e.register(0x00ef_2000, |_, _| Ret {
            eax: 11,
            ..Ret::default()
        });
        let ret = ReferencePeer::run(&fake(), &mut e, 0x00ef_2000, &[]);
        assert_eq!(ret.eax, 11);
    }

    #[test]
    fn a_mismatch_has_a_failure_class() {
        let report = Report {
            runs: 2,
            reference_runs: 2,
            reference_found: 1,
            ..Report::default()
        };
        assert_eq!(classify(&report), "real");
        let mut report = report;
        report.first.push(Difference {
            case: Case(vec![]),
            control_word: 0x037a,
            branch: Branch::Reference,
            what: Found::Stop {
                translation: Some("call budget spent: more than 100000 calls".into()),
                typed: None,
            },
        });
        assert_eq!(classify(&report), "hang");
        let clean = Report {
            runs: 2,
            reference_runs: 2,
            ..Report::default()
        };
        assert_eq!(classify(&clean), "");
    }

    struct WrongByte;
    impl Backend for WrongByte {
        fn init(&self, _: *mut u8, _: u64, _: u32, _: &[u32]) -> bool {
            true
        }
        fn call(&self, _: &str, args: &[u32]) -> u32 {
            let e = unsafe { &mut *ACTIVE_ENGINE.with(|c| c.get()) };
            e.mem.set_u32(args[0] + 8, args[1]);
            e.mem.set_u8(args[0] + 9, e.mem.u8(args[0] + 9) ^ 0x5a);
            0
        }
        fn trap(&self, _: bool) -> Option<String> {
            None
        }
        fn set_import_hook(&self, _: ImportHook) {}
    }

    #[test]
    fn a_planted_bug_control_fails_against_the_peer() {
        fn typed(e: &mut Engine, args: &[u32]) -> Ret {
            e.mem.set_u32(args[0] + 8, args[1]);
            Ret::default()
        }
        fn translation(e: &mut Engine, args: &[u32]) -> Ret {
            e.mem.set_u32(args[0] + 8, args[1]);
            Ret::default()
        }
        let peer = BackendPeer::new(
            WrongByte,
            vec![Export {
                address: 0x00ef_1000,
                wrapper: "decomp_call_0".into(),
                arity: 2,
                kind: ResultKind::Word,
            }],
            vec![],
            HashMap::new(),
        );
        let report = crate::typed::diff::Diff::new((0x00ef_1000, typed))
            .translation(translation)
            .reference_peer(peer)
            .inputs(&[Input::Of(&[0x0200_1000]), Input::Of(&[0x1234])])
            .setup(|e, case: &Case| {
                e.map(0x0200_1000, 0x40);
                vec![case.word(0), case.word(1)]
            })
            .run();
        assert!(!report.is_same(), "{report}");
        assert!(report.reference_found > 0, "{report}");
        assert_eq!(classify(&report), "real");
    }
}
