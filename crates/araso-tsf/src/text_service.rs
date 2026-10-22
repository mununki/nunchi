//! The text input processor: one per thread that has text fields, with one `InputSession`.
//!
//! Keys are handled in `OnTestKeyDown`, inside a synchronous edit session, and the answer kept
//! for the `OnKeyDown` that follows: whether the session eats a key is only known once it has
//! handled it (Space commits the word, and then the app types the space itself), and a key the
//! app should get has to have its word committed before the app sees it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use araso_core::{InputSession, Key};
use windows::core::{implement, Interface, Ref, Result, BOOL, GUID};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::TextServices::*;
use windows_core::IUnknownImpl;

use crate::client::TsfClient;
use crate::display_attribute::{DisplayAttributeInfo, EnumDisplayAttributeInfo};
use crate::edit_session::EditSession;
use crate::keys::{self, Action};
use crate::runtime;

#[implement(
    ITfTextInputProcessorEx,
    ITfTextInputProcessor,
    ITfKeyEventSink,
    ITfThreadMgrEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider
)]
pub struct TextService {
    inner: RefCell<Inner>,
    /// Our own edit session is running.
    editing: Cell<bool>,
}

#[derive(Default)]
struct Inner {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    thread_mgr_cookie: Option<u32>,
    composition: Option<ITfComposition>,
    /// The context the composition is in.
    composition_context: Option<ITfContext>,
    session: Option<InputSession>,
    generation: u64,
    atom: u32,
    /// The key the last `OnTestKeyDown` or `OnTestKeyUp` handled, for the `OnKeyDown` or
    /// `OnKeyUp` after it.
    pending: Option<Pending>,
    taps: keys::Taps,
}

/// A key event handled when TSF asked whether it would be, and the answer.
struct Pending {
    up: bool,
    wparam: usize,
    eaten: bool,
    at: std::time::Instant,
}

impl Pending {
    fn new(up: bool, wparam: WPARAM, eaten: bool) -> Self {
        Pending { up, wparam: wparam.0, eaten, at: std::time::Instant::now() }
    }
}

impl TextService {
    pub fn new() -> Self {
        crate::add_ref();
        TextService { inner: RefCell::new(Inner::default()), editing: Cell::new(false) }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::release();
    }
}

impl TextService_Impl {
    /// The session, rebuilt from the runtime when a setting changed (not mid-word: the word
    /// being typed finishes in the session it started in).
    fn ensure_session(&self) {
        let runtime = runtime::current();
        let mut inner = self.inner.borrow_mut();
        if let Some(session) = &inner.session {
            if inner.generation == runtime.generation || session.is_composing() {
                return;
            }
        }
        let mut session = runtime::make_session(&runtime);
        session.activate(runtime::app_name().as_deref());
        inner.session = Some(session);
        inner.generation = runtime.generation;
    }

