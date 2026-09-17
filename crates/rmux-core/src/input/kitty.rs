//! Kitty keyboard protocol state: which progressive enhancements are in force
//! and the stack an application saves them on.
//!
//! One of these per screen. The specification requires the main and alternate
//! screens to negotiate independently, so that a full-screen editor can change
//! the keyboard mode without knowing — or disturbing — what the shell that
//! launched it asked for.

use std::collections::VecDeque;

use super::mode;
use super::writer::ScreenWriter;

/// The one progressive enhancement rmux implements: disambiguate escape codes,
/// which is what makes a modified key distinct from the bare one.
pub(crate) const SUPPORTED: u8 = 0b1;

/// How many saves an application may stack before the oldest is evicted. The
/// specification asks for a limit so a program cannot exhaust memory by
/// pushing; the depth itself is ours.
const STACK_MAX: usize = 16;

/// How a set request applies its flags.
pub(crate) enum SetMode {
    /// Set what is named, reset what is not.
    Replace,
    /// Set what is named, leave the rest.
    Add,
    /// Reset what is named, leave the rest.
    Remove,
}

impl SetMode {
    /// The mode parameter of `CSI = flags ; mode u`, which defaults to 1.
    pub(crate) fn from_param(param: i32) -> Self {
        match param {
            2 => Self::Add,
            3 => Self::Remove,
            _ => Self::Replace,
        }
    }
}

/// Both screens' negotiations. Which one is in force is the screen's business,
/// not this type's, so every entry point is told.
///
/// Entering the alternate screen starts from nothing and leaving it throws
/// that away, so an editor's keyboard mode neither inherits from nor outlives
/// the shell it was launched from.
#[derive(Debug, Default)]
pub(crate) struct KittyScreens {
    main: KittyKeyboard,
    alternate: KittyKeyboard,
}

impl KittyScreens {
    fn screen(&mut self, alternate: bool) -> &mut KittyKeyboard {
        if alternate {
            &mut self.alternate
        } else {
            &mut self.main
        }
    }

    pub(crate) fn flags(&self, alternate: bool) -> u8 {
        if alternate {
            self.alternate.flags()
        } else {
            self.main.flags()
        }
    }

    pub(crate) fn set(&mut self, alternate: bool, requested: i32, mode: &SetMode) {
        self.screen(alternate).set(requested, mode);
    }

    pub(crate) fn push(&mut self, alternate: bool, requested: i32) {
        self.screen(alternate).push(requested);
    }

    pub(crate) fn pop(&mut self, alternate: bool, count: i32) {
        self.screen(alternate).pop(count);
    }

    /// `CSI > 4 m` and `CSI > 4 ; m`. Those clear `EXTENDED_KEY_MODES`, which
    /// includes the Kitty bit, so the flags have to go with it — a query that
    /// still answered `1` would promise a disambiguation the pane is no longer
    /// in, and the next Kitty request would quietly take the xterm mode the
    /// application just asked for away again. The stack survives: a later pop
    /// is a Kitty request, and those own this bit.
    pub(crate) fn clear_current(&mut self, alternate: bool) {
        self.screen(alternate).flags = 0;
    }

    /// The alternate screen negotiates from nothing, every time it is entered
    /// or left, so nothing follows an editor in or out.
    pub(crate) fn reset_alternate(&mut self) {
        self.alternate = KittyKeyboard::default();
    }

    /// RIS. A reset that left saves on the stack would let a later pop restore
    /// a mode no application ever asked for.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The flags in force on one screen, and what has been pushed there.
#[derive(Debug, Default)]
struct KittyKeyboard {
    flags: u8,
    stack: VecDeque<u8>,
}

impl KittyKeyboard {
    /// What an application gets, which is only ever what rmux implements.
    fn flags(&self) -> u8 {
        self.flags
    }

    /// `CSI = flags ; mode u`.
    fn set(&mut self, requested: i32, mode: &SetMode) {
        let flags = supported(requested);
        self.flags = match mode {
            SetMode::Replace => flags,
            SetMode::Add => self.flags | flags,
            SetMode::Remove => self.flags & !flags,
        };
    }

    /// `CSI > flags u`. A full stack loses its oldest entry, never this one.
    fn push(&mut self, requested: i32) {
        if self.stack.len() == STACK_MAX {
            self.stack.pop_front();
        }
        self.stack.push_back(self.flags);
        self.flags = supported(requested);
    }

    /// `CSI < number u`. Popping past the last save resets every flag, which
    /// is the specification's answer to an application that pops too far.
    fn pop(&mut self, count: i32) {
        for _ in 0..count.max(0) {
            self.flags = self.stack.pop_back().unwrap_or(0);
        }
    }
}

/// What is left of a requested flag set once the unimplemented bits go.
fn supported(requested: i32) -> u8 {
    // the mask is one bit wide, so the result always fits
    (requested.max(0) & i32::from(SUPPORTED)) as u8
}

/// Put a pane's Kitty flags in force on its screen, leaving the xterm
/// extended-key modes alone: an application that never spoke Kitty may have
/// enabled one of those.
pub(super) fn apply<W: ScreenWriter + ?Sized>(kitty: &KittyScreens, writer: &mut W) {
    if kitty.flags(writer.is_alternate()) & SUPPORTED != 0 {
        writer.mode_set(mode::MODE_KEYS_KITTY);
    } else {
        writer.mode_clear(mode::MODE_KEYS_KITTY);
    }
}
