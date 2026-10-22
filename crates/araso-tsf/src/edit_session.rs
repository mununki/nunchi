//! An edit session that runs a closure: TSF hands it the edit cookie text may be read and
//! changed with.

use std::cell::RefCell;

use windows::core::implement;
use windows::Win32::UI::TextServices::{ITfEditSession, ITfEditSession_Impl};

type Body = Box<dyn FnOnce(u32) -> windows::core::Result<()>>;

#[implement(ITfEditSession)]
pub struct EditSession {
    body: RefCell<Option<Body>>,
}

impl EditSession {
    pub fn new(body: impl FnOnce(u32) -> windows::core::Result<()> + 'static) -> ITfEditSession {
        EditSession { body: RefCell::new(Some(Box::new(body))) }.into()
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
        let body = self.body.borrow_mut().take();
        match body {
            Some(body) => crate::guard(|| body(ec)),
            None => Ok(()),
        }
    }
}