    /// Runs `op` on the session with a client for `context`, in a synchronous read-write edit
    /// session. Nil when the edit session couldn't run (the document is locked).
    fn edit<R: 'static>(
        &self,
        context: &ITfContext,
        op: impl FnOnce(&mut InputSession, &mut TsfClient) -> R + 'static,
    ) -> Option<R> {
        let this = self.to_object();
        let result: Rc<RefCell<Option<R>>> = Rc::new(RefCell::new(None));
        let out = result.clone();
        let session_context = context.clone();
        let body = EditSession::new(move |ec| {
            let (mut session, mut composition, atom) = {
                let mut inner = this.inner.borrow_mut();
                let Some(session) = inner.session.take() else { return Ok(()) };
                (session, inner.composition.take(), inner.atom)
            };
            let sink: ITfCompositionSink = this.to_interface();
            let transitory = crate::client::is_transitory(&session_context);
            let mut client =
                TsfClient { ec, context: &session_context, composition: &mut composition, sink: &sink, atom, transitory };
            let r = op(&mut session, &mut client);
            let mut inner = this.inner.borrow_mut();
            inner.composition_context = composition.as_ref().map(|_| session_context.clone());
            inner.composition = composition;
            inner.session = Some(session);
            *out.borrow_mut() = Some(r);
            Ok(())
        });
        let client_id = self.inner.borrow().client_id;
        self.editing.set(true);
        let requested = unsafe { context.RequestEditSession(client_id, &body, TF_ES_SYNC | TF_ES_READWRITE) };
        self.editing.set(false);
        match requested {
            Ok(hr) if hr.is_ok() => {}
            Ok(hr) => crate::log::write(&format!("edit session refused: {hr:?}")),
            Err(e) => crate::log::write(&format!("edit session failed: {e}")),
        }
        let r = result.borrow_mut().take();
        r
    }

    /// Commits what is being composed, in the context it is in.
    fn commit_composition(&self, new_context: bool) {
        let context = self.inner.borrow().composition_context.clone();
        let Some(context) = context else {
            if let Some(session) = self.inner.borrow_mut().session.as_mut() {
                if new_context {
                    session.commit_for_new_context(None);
                } else {
                    session.commit(None);
                }
            }
            return;
        };
        self.edit(&context, move |session, client| {
            if new_context {
                session.commit_for_new_context(Some(client));
            } else {
                session.commit(Some(client));
            }
        });
    }

    fn key_down(&self, context: &ITfContext, wparam: WPARAM, lparam: LPARAM) -> bool {
        let vk = VIRTUAL_KEY(wparam.0 as u16);
        self.inner.borrow_mut().taps.key_down(vk, lparam.0);
        self.ensure_session();
        let runtime = runtime::current();
        if !runtime.has_data() {
            return false;
        }
        let action = {
            let inner = self.inner.borrow();
            let Some(session) = inner.session.as_ref() else { return false };
            keys::translate(vk, lparam.0, session.policy(), &runtime.settings.pause_key)
        };
        let eaten = match action {
            Action::Pass => false,
            Action::Command => {
                self.edit(context, |session, client| session.command_pressed(client));
                false
            }
            Action::Pause => {
                self.edit(context, |session, client| session.toggle_pause(Some(client)));
                let paused = self.inner.borrow().session.as_ref().is_some_and(|s| s.paused());
                crate::log::write(&format!("paused={paused}"));
                true
            }
            Action::Key(key) => {
                let eaten = self.edit(context, move |session, client| session.handle(key, client)).unwrap_or(false);
                crate::log::write(&format!("key {} eaten={eaten}", keys::kind(key)));
                eaten
            }
        };
        eaten
    }

    fn key_up(&self, context: &ITfContext, wparam: WPARAM, lparam: LPARAM) -> bool {
        let vk = VIRTUAL_KEY(wparam.0 as u16);
        let Some(tap) = self.inner.borrow_mut().taps.key_up(vk, lparam.0) else { return false };
        self.ensure_session();
        let runtime = runtime::current();
        if !runtime.has_data() {
            return false;
        }
        let pause = &runtime.settings.pause_key;
        let (switch, vim) = {
            let inner = self.inner.borrow();
            let Some(session) = inner.session.as_ref() else { return false };
            (keys::switch_tap(tap, session.policy(), pause), session.in_vim_normal_mode())
        };
        if keys::pause_tap(tap, pause) {
            self.edit(context, |session, client| session.toggle_pause(Some(client)));
            crate::log::write("pause tap");
            return false;
        }
        if switch && !vim {
            let handled = self.edit(context, |session, client| session.handle(Key::Cycle, client)).unwrap_or(false);
            crate::log::write(&format!("switch tap handled={handled}"));
            // Right Alt let go on its own opens the app's menu: not when it switched a word.
            return handled && tap == araso_core::settings::Tap::RightAlt;
        }
        false
    }

    /// The answer for a key event the test call already handled; nil for another one.
    fn take_pending(&self, up: bool, wparam: WPARAM) -> Option<bool> {
        let pending = self.inner.borrow_mut().pending.take()?;
        // The same key: IMM32's path hands the key over twice with a different lParam (the scan
        // code and flags rebuilt), so only the key itself is compared.
        let same = pending.up == up && pending.wparam == wparam.0;
        (same && pending.at.elapsed() < std::time::Duration::from_secs(2)).then_some(pending.eaten)
    }

    fn focus_changed(&self) {
        if self.inner.borrow().composition.is_some() {
            self.commit_composition(true);
        }
        self.ensure_session();
        let mut inner = self.inner.borrow_mut();
        if let Some(session) = inner.session.as_mut() {
            session.activate(runtime::app_name().as_deref());
        }
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, thread_mgr: Ref<ITfThreadMgr>, client_id: u32) -> Result<()> {
        ITfTextInputProcessorEx_Impl::ActivateEx(self, thread_mgr, client_id, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        crate::guard(|| {
            if self.inner.borrow().composition.is_some() {
                self.commit_composition(false);
            }
            let (thread_mgr, cookie, client_id) = {
                let mut inner = self.inner.borrow_mut();
                (inner.thread_mgr.take(), inner.thread_mgr_cookie.take(), inner.client_id)
            };
            if let Some(thread_mgr) = thread_mgr {
                unsafe {
                    if let Ok(keystrokes) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                        let _ = keystrokes.UnadviseKeyEventSink(client_id);
                    }
                    if let (Some(cookie), Ok(source)) = (cookie, thread_mgr.cast::<ITfSource>()) {
                        let _ = source.UnadviseSink(cookie);
                    }
                }
            }
            let mut inner = self.inner.borrow_mut();
            inner.session = None;
            inner.composition = None;
            inner.composition_context = None;
            crate::log::write("deactivated");
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, thread_mgr: Ref<ITfThreadMgr>, client_id: u32, _flags: u32) -> Result<()> {
        crate::guard(|| {
            let thread_mgr = thread_mgr.ok()?.clone();
            runtime::preload();
            {
                let mut inner = self.inner.borrow_mut();
                inner.thread_mgr = Some(thread_mgr.clone());
                inner.client_id = client_id;
            }
            unsafe {
                let source: ITfSource = thread_mgr.cast()?;
                let sink: ITfThreadMgrEventSink = self.to_interface();
                let cookie = source.AdviseSink(&ITfThreadMgrEventSink::IID, &sink)?;
                self.inner.borrow_mut().thread_mgr_cookie = Some(cookie);

                let keystrokes: ITfKeystrokeMgr = thread_mgr.cast()?;
                let key_sink: ITfKeyEventSink = self.to_interface();
                keystrokes.AdviseKeyEventSink(client_id, &key_sink, true)?;

                if let Ok(categories) =
                    CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                {
                    if let Ok(atom) = categories.RegisterGUID(&crate::GUID_DISPLAY_ATTRIBUTE_INPUT) {
                        self.inner.borrow_mut().atom = atom;
                    }
                }
            }
            self.ensure_session();
            crate::log::write(&format!("activated, client {client_id}"));
            Ok(())
        })
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(&self, context: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            let Some(context) = context.as_ref() else { return Ok(false.into()) };
            let eaten = self.key_down(context, wparam, lparam);
            self.inner.borrow_mut().pending = Some(Pending::new(false, wparam, eaten));
            Ok(eaten.into())
        })
    }

    fn OnKeyDown(&self, context: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            if let Some(eaten) = self.take_pending(false, wparam) {
                return Ok(eaten.into());
            }
            let Some(context) = context.as_ref() else { return Ok(false.into()) };
            Ok(self.key_down(context, wparam, lparam).into())
        })
    }

    fn OnTestKeyUp(&self, context: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            let Some(context) = context.as_ref() else { return Ok(false.into()) };
            let eaten = self.key_up(context, wparam, lparam);
            self.inner.borrow_mut().pending = Some(Pending::new(true, wparam, eaten));
            Ok(eaten.into())
        })
    }

    fn OnKeyUp(&self, context: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard(|| {
            if let Some(eaten) = self.take_pending(true, wparam) {
                return Ok(eaten.into());
            }
            let Some(context) = context.as_ref() else { return Ok(false.into()) };
            Ok(self.key_up(context, wparam, lparam).into())
        })
    }

    fn OnPreservedKey(&self, _context: Ref<ITfContext>, _guid: *const GUID) -> Result<BOOL> {
        Ok(false.into())
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _manager: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _manager: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnSetFocus(&self, _focus: Ref<ITfDocumentMgr>, _previous: Ref<ITfDocumentMgr>) -> Result<()> {
        crate::guard(|| {
            self.focus_changed();
            Ok(())
        })
    }

    fn OnPushContext(&self, _context: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _context: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    /// The app ended the composition (a click elsewhere): the text stays as it is shown.
    fn OnCompositionTerminated(&self, ec: u32, composition: Ref<ITfComposition>) -> Result<()> {
        crate::guard(|| {
            crate::log::write(&format!("terminated: editing={} ours={}", self.editing.get(), self.inner.borrow().composition.is_some()));
            let context = self.inner.borrow().composition_context.clone();
            if let (Some(composition), Some(context)) = (composition.as_ref(), context) {
                unsafe {
                    if let (Ok(range), Ok(property)) = (composition.GetRange(), context.GetProperty(&GUID_PROP_ATTRIBUTE)) {
                        let _ = property.Clear(ec, &range);
                    }
                }
            }
            let mut inner = self.inner.borrow_mut();
            inner.composition = None;
            inner.composition_context = None;
            if let Some(session) = inner.session.as_mut() {
                session.composition_terminated();
            }
            crate::log::write("composition terminated by the app");
            Ok(())
        })
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(EnumDisplayAttributeInfo { index: Cell::new(0) }.into())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if !guid.is_null() && unsafe { *guid } == crate::GUID_DISPLAY_ATTRIBUTE_INPUT {
            Ok(DisplayAttributeInfo.into())
        } else {
            Err(windows::Win32::Foundation::E_INVALIDARG.into())
        }
    }
}
