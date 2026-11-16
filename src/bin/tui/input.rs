//! Keyboard and paste: global keys, popups, the search line, then the action
//! registry, then movement.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
	actions::{self, Avail, Ctx},
	app::{App, Screen},
	dialogs::Modal,
};

/// Rows a page key moves.
const PAGE: usize = 15;

impl App {
	pub fn on_event(
		&mut self,
		event: Event,
	) {
		match event {
			Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
			Event::Mouse(mouse) if self.mouse => self.on_mouse(mouse),
			Event::Paste(text) => self.on_paste(&text),
			_ => {},
		}
	}

	pub fn on_key(
		&mut self,
		key: KeyEvent,
	) {
		self.clear_error();
		let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
		// quitting works everywhere, but asks when the offer has unsaved changes
		if ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c')) {
			let unsaved = self.offer.as_ref().is_some_and(|e| e.dirty)
				|| self.template.as_ref().is_some_and(|t| t.dirty());
			if matches!(self.modal, Some(Modal::Dialog(_))) && unsaved {
				// the question is already open
				return;
			}
			self.modal = None;
			self.request_quit();
			return;
		}
		if self.modal.is_some() {
			self.on_modal_key(key);
			return;
		}
		if self.searching && self.on_search_key(&key) {
			return;
		}
		if self.screen == Screen::Template
			&& self.template.as_ref().is_some_and(|t| t.searching)
			&& self.on_template_search_key(&key)
		{
			return;
		}
		let ctx = self.ctx();
		if ctx == Ctx::Empty && self.on_first_run_key(&key) {
			return;
		}
		if let Some(action) = actions::for_key(ctx, &key) {
			if action.availability(self) != Avail::Hidden {
				self.run(action);
				return;
			}
		}
		self.on_move_key(&key);
	}

	/// Typing in the search line. Returns `false` for keys that should do their
	/// usual thing.
	fn on_search_key(
		&mut self,
		key: &KeyEvent,
	) -> bool {
		match key.code {
			// like a quick-open box: Enter opens the highlighted project, the filter stays
			KeyCode::Enter => {
				self.searching = false;
				if !self.rows.is_empty() {
					self.open_selected();
				}
			},
			KeyCode::Esc => {
				self.searching = false;
				self.search.clear();
				self.refresh_rows();
			},
			// moving in the list ends typing, the filter stays
			KeyCode::Up
			| KeyCode::Down
			| KeyCode::PageUp
			| KeyCode::PageDown
			| KeyCode::Tab
			| KeyCode::BackTab => {
				self.searching = false;
				return false;
			},
			_ => {
				if !self.search.on_key(key) {
					return false;
				}
				self.refresh_rows();
			},
		}
		true
	}

	/// Typing in the search line of the Template tab, like the project search.
	fn on_template_search_key(
		&mut self,
		key: &KeyEvent,
	) -> bool {
		let Some(editor) = self.template.as_mut() else {
			return false;
		};
		match key.code {
			KeyCode::Enter => {
				editor.searching = false;
				if !editor.rows().is_empty() {
					self.run(actions::Action::ChangeLine);
				}
			},
			KeyCode::Esc => {
				editor.searching = false;
				editor.search.clear();
				let any = !editor.rows().is_empty();
				editor.table.select(any.then_some(0));
			},
			KeyCode::Up
			| KeyCode::Down
			| KeyCode::PageUp
			| KeyCode::PageDown
			| KeyCode::Tab
			| KeyCode::BackTab => {
				editor.searching = false;
				return false;
			},
			_ => {
				if !editor.search.on_key(key) {
					return false;
				}
				let any = !editor.rows().is_empty();
				editor.table.select(any.then_some(0));
			},
		}
		true
	}

	/// The buttons of the start page: ←/→/Tab move, Enter presses.
	fn on_first_run_key(
		&mut self,
		key: &KeyEvent,
	) -> bool {
		let count = self.first_run_buttons().len();
		match key.code {
			KeyCode::Left | KeyCode::BackTab | KeyCode::Up => {
				self.first_run_focus = (self.first_run_focus + count - 1) % count
			},
			KeyCode::Right | KeyCode::Tab | KeyCode::Down => {
				self.first_run_focus = (self.first_run_focus + 1) % count
			},
			KeyCode::Enter | KeyCode::Char(' ') => {
				let focus = self.first_run_focus.min(count - 1);
				self.press_first_run(focus);
			},
			_ => return false,
		}
		true
	}

	/// Pasted text goes into the text field that has the focus.
	fn on_paste(
		&mut self,
		text: &str,
	) {
		match self.modal.as_mut() {
			Some(Modal::Form(form)) => {
				if let Some(field) = form.fields.get_mut(form.focus).filter(|f| f.is_text()) {
					field.input.insert_str(text);
					field.error = None;
				}
			},
			Some(Modal::Palette(palette)) => {
				palette.input.insert_str(text);
				palette.selected = 0;
			},
			Some(Modal::Typed(typed)) => {
				typed.input.insert_str(text);
				typed.focus = 0;
			},
			Some(_) => {},
			None if self.screen == Screen::Template => {
				if let Some(editor) = self.template.as_mut() {
					editor.searching = true;
					editor.search.insert_str(text);
					let any = !editor.rows().is_empty();
					editor.table.select(any.then_some(0));
				}
			},
			None if self.screen == Screen::Projects => {
				self.searching = true;
				self.search.insert_str(text);
				self.refresh_rows();
			},
			None => {},
		}
	}

	/// Arrows, page keys and j/k: move in the list or scroll the page.
	fn on_move_key(
		&mut self,
		key: &KeyEvent,
	) {
		match self.screen {
			Screen::Dashboard => {
				let count = self.tasks.len();
				let selected = self.task_state.selected().unwrap_or(0);
				if let Some(next) = move_index(selected, count, key) {
					self.task_state.select(Some(next));
				}
			},
			Screen::Projects => {
				let selected = self.table.selected().unwrap_or(0);
				if let Some(next) = move_index(selected, self.rows.len(), key) {
					self.table.select(Some(next));
				}
			},
			Screen::Detail => {
				let scroll = &mut self.detail_scroll;
				match key.code {
					// with the panel beside the page, ↑/↓ choose the action; j/k and the page keys scroll
					KeyCode::Up if self.wide => self.move_panel(-1),
					KeyCode::Down if self.wide => self.move_panel(1),
					KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
					KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
					KeyCode::PageDown | KeyCode::Char(' ') => *scroll = scroll.saturating_add(PAGE as u16),
					KeyCode::PageUp => *scroll = scroll.saturating_sub(PAGE as u16),
					KeyCode::Home => *scroll = 0,
					KeyCode::End => *scroll = u16::MAX,
					_ => {},
				}
			},
			// the log counts from the newest line at the bottom
			Screen::Log => {
				let scroll = &mut self.log_scroll;
				match key.code {
					KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_add(1),
					KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_sub(1),
					KeyCode::PageUp => *scroll = scroll.saturating_add(PAGE as u16),
					KeyCode::PageDown => *scroll = scroll.saturating_sub(PAGE as u16),
					KeyCode::Home => *scroll = u16::MAX,
					KeyCode::End => *scroll = 0,
					_ => {},
				}
			},
			Screen::Template => {
				let Some(editor) = self.template.as_mut() else {
					return;
				};
				let selected = editor.table.selected().unwrap_or(0);
				if let Some(next) = move_index(selected, editor.rows().len(), key) {
					editor.table.select(Some(next));
				}
			},
			Screen::Offer => {
				let Some(editor) = self.offer.as_mut() else {
					return;
				};
				let selected = editor.table.selected().unwrap_or(0);
				if let Some(next) = move_index(selected, editor.offer.lines.len(), key) {
					editor.table.select(Some(next));
				}
			},
		}
		// unknown keys on the project page or in lists: a hint instead of nothing
		if let KeyCode::Char(c) = key.code {
			if !matches!(c, 'j' | 'k' | ' ')
				&& !key.modifiers.contains(KeyModifiers::CONTROL)
				&& self.modal.is_none()
			{
				let palette = actions::Key::ctrl('p').label();
				self.notify(lformat!(
					"{} does nothing here; ? shows the keys, {} lists all commands",
					c,
					palette
				));
			}
		}
	}
}

/// The new position after a movement key in a list of `count` rows, `None` for
/// other keys.
pub fn move_index(
	selected: usize,
	count: usize,
	key: &KeyEvent,
) -> Option<usize> {
	if count == 0 {
		return None;
	}
	let last = count - 1;
	Some(match key.code {
		KeyCode::Down | KeyCode::Char('j') => (selected + 1).min(last),
		KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
		KeyCode::PageDown => (selected + PAGE).min(last),
		KeyCode::PageUp => selected.saturating_sub(PAGE),
		KeyCode::Home => 0,
		KeyCode::End => last,
		_ => return None,
	})
}
