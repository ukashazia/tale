use super::*;

impl App {
    pub(super) fn update_input(&mut self, input: InputEvent) -> Vec<Effect> {
        if self.interactive_handoff_active {
            return Vec::new();
        }
        match input {
            InputEvent::Resize { width, height } => {
                self.set_terminal_size(width, height);
                Vec::new()
            }
            InputEvent::Mouse(mouse) => self.handle_mouse(mouse),
            InputEvent::Paste(text) => self.handle_paste(&text),
            InputEvent::FocusGained | InputEvent::FocusLost => Vec::new(),
            InputEvent::Key(key) => self.handle_key(key),
        }
    }

    /// The frame geometry the current terminal size produces, so the reducer
    /// paths that need a rectangle agree with what was last drawn.
    pub(super) fn frame_layout(&self) -> crate::ui::layout::FrameLayout {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: self.terminal_width,
            height: self.terminal_height,
        };
        crate::ui::layout::compute(area, self)
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<Effect> {
        if !self.resolved_config.ui.mouse {
            return Vec::new();
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            let area = ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            };
            // The same rows the footer was drawn from, so a click lands on the
            // hint the user actually sees.
            let footer = crate::ui::components::interaction_shell::footer_rows(self, area.width);
            let layout = crate::ui::layout::compute_with_footer(area, self, &footer);
            if !matches!(self.interaction, InteractionMode::Normal) {
                return self.handle_interaction_mouse(mouse, layout.footer);
            }
            if self.resolved_config.ui.show_footer
                && contains_point(layout.footer, mouse.column, mouse.row)
            {
                for (row, hints) in footer.iter().enumerate() {
                    let y = layout
                        .footer
                        .y
                        .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
                    let mut x = layout.footer.x;
                    for hint in hints {
                        let end = x.saturating_add(u16::try_from(hint.width()).unwrap_or(u16::MAX));
                        if mouse.row == y && mouse.column >= x && mouse.column < end {
                            return self.dispatch_action(hint.action_id);
                        }
                        x = end.saturating_add(2);
                    }
                }
                return Vec::new();
            }
        }
        let action = match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if self.focus == Focus::Collection
                    && self.mouse_in_scrollable_collection(mouse.column, mouse.row) =>
            {
                match mouse.kind {
                    MouseEventKind::ScrollUp => Some(ActionId::CollectionMoveUp),
                    MouseEventKind::ScrollDown => Some(ActionId::CollectionMoveDown),
                    _ => None,
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus_mouse_region(mouse.column, mouse.row);
                None
            }
            _ => None,
        };
        action.map_or_else(Vec::new, |action_id| {
            if self.action_available_for_id(action_id) {
                self.dispatch_action(action_id)
            } else {
                Vec::new()
            }
        })
    }

    pub(super) fn handle_interaction_mouse(
        &mut self,
        mouse: MouseEvent,
        area: ratatui::layout::Rect,
    ) -> Vec<Effect> {
        if !contains_point(area, mouse.column, mouse.row) {
            return self.handle_interaction_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        if let InteractionMode::Transient(state) = &self.interaction
            && state.kind == TransientKind::Action
        {
            let action_id = crate::ui::components::interaction_shell::action_menu_action_at(
                self,
                state,
                area,
                mouse.column,
                mouse.row,
            );
            let Some(action_id) = action_id else {
                return Vec::new();
            };
            if let Some(reason) = self.action_unavailable_reason(action_id) {
                if let InteractionMode::Transient(state) = &mut self.interaction {
                    state.message = Some(reason);
                }
                return Vec::new();
            }
            self.interaction = InteractionMode::Normal;
            return self.dispatch_action(action_id);
        }
        if let InteractionMode::FilterLine(state) = &self.interaction {
            let insertion = crate::ui::components::interaction_shell::filter_suggestion_at(
                self,
                state,
                area,
                mouse.column,
                mouse.row,
            )
            .and_then(|index| {
                state
                    .suggestions()
                    .nth(index)
                    .map(|suggestion| suggestion.insertion.clone())
            });
            if let (Some(insertion), InteractionMode::FilterLine(state)) =
                (insertion, &mut self.interaction)
            {
                let (start, end) = active_token(&state.editor.input, state.editor.cursor);
                state.editor.input.replace_range(start..end, &insertion);
                state.editor.cursor = start.saturating_add(insertion.len());
                state.selected_completion = None;
            }
            return self.update_live_filter();
        }
        let clicked_route = match &mut self.interaction {
            InteractionMode::CommandLine(state) => {
                crate::ui::components::interaction_shell::navigation_route_at(
                    state,
                    area,
                    mouse.column,
                    mouse.row,
                )
            }
            InteractionMode::Transient(state) => {
                if !matches!(state.kind, TransientKind::Copy | TransientKind::Choice) {
                    return Vec::new();
                }
                let key = if state.kind == TransientKind::Choice {
                    crate::ui::components::interaction_shell::choice_menu_key_at(
                        state,
                        area,
                        mouse.column,
                        mouse.row,
                    )
                } else {
                    crate::ui::components::interaction_shell::copy_menu_field_at(
                        state,
                        area,
                        mouse.column,
                        mouse.row,
                    )
                };
                if let Some(key) = key {
                    return self.handle_transient_key(KeyEvent::new(
                        KeyCode::Char(key),
                        KeyModifiers::NONE,
                    ));
                }
                None
            }
            InteractionMode::Normal
            | InteractionMode::HelpSheet
            | InteractionMode::FilterLine(_) => None,
        };
        if let Some(route) = clicked_route {
            return self.open_navigation_route(route);
        }
        Vec::new()
    }

    /// Which keys are live right now. This is the single answer: key dispatch,
    /// the footer, and contextual help all read it, so they cannot disagree.
    pub fn action_context(&self) -> ActionContext {
        match self.current_route() {
            Route::Local if self.views.local.section == LocalSection::Accounts => {
                ActionContext::Collection
            }
            Route::Local | Route::Dns | Route::Access | Route::Diagnostics => ActionContext::Detail,
            Route::Audit if self.focus != Focus::Inspector => ActionContext::Audit,
            Route::Overview
            | Route::Devices
            | Route::Services
            | Route::Tasks
            | Route::Profiles
            | Route::Users
            | Route::Routes
            | Route::Credentials
            | Route::Audit
                if matches!(self.focus, Focus::Inspector) =>
            {
                ActionContext::Detail
            }
            Route::Overview
            | Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Credentials
            | Route::Profiles
            | Route::Config
            | Route::Services
            | Route::Tasks => ActionContext::Collection,
            _ => ActionContext::Root,
        }
    }

    pub(super) fn focus_mouse_region(&mut self, column: u16, row: u16) {
        if self.current_route() == Route::Overview {
            self.focus_overview_mouse_region(column, row);
            return;
        }
        if self.current_route() == Route::Audit {
            let frame = self.frame_layout();
            if frame
                .inspector
                .is_some_and(|inspector| contains_point(inspector, column, row))
            {
                self.focus = Focus::Inspector;
                return;
            }
            self.focus = Focus::Collection;
            let Some(collection) = self.audit_event_area() else {
                return;
            };
            if !contains_point(collection, column, row) {
                return;
            }
            let first_row = collection.y.saturating_add(2);
            let row_count = usize::from(collection.height.saturating_sub(3));
            if row >= first_row && usize::from(row.saturating_sub(first_row)) < row_count {
                let position = usize::from(row.saturating_sub(first_row));
                if position < self.audit_event_count() {
                    self.admin_activity_selected = position;
                }
            }
            return;
        }
        if self.current_route() != Route::Devices {
            let frame = self.frame_layout();
            if frame.minimum {
                return;
            }
            if matches!(
                self.current_route(),
                Route::Routes | Route::Credentials | Route::Services | Route::Tasks
            ) && frame
                .inspector
                .is_some_and(|inspector| contains_point(inspector, column, row))
            {
                self.focus = Focus::Inspector;
                return;
            }
            self.focus = Focus::Collection;
            let area = frame
                .inspector
                .map_or(frame.content, |inspector| ratatui::layout::Rect {
                    x: frame.content.x,
                    y: frame.content.y,
                    width: inspector.x.saturating_sub(frame.content.x),
                    height: frame.content.height,
                });
            if !contains_point(area, column, row) {
                return;
            }
            let first_row = match self.current_route() {
                Route::Users | Route::Profiles => area.y.saturating_add(1),
                // A border and a heading row sit above the first task.
                Route::Routes | Route::Credentials | Route::Tasks | Route::Config => {
                    area.y.saturating_add(2)
                }
                Route::Local if self.views.local.section == LocalSection::Accounts => {
                    area.y.saturating_add(3)
                }
                Route::Services => area.y.saturating_add(3),
                _ => return,
            };
            if row < first_row {
                return;
            }
            let position = row.saturating_sub(first_row);
            match self.current_route() {
                Route::Users => {
                    let length = self.filtered_admin_users().len();
                    if usize::from(position) < length {
                        self.admin_user_selected = usize::from(position);
                    }
                }
                Route::Routes => {
                    let length = self.filtered_admin_routes().len();
                    if usize::from(position) < length {
                        self.admin_route_selected = usize::from(position);
                    }
                }
                Route::Profiles => {
                    let length = self.profile_rows().len();
                    if usize::from(position) < length {
                        self.views.profiles.selected = usize::from(position);
                    }
                }
                Route::Config => {
                    let length = self.config_rows().len();
                    if usize::from(position) < length {
                        self.views.config.selected = usize::from(position);
                    }
                }
                Route::Credentials => {
                    let length = self.filtered_admin_credentials().len();
                    if usize::from(position) < length {
                        self.admin_credential_selected = usize::from(position);
                    }
                }
                Route::Local if usize::from(position) < self.local_accounts.len() => {
                    self.views.local.selected = usize::from(position);
                    self.views.local.scroll = self.views.local.selected;
                }
                Route::Services if usize::from(position) < self.service_row_count() => {
                    self.views.services.selected = usize::from(position);
                }
                // The table shows a window over the history, so the row under
                // the pointer is an offset from wherever that window starts.
                Route::Tasks => {
                    let count = self.filtered_task_count();
                    let visible = usize::from(area.height.saturating_sub(3)).max(1);
                    let start =
                        crate::ui::views::tasks::window_start(self.task_cursor(), count, visible);
                    let index = start.saturating_add(usize::from(position));
                    if index < count {
                        self.tasks
                            .select_filtered_position(&self.task_filter, index);
                    }
                }
                _ => {}
            }
            return;
        }
        let frame = self.frame_layout();
        if let Some(inspector) = frame.inspector
            && contains_point(inspector, column, row)
        {
            self.reset_device_detail_state();
            self.focus = Focus::Inspector;
            return;
        }
        let Some(collection) = self.device_collection_area(frame) else {
            self.focus = Focus::Collection;
            return;
        };
        if !contains_point(collection, column, row) {
            self.focus = Focus::Collection;
            return;
        }
        self.focus = Focus::Collection;
        let first_row = collection.y.saturating_add(2);
        let row_count = usize::from(collection.height.saturating_sub(3));
        if row >= first_row && usize::from(row.saturating_sub(first_row)) < row_count {
            let position = self
                .views
                .devices
                .scroll
                .saturating_add(usize::from(row.saturating_sub(first_row)));
            self.move_selection_to(position);
        }
    }

    pub(super) fn mouse_in_scrollable_collection(&self, column: u16, row: u16) -> bool {
        if self.current_route() == Route::Overview {
            return self
                .overview_collection_area()
                .is_some_and(|area| contains_point(area, column, row));
        }
        if self.current_route() == Route::Devices {
            let frame = self.frame_layout();
            return self
                .device_collection_area(frame)
                .is_some_and(|area| contains_point(area, column, row));
        }
        if self.current_route() == Route::Audit {
            return self
                .audit_event_area()
                .is_some_and(|area| contains_point(area, column, row));
        }
        if !matches!(
            self.current_route(),
            Route::Users
                | Route::Routes
                | Route::Credentials
                | Route::Profiles
                | Route::Services
                | Route::Tasks
        ) {
            return false;
        }
        let frame = self.frame_layout();
        if frame.minimum {
            return false;
        }
        let area = frame
            .inspector
            .map_or(frame.content, |inspector| ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            });
        contains_point(area, column, row)
    }

    pub(super) fn focus_overview_mouse_region(&mut self, column: u16, row: u16) {
        let Some(collection) = self.overview_collection_area() else {
            return;
        };
        if collection.width < self.terminal_width
            && row >= collection.y
            && column >= collection.x.saturating_add(collection.width)
        {
            self.focus = Focus::Inspector;
            return;
        }
        self.focus = Focus::Collection;
        if !contains_point(collection, column, row) {
            return;
        }
        let first_row = collection.y.saturating_add(2);
        if row < first_row {
            return;
        }
        let viewport = usize::from(collection.height.saturating_sub(3)).max(1);
        let selected = self
            .selected_overview_finding()
            .and_then(|selected| {
                self.health_findings
                    .iter()
                    .position(|finding| finding.id == selected.id)
            })
            .unwrap_or(0);
        let start = selected
            .saturating_add(1)
            .saturating_sub(viewport)
            .min(self.health_findings.len().saturating_sub(1));
        let position = start.saturating_add(usize::from(row.saturating_sub(first_row)));
        if position < self.health_findings.len() {
            self.select_overview_position(position);
        }
    }

    pub(super) fn overview_collection_area(&self) -> Option<ratatui::layout::Rect> {
        if self.focus == Focus::Inspector {
            return None;
        }
        let frame = self.frame_layout();
        if frame.minimum {
            return None;
        }
        let source_height = if frame.content.width >= 110 { 5 } else { 6 }
            .min(frame.content.height.saturating_sub(3));
        let mut collection = ratatui::layout::Rect {
            x: frame.content.x,
            y: frame.content.y.saturating_add(source_height),
            width: frame.content.width,
            height: frame.content.height.saturating_sub(source_height),
        };
        if frame.content.width >= 110 {
            collection.width = collection.width.saturating_mul(60) / 100;
        }
        Some(collection)
    }

    /// The audit collection uses the whole pane on compact terminals. On a
    /// wide terminal it yields the right side either to delivery status or to
    /// the selected event's inspector.
    pub(super) fn audit_event_area(&self) -> Option<ratatui::layout::Rect> {
        if self.focus == Focus::Inspector {
            return None;
        }
        let frame = self.frame_layout();
        if frame.minimum {
            return None;
        }
        if let Some(inspector) = frame.inspector {
            return Some(ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            });
        }
        if frame.content.width < 110 {
            return Some(frame.content);
        }
        Some(ratatui::layout::Rect {
            width: frame.content.width.saturating_mul(60) / 100,
            ..frame.content
        })
    }

    pub(super) fn device_collection_area(
        &self,
        frame: crate::ui::layout::FrameLayout,
    ) -> Option<ratatui::layout::Rect> {
        if frame.minimum || self.focus == Focus::Inspector {
            return None;
        }
        Some(match frame.inspector {
            Some(inspector) => ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            },
            None => frame.content,
        })
    }

    pub(super) fn handle_paste(&mut self, text: &str) -> Vec<Effect> {
        match &mut self.interaction {
            InteractionMode::CommandLine(state) => {
                insert_text(&mut state.editor, text);
                state.error = None;
                self.refresh_command_completions();
                return Vec::new();
            }
            InteractionMode::FilterLine(state) => {
                let detail_search = matches!(state.purpose, FilterLinePurpose::DetailSearch { .. });
                insert_text(&mut state.editor, text);
                return if detail_search {
                    self.update_detail_search_preview();
                    Vec::new()
                } else {
                    self.update_live_filter()
                };
            }
            _ => {}
        }
        let Some(overlay) = self.overlays.last_mut() else {
            return Vec::new();
        };
        match overlay {
            Overlay::Form(state) => {
                state.error = None;
                if !state.is_editing() {
                    return Vec::new();
                }
                if let Some(list) = state.list.as_mut() {
                    list.edit(|entry| entry.push_str(text));
                } else if state.selected_field().is_some_and(FormField::is_secret) {
                    if let Some(secret) = state.secret.as_mut() {
                        secret.push_str(text);
                    }
                } else if let cursor = state.cursor
                    && let Some(field) = state.selected_field_mut()
                    && field.is_text()
                {
                    field.value.insert_str(cursor, text);
                    state.cursor = cursor.saturating_add(text.len());
                }
            }
            Overlay::Confirmation(state) => {
                state.input.push_str(text);
                state.error = None;
            }
            _ => {}
        }
        Vec::new()
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if let Some(effect) = self.handle_text_key(key) {
            return effect;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if !self.overlays.is_empty() {
                return self.pop_overlay();
            }
            if !matches!(self.interaction, InteractionMode::Normal) {
                return self
                    .handle_interaction_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
            let effects = self.cancel_focused_task();
            if !effects.is_empty() {
                return effects;
            }
            return self.request_shutdown(ShutdownReason::UserQuit);
        }
        if !self.overlays.is_empty() {
            if key.code == KeyCode::Esc {
                return self.pop_overlay();
            }
            return self.handle_overlay_key(key);
        }
        if !matches!(self.interaction, InteractionMode::Normal) {
            return self.handle_interaction_key(key);
        }
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            return self.handle_quit_key();
        }
        if key.code == KeyCode::Esc {
            if self.focus == Focus::Inspector {
                self.close_collection_inspector();
            }
            return Vec::new();
        }
        let context = self.action_context();
        let Some(action_id) = action::action_for_key(key, context) else {
            return Vec::new();
        };
        self.dispatch_action(action_id)
    }

    pub(super) fn handle_interaction_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match &self.interaction {
            InteractionMode::CommandLine(_) => self.handle_command_line_key(key),
            InteractionMode::FilterLine(_) => self.handle_filter_line_key(key),
            InteractionMode::Transient(_) => self.handle_transient_key(key),
            InteractionMode::HelpSheet => self.handle_help_sheet_key(key),
            InteractionMode::Normal => Vec::new(),
        }
    }

    pub(super) fn handle_command_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            return self.accept_navigation();
        }
        let edited = if let InteractionMode::CommandLine(state) = &mut self.interaction {
            edit_line(&mut state.editor, key)
        } else {
            false
        };
        if edited {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = None;
            }
            self.refresh_command_completions();
        }
        Vec::new()
    }

    pub(super) fn handle_filter_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            let detail_restoration = match &self.interaction {
                InteractionMode::FilterLine(FilterLineState {
                    purpose:
                        FilterLinePurpose::DetailSearch {
                            route,
                            scroll,
                            query,
                            match_line,
                        },
                    ..
                }) => Some((*route, *scroll, query.clone(), *match_line)),
                _ => None,
            };
            if let Some((route, scroll, query, match_line)) = detail_restoration {
                if route == Route::Devices {
                    self.views.devices.detail_scroll = scroll;
                    self.views.devices.detail_search = query;
                    self.views.devices.detail_search_match = match_line;
                } else {
                    self.detail_scroll = scroll;
                    self.detail_search = query;
                    self.detail_search_match = match_line;
                }
                self.interaction = InteractionMode::Normal;
                return Vec::new();
            }
            let restoration = match &self.interaction {
                InteractionMode::FilterLine(state) => Some(state.restoration.clone()),
                _ => None,
            };
            if let Some(restoration) = restoration {
                if self.current_route() == Route::Tasks {
                    self.task_filter = restoration.task_filter;
                    self.tasks.selected = restoration.task_selection;
                } else if matches!(
                    self.current_route(),
                    Route::Users | Route::Routes | Route::Credentials | Route::Audit
                ) {
                    self.set_simple_collection_filter(
                        restoration.input,
                        restoration.collection_selection,
                    );
                } else if self.current_route() == Route::Profiles {
                    self.views.profiles.filter = restoration.input;
                    self.views.profiles.selected = restoration.profile_selection;
                } else if self.current_route() == Route::Config {
                    self.views.config.filter = restoration.input;
                    self.views.config.selected = restoration.config_selection;
                } else if self.current_route() == Route::Services {
                    self.views.services.filter_draft = restoration.input;
                    self.views.services.applied_filter = restoration.expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.views.devices.filter_draft = restoration.input;
                    self.views.devices.applied_filter = restoration.expression;
                    self.views.devices.selected_id = restoration.selection;
                    self.views.devices.scroll = restoration.scroll;
                    self.reconcile_selection(None);
                }
            }
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            if matches!(
                self.interaction,
                InteractionMode::FilterLine(FilterLineState {
                    purpose: FilterLinePurpose::DetailSearch { .. },
                    ..
                })
            ) {
                let valid = matches!(
                    &self.interaction,
                    InteractionMode::FilterLine(state) if state.error.is_none()
                );
                if valid {
                    self.interaction = InteractionMode::Normal;
                    self.clamp_device_detail_scroll();
                }
                return Vec::new();
            }
            let (input, valid) = match &self.interaction {
                InteractionMode::FilterLine(state) => {
                    (state.editor.input.clone(), state.error.is_none())
                }
                _ => (String::new(), false),
            };
            if valid {
                return self.accept_filter(&input);
            }
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.complete_filter(key.code == KeyCode::BackTab);
            return self.update_live_filter();
        }
        let edited = if let InteractionMode::FilterLine(state) = &mut self.interaction {
            edit_line(&mut state.editor, key)
        } else {
            false
        };
        if edited {
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.selected_completion = None;
            }
            if matches!(
                self.interaction,
                InteractionMode::FilterLine(FilterLineState {
                    purpose: FilterLinePurpose::DetailSearch { .. },
                    ..
                })
            ) {
                self.update_detail_search_preview();
                return Vec::new();
            }
            return self.update_live_filter();
        }
        Vec::new()
    }

    pub(super) fn handle_transient_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            if let InteractionMode::Transient(state) = &mut self.interaction
                && state.prefix.is_some()
            {
                state.prefix = None;
                state.message = None;
                return Vec::new();
            }
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Char('?') && key.modifiers.is_empty() {
            self.interaction = InteractionMode::HelpSheet;
            return Vec::new();
        }
        let KeyCode::Char(character) = key.code else {
            return Vec::new();
        };
        if !key.modifiers.is_empty() {
            return Vec::new();
        }
        let (kind, actions, fields, addresses, choices, prefix) = match &self.interaction {
            InteractionMode::Transient(state) => (
                state.kind.clone(),
                state.actions.clone(),
                state.fields.clone(),
                state.addresses.clone(),
                state.choices.clone(),
                state.prefix,
            ),
            _ => return Vec::new(),
        };
        let mut sequence = String::new();
        if let Some(prefix) = prefix {
            sequence.push(prefix);
        }
        sequence.push(character);
        match kind {
            TransientKind::Action => {
                if let Some(action_id) = actions.iter().copied().find(|id| {
                    action::transient_sequence(*id).is_some_and(|value| value == sequence)
                }) {
                    if let Some(reason) = self.action_unavailable_reason(action_id) {
                        if let InteractionMode::Transient(state) = &mut self.interaction {
                            state.message = Some(reason);
                        }
                        return Vec::new();
                    }
                    self.interaction = InteractionMode::Normal;
                    return self.dispatch_action(action_id);
                }
                if prefix.is_none()
                    && actions.iter().any(|id| {
                        action::transient_sequence(*id)
                            .is_some_and(|value| value.len() == 2 && value.starts_with(character))
                    })
                {
                    if let InteractionMode::Transient(state) = &mut self.interaction {
                        state.prefix = Some(character);
                        state.message = None;
                    }
                    return Vec::new();
                }
            }
            TransientKind::Choice => {
                if let Some(choice) = choices
                    .iter()
                    .find(|choice| choice.sequence == sequence)
                    .cloned()
                {
                    self.interaction = InteractionMode::Normal;
                    return self.apply_choice(choice.outcome);
                }
                if prefix.is_none()
                    && choices.iter().any(|choice| {
                        choice.sequence.chars().count() == 2
                            && choice.sequence.starts_with(character)
                    })
                {
                    if let InteractionMode::Transient(state) = &mut self.interaction {
                        state.prefix = Some(character);
                        state.message = None;
                    }
                    return Vec::new();
                }
            }
            TransientKind::Copy => {
                // Inside the address level a digit picks one address.
                if prefix == Some(ADDRESS_PREFIX) {
                    if character == ADDRESS_PREFIX {
                        let effects = self.copy_text(addresses.join("\n"));
                        self.interaction = InteractionMode::Normal;
                        return effects;
                    }
                    if let Some(index) = character
                        .to_digit(10)
                        .and_then(|digit| usize::try_from(digit).ok())
                        .and_then(|digit| digit.checked_sub(1))
                        && let Some(address) = addresses.get(index)
                    {
                        let address = address.clone();
                        let effects = self.copy_text(address);
                        self.interaction = InteractionMode::Normal;
                        return effects;
                    }
                } else {
                    if character == copy_field_key(CopyField::Addresses) && addresses.len() > 1 {
                        // More than one address is a choice, not a single value.
                        if let InteractionMode::Transient(state) = &mut self.interaction {
                            state.prefix = Some(ADDRESS_PREFIX);
                            state.message = None;
                        }
                        return Vec::new();
                    }
                    if let Some(field) = fields
                        .iter()
                        .copied()
                        .find(|field| copy_field_key(*field) == character)
                    {
                        let effects = self.copy_field(field);
                        self.interaction = InteractionMode::Normal;
                        return effects;
                    }
                }
            }
        }
        if let InteractionMode::Transient(state) = &mut self.interaction {
            state.message = Some(format!("unknown key: {sequence}"));
            if state.kind == TransientKind::Action {
                state.prefix = None;
            }
        }
        Vec::new()
    }

    pub(super) fn handle_help_sheet_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        self.interaction = InteractionMode::Normal;
        self.handle_key(key)
    }

    pub(super) fn update_live_filter(&mut self) -> Vec<Effect> {
        let (input, cursor, anchored) = match &self.interaction {
            InteractionMode::FilterLine(state) => (
                state.editor.input.clone(),
                state.editor.cursor,
                state.selected_completion.is_some(),
            ),
            _ => return Vec::new(),
        };
        let generation = self.advance_completion_generation();
        // While `Tab` walks the tray the offered set stays anchored, so the row the
        // user is cycling through does not move underneath them.
        let sections = (!anchored).then(|| self.filter_suggestions(&input, cursor));
        if matches!(
            self.current_route(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit
        ) {
            self.set_simple_collection_filter(input, 0);
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Services
            && self.views.services.section != ServiceSection::Serve
        {
            self.views.services.filter_draft = input;
            self.views.services.applied_filter = FilterExpression::empty();
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Profiles {
            self.views.profiles.filter = input;
            self.views.profiles.selected = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Config {
            self.views.config.filter = input;
            self.views.config.selected = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Tasks {
            self.task_filter = input;
            self.tasks.select_filtered_first(&self.task_filter);
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        let parsed = filter::parse(&input, &self.filter_schema());
        match parsed {
            Ok(expression) => {
                if self.current_route() == Route::Services {
                    self.views.services.filter_draft = input;
                    self.views.services.applied_filter = expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.views.devices.filter_draft = input;
                    self.views.devices.applied_filter = expression;
                    self.reconcile_selection(None);
                }
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = None;
                    state.generation = generation;
                    if let Some(sections) = sections {
                        state.sections = sections;
                    }
                }
            }
            Err(error) => {
                // The last valid expression stays applied, so the rows behind the
                // prompt keep showing a real result while the term is repaired.
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(FilterErrorReport {
                        message: error.to_string(),
                        expected: error.expected,
                    });
                    state.generation = generation;
                    if let Some(sections) = sections {
                        state.sections = sections;
                    }
                }
            }
        }
        Vec::new()
    }

    pub(super) fn command_candidates(&self, input: &str) -> Vec<NavigationCandidate> {
        navigation_candidates(input.trim())
    }

    /// The filter vocabulary of the route the shell is currently showing.
    pub fn filter_schema(&self) -> FilterSchema {
        match self.current_route() {
            Route::Tasks => filter::tasks_schema(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit => {
                filter::collection_schema()
            }
            Route::Services => match self.views.services.section {
                ServiceSection::Serve => filter::service_schema(),
                ServiceSection::Taildrive | ServiceSection::Certificates => {
                    filter::collection_schema()
                }
            },
            Route::Diagnostics => filter::empty_schema(),
            Route::Profiles => filter::profiles_schema(),
            Route::Config => filter::config_schema(),
            _ => filter::device_schema(),
        }
    }

    pub(super) fn filter_suggestions(
        &self,
        input: &str,
        cursor: usize,
    ) -> Vec<FilterSuggestionSection> {
        let schema = self.filter_schema();
        let (start, end) = active_token(input, cursor);
        let token = input.get(start..end).map_or("", |value| value);
        match filter_stage(token, &schema) {
            FilterStage::Field { prefix, fragment } => field_sections(&schema, prefix, fragment),
            FilterStage::Value {
                spec,
                prefix,
                fragment,
            } => self.value_sections(spec, &prefix, fragment),
        }
    }

    pub(super) fn value_sections(
        &self,
        spec: &'static FilterFieldSpec,
        prefix: &str,
        fragment: &str,
    ) -> Vec<FilterSuggestionSection> {
        let values = match spec.value_kind {
            FilterValueKind::Enumeration(values) => {
                values.iter().map(|value| (*value).to_owned()).collect()
            }
            FilterValueKind::Duration => DURATION_SUGGESTIONS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            FilterValueKind::Snapshot => self.snapshot_values(spec.field),
        };
        let mut suggestions = rank(
            values.into_iter().map(|value| {
                let text = quote_value(&value);
                FilterSuggestion {
                    kind: FilterSuggestionKind::Value,
                    insertion: format!("{prefix}{text}"),
                    note: String::new(),
                    matches: Vec::new(),
                    score: 0,
                    text,
                }
            }),
            fragment,
        );
        suggestions.truncate(SNAPSHOT_VALUE_LIMIT);
        // Values lead: `Tab` should land on one, not on a match-mode refinement.
        let mut sections = Vec::new();
        if !suggestions.is_empty() {
            sections.push(FilterSuggestionSection {
                label: format!("{} values", spec.name),
                suggestions,
            });
        }
        let operators = spec
            .operators
            .iter()
            .copied()
            // Comparisons are offered as whole operands beside the values, so
            // only the match-mode refinements belong in their own section.
            .filter(|operator| *operator == filter::FilterOperator::StartsWith)
            .map(|operator| FilterSuggestion {
                kind: FilterSuggestionKind::Operator,
                text: operator.syntax().to_owned(),
                insertion: format!("{prefix}{}", operator.syntax()),
                note: operator.description().to_owned(),
                matches: Vec::new(),
                score: 0,
            })
            .collect::<Vec<_>>();
        if fragment.is_empty() && !operators.is_empty() {
            sections.push(FilterSuggestionSection {
                label: format!("{} operators", spec.name),
                suggestions: operators,
            });
        }
        sections
    }

    /// Deduplicated, deterministically ordered values already present in the snapshot.
    pub(super) fn snapshot_values(&self, field: FilterField) -> Vec<String> {
        let mut values = BTreeSet::new();
        // Mapping fields draw their suggestions from the mappings on screen,
        // not from the device list.
        if matches!(
            field,
            FilterField::Port | FilterField::Mount | FilterField::Backend
        ) {
            for mapping in self.services_snapshot.mappings() {
                let _ = match field {
                    FilterField::Port => values.insert(mapping.listener.port().to_string()),
                    FilterField::Mount => values.insert(mapping.mount.as_path().to_owned()),
                    _ => values.insert(mapping.backend.argument()),
                };
            }
            return values
                .into_iter()
                .filter(|value| !value.is_empty())
                .take(SNAPSHOT_VALUE_LIMIT)
                .collect();
        }
        for device in &self.devices_resource.snapshot {
            match field {
                FilterField::Id => {
                    let _ = values.insert(device.id.0.clone());
                }
                FilterField::Name => {
                    let _ = values.insert(device.display_name.clone());
                    let _ = values.insert(device.hostname.clone());
                }
                FilterField::Owner => {
                    if let Some(owner) = device.owner.clone() {
                        let _ = values.insert(owner);
                    }
                }
                FilterField::Os => {
                    let _ = values.insert(device.os.label().to_owned());
                }
                FilterField::Tag => values.extend(device.tags.iter().cloned()),
                FilterField::ClientVersion => {
                    if let Some(version) = device.version.clone() {
                        let _ = values.insert(version);
                    }
                }
                _ => {}
            }
        }
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .take(SNAPSHOT_VALUE_LIMIT)
            .collect()
    }

    pub(super) fn refresh_command_completions(&mut self) {
        let input = match &self.interaction {
            InteractionMode::CommandLine(state) => state.editor.input.clone(),
            _ => return,
        };
        let generation = self.advance_completion_generation();
        let candidates = self.command_candidates(&input);
        if let InteractionMode::CommandLine(state) = &mut self.interaction {
            state.candidates = candidates;
            state.generation = generation;
            state.error = state
                .candidates
                .is_empty()
                .then(|| "No matching view".to_owned());
        }
    }

    pub(super) fn advance_completion_generation(&mut self) -> u64 {
        self.next_completion_generation = self.next_completion_generation.saturating_add(1);
        self.next_completion_generation
    }

    /// `Tab` takes the best offer, then walks forward; `Shift+Tab` walks backward.
    /// A lone offer is accepted outright so the tray can move on to the next stage.
    pub(super) fn complete_filter(&mut self, reverse: bool) {
        let InteractionMode::FilterLine(state) = &mut self.interaction else {
            return;
        };
        let count = state.suggestion_count();
        if count == 0 {
            return;
        }
        let index = match (state.selected_completion, reverse) {
            (None, false) => 0,
            (None, true) => count.saturating_sub(1),
            (Some(current), false) => current.saturating_add(1) % count,
            (Some(0), true) => count.saturating_sub(1),
            (Some(current), true) => current.saturating_sub(1),
        };
        let Some(insertion) = state
            .suggestions()
            .nth(index)
            .map(|suggestion| suggestion.insertion.clone())
        else {
            return;
        };
        let (start, end) = active_token(&state.editor.input, state.editor.cursor);
        state.editor.input.replace_range(start..end, &insertion);
        state.editor.cursor = start.saturating_add(insertion.len());
        state.selected_completion = (count > 1).then_some(index);
    }

    pub(super) fn handle_text_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let overlay = self.overlays.last_mut()?;
        match overlay {
            Overlay::Form(state) => {
                // Two modes, and the same rule in both: Enter acts on what is
                // selected. Browsing, that means edit this field or submit;
                // editing, it means keep the value and stop editing.
                if state.is_editing() {
                    // An open list is a form of its own: entries are selected,
                    // reordered and typed into without leaving the field.
                    //
                    // Every binding here is one a terminal actually sends under
                    // the encoding this app asks for. Ctrl+I and Tab are the
                    // same byte, so Tab adds an entry; Ctrl with an arrow is not
                    // encoded at all, so the moves are plain control characters.
                    if let Some(list) = state.list.as_mut() {
                        let control = key.modifiers.contains(KeyModifiers::CONTROL);
                        match key.code {
                            KeyCode::Enter => state.commit_edit(),
                            KeyCode::Esc => state.abandon_edit(),
                            KeyCode::Up => list.select(-1),
                            KeyCode::Down => list.select(1),
                            KeyCode::Tab => list.insert(),
                            KeyCode::Char('p') if control => list.move_entry(-1),
                            KeyCode::Char('n') if control => list.move_entry(1),
                            KeyCode::Char('x') if control => list.remove(),
                            KeyCode::Backspace => list.edit(|entry| {
                                let _ = entry.pop();
                            }),
                            KeyCode::Char(character) if is_typed_text(key) => {
                                list.edit(|entry| entry.push(character));
                            }
                            _ => return None,
                        }
                        state.error = None;
                        return Some(Vec::new());
                    }
                    match key.code {
                        KeyCode::Enter => {
                            state.commit_edit();
                            state.error = None;
                            self.refresh_form_fields();
                            return Some(Vec::new());
                        }
                        KeyCode::Esc => state.abandon_edit(),
                        KeyCode::Left if state.selected_field().is_some_and(FormField::is_text) => {
                            if let Some(field) = state.selected_field() {
                                state.cursor = if key.modifiers.contains(KeyModifiers::ALT) {
                                    previous_word_boundary(&field.value, state.cursor)
                                } else {
                                    previous_scalar_boundary(&field.value, state.cursor)
                                };
                            }
                        }
                        KeyCode::Right
                            if state.selected_field().is_some_and(FormField::is_text) =>
                        {
                            if let Some(field) = state.selected_field() {
                                state.cursor = if key.modifiers.contains(KeyModifiers::ALT) {
                                    next_word_boundary(&field.value, state.cursor)
                                } else {
                                    next_scalar_boundary(&field.value, state.cursor)
                                };
                            }
                        }
                        KeyCode::Home if state.selected_field().is_some_and(FormField::is_text) => {
                            state.cursor = 0;
                        }
                        KeyCode::End if state.selected_field().is_some_and(FormField::is_text) => {
                            if let Some(field) = state.selected_field() {
                                state.cursor = field.value.len();
                            }
                        }
                        KeyCode::Left => {
                            if let Some(field) = state.selected_field_mut() {
                                field.cycle(false);
                            }
                        }
                        KeyCode::Right => {
                            if let Some(field) = state.selected_field_mut() {
                                field.cycle(true);
                            }
                        }
                        KeyCode::Backspace => {
                            if state.selected_field().is_some_and(FormField::is_secret) {
                                if let Some(secret) = state.secret.as_mut() {
                                    secret.pop();
                                }
                            } else if let cursor = state.cursor
                                && let Some(field) = state.selected_field_mut()
                                && field.is_text()
                            {
                                let previous = previous_scalar_boundary(&field.value, cursor);
                                field.value.replace_range(previous..cursor, "");
                                state.cursor = previous;
                            }
                        }
                        KeyCode::Char(character) if is_typed_text(key) => {
                            if state.selected_field().is_some_and(FormField::is_secret) {
                                if let Some(secret) = state.secret.as_mut() {
                                    secret.push(character);
                                }
                            } else if let cursor = state.cursor
                                && let Some(field) = state.selected_field_mut()
                            {
                                if field.is_text() {
                                    field.value.insert(cursor, character);
                                    state.cursor = cursor.saturating_add(character.len_utf8());
                                } else if character == ' ' {
                                    field.cycle(true);
                                }
                            }
                        }
                        _ => return None,
                    }
                    state.error = None;
                    return Some(Vec::new());
                }
                match key.code {
                    KeyCode::Enter => {
                        if state.on_submit_row() {
                            let state = state.clone();
                            return Some(self.accept_form(state));
                        }
                        // A field something else decides says so instead of
                        // opening an editor that could not change anything.
                        if let Some(reason) = state.locked_reason() {
                            state.error = Some(reason.to_owned());
                            return Some(Vec::new());
                        }
                        state.begin_edit();
                    }
                    KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                        state.move_selection(-1);
                    }
                    KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                        state.move_selection(1);
                    }
                    _ => return None,
                }
                state.error = None;
                Some(Vec::new())
            }
            Overlay::Confirmation(state) => {
                match key.code {
                    KeyCode::Char(character) if is_typed_text(key) => {
                        state.input.push(character);
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        let state = (**state).clone();
                        return Some(self.accept_confirmation(state));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            _ => None,
        }
    }

    pub(super) fn handle_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(overlay) = self.overlays.pop() else {
            return Vec::new();
        };
        match overlay {
            Overlay::QuitConfirmation => {
                if key.code == KeyCode::Enter
                    || (key.code == KeyCode::Char('y') && key.modifiers.is_empty())
                {
                    return self.request_shutdown(ShutdownReason::UserQuit);
                }
                if key.code == KeyCode::Char('n') && key.modifiers.is_empty() {
                    return Vec::new();
                }
                self.overlays.push(Overlay::QuitConfirmation);
                Vec::new()
            }
            Overlay::TaskInspector(task_id) => {
                self.overlays.push(Overlay::TaskInspector(task_id));
                Vec::new()
            }
            Overlay::Form(state) => {
                self.overlays.push(Overlay::Form(state));
                Vec::new()
            }
            Overlay::Confirmation(mut state) => {
                if key.code == KeyCode::Tab
                    && state.mutation.as_ref().is_some_and(|mutation| {
                        matches!(mutation, LocalMutation::Disconnect { .. })
                    })
                {
                    state.lose_ssh_checked = !state.lose_ssh_checked;
                    if state.lose_ssh_checked {
                        state.required_phrase = Some("LOSE-SSH".to_owned());
                    } else {
                        state.required_phrase = Some("DISCONNECT".to_owned());
                    }
                }
                self.overlays.push(Overlay::Confirmation(state));
                Vec::new()
            }
            Overlay::SecretResult => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('c'))
                    && key.modifiers.is_empty()
                {
                    let effects = self.copy_secret_result();
                    self.overlays.push(Overlay::SecretResult);
                    return effects;
                }
                self.overlays.push(Overlay::SecretResult);
                Vec::new()
            }
            Overlay::AuditInvestigation => {
                self.overlays.push(Overlay::AuditInvestigation);
                Vec::new()
            }
        }
    }

    pub(super) fn handle_quit_key(&mut self) -> Vec<Effect> {
        if self.tasks.has_active() {
            self.overlays.push(Overlay::QuitConfirmation);
            Vec::new()
        } else {
            self.request_shutdown(ShutdownReason::UserQuit)
        }
    }

    pub(super) fn pop_overlay(&mut self) -> Vec<Effect> {
        if let Some(overlay) = self.overlays.pop() {
            if matches!(overlay, Overlay::SecretResult) {
                return self.close_secret_result();
            }
            let confirmation_action = match &overlay {
                Overlay::Confirmation(state) => Some(state.action_id),
                _ => None,
            };
            if confirmation_action == Some(ActionId::CollectionExport) {
                self.pending_export_fingerprint = None;
            }
            if confirmation_action == Some(ActionId::AdminCredentialAuthKeyCreate) {
                self.pending_auth_key_request = None;
            }
            if confirmation_action == Some(ActionId::AdminCredentialRevoke) {
                self.pending_credential_revoke = None;
            }
            return Vec::new();
        }
        Vec::new()
    }

    pub(super) fn accept_navigation(&mut self) -> Vec<Effect> {
        let route = match &self.interaction {
            InteractionMode::CommandLine(state) => {
                state.candidates.first().map(|candidate| candidate.route)
            }
            _ => None,
        };
        let Some(route) = route else {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some("No matching view".to_owned());
            }
            return Vec::new();
        };
        self.open_navigation_route(route)
    }

    pub(super) fn open_navigation_route(&mut self, route: Route) -> Vec<Effect> {
        if route.requires_admin_profile() && self.admin.profile.is_none() {
            self.interaction = InteractionMode::Normal;
            self.pending_navigation_route = Some(route);
            self.navigate(Route::Profiles);
            self.status_notice = Some(format!(
                "Choose an administration profile and press Enter to open {}",
                route.label()
            ));
            return Vec::new();
        }
        if let Some(reason) = self.route_unavailable_reason(route) {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some(reason.to_owned());
            }
            return Vec::new();
        }
        self.interaction = InteractionMode::Normal;
        if self.current_route() == route {
            self.focus = Focus::Collection;
            return self.load_visible_diagnostics();
        }
        self.navigate(route);
        self.load_visible_diagnostics()
    }

    pub fn route_unavailable_reason(&self, route: Route) -> Option<&'static str> {
        if route.requires_admin_profile() && self.admin.profile.is_none() {
            return Some("Select an administration profile to open this view");
        }
        if route == Route::Local && self.source_mode == SourceMode::Unavailable {
            return Some("Enable local integration to open this view");
        }
        if route.requires_local_daemon() && !self.local_routes_available() {
            return Some("Connect to the local daemon to open this view");
        }
        if route.requires_observation_source()
            && !self.local_routes_available()
            && self.admin.profile.is_none()
        {
            return Some("Connect to the local daemon or select an administration profile");
        }
        None
    }

    pub(super) fn local_routes_available(&self) -> bool {
        matches!(
            self.local_daemon_state,
            LocalDaemonState::Mock | LocalDaemonState::Connecting | LocalDaemonState::Live
        )
    }

    pub(super) fn leave_unavailable_route(&mut self) {
        if self
            .route_unavailable_reason(self.current_route())
            .is_some()
        {
            self.open_local_account_recovery();
        }
    }

    pub(super) fn open_local_account_recovery(&mut self) {
        self.set_route(Route::Local);
        self.views.local.section = LocalSection::Accounts;
    }

    pub(super) fn accept_filter(&mut self, input: &str) -> Vec<Effect> {
        if matches!(
            self.current_route(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit
        ) {
            self.set_simple_collection_filter(input.trim().to_owned(), 0);
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Services
            && self.views.services.section != ServiceSection::Serve
        {
            self.views.services.filter_draft = input.trim().to_owned();
            self.views.services.applied_filter = FilterExpression::empty();
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Profiles {
            self.views.profiles.filter = input.trim().to_owned();
            self.views.profiles.selected = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Config {
            self.views.config.filter = input.trim().to_owned();
            self.views.config.selected = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Tasks {
            self.task_filter = input.trim().to_owned();
            self.tasks.select_filtered_first(&self.task_filter);
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        match filter::parse(input, &self.filter_schema()) {
            Ok(expression) => {
                if self.current_route() == Route::Services {
                    self.views.services.filter_draft = input.to_owned();
                    self.views.services.applied_filter = expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.views.devices.filter_draft = input.to_owned();
                    self.views.devices.applied_filter = expression;
                    self.reconcile_selection(None);
                }
                self.interaction = InteractionMode::Normal;
            }
            Err(error) => {
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(FilterErrorReport {
                        message: error.to_string(),
                        expected: error.expected,
                    });
                }
            }
        }
        Vec::new()
    }

    pub(super) fn navigate(&mut self, route: Route) {
        if self.current_route() == route {
            return;
        }
        self.capture_current_frame();
        let frame = ViewFrame::new(route);
        let _ = self.view_history.append(frame.clone());
        self.restore_view_frame(&frame);
        self.focus = Focus::Collection;
    }

    pub fn set_route(&mut self, route: Route) {
        self.view_history = ViewHistory::new(route);
        self.restore_view_frame(&ViewFrame::new(route));
    }

    pub(super) fn capture_current_frame(&mut self) {
        let frame = self.current_view_frame();
        self.view_history.replace_current(frame);
    }

    pub(super) fn current_view_frame(&self) -> ViewFrame {
        let route = self.current_route();
        let selection = match route {
            Route::Overview => self
                .views
                .overview
                .selected_id
                .clone()
                .map(ResourceIdentity::Opaque),
            Route::Devices => self
                .views
                .devices
                .selected_id
                .clone()
                .map(ResourceIdentity::Device),
            _ => None,
        };
        let section = (route == Route::Services).then_some(self.views.services.section);
        let local_section = (route == Route::Local).then_some(self.views.local.section);
        ViewFrame {
            route,
            focus: self.focus,
            selection: selection.clone(),
            scroll_anchor: selection,
            filter_text: if route == Route::Devices {
                self.views.devices.filter_draft.clone()
            } else {
                String::new()
            },
            filter: if route == Route::Devices {
                self.views.devices.applied_filter.clone()
            } else {
                FilterExpression::empty()
            },
            task_filter: if route == Route::Tasks {
                self.task_filter.clone()
            } else {
                String::new()
            },
            sort: self.views.devices.sort,
            section,
            local_section,
            saved_view: None,
        }
    }

    pub(super) fn restore_view_frame(&mut self, frame: &ViewFrame) {
        self.opened_task_return = false;
        if frame.route != Route::Profiles {
            self.pending_navigation_route = None;
        }
        self.detail_scroll = 0;
        self.detail_search.clear();
        self.detail_search_match = None;
        self.focus = frame.focus;
        if frame.route == Route::Overview {
            self.views.overview.selected_id = match &frame.selection {
                Some(ResourceIdentity::Opaque(id)) => Some(id.clone()),
                _ => None,
            };
            self.reconcile_overview_selection();
        }
        if frame.route == Route::Devices {
            self.views.devices.filter_draft = frame.filter_text.clone();
            self.views.devices.applied_filter = frame.filter.clone();
            self.views.devices.sort = frame.sort;
            self.views.devices.selected_id = match &frame.selection {
                Some(ResourceIdentity::Device(id)) => Some(id.clone()),
                _ => None,
            };
            let requested = self.views.devices.selected_id.clone();
            self.reconcile_selection(None);
            if requested.is_some() && self.views.devices.selected_id != requested {
                self.runtime_error = Some("previous selection no longer exists".to_owned());
            }
        }
        if frame.route == Route::Services {
            self.views.services.section = frame.section.unwrap_or(ServiceSection::Serve);
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
        }
        if frame.route == Route::Local {
            self.views.local.section = frame.local_section.unwrap_or(LocalSection::Client);
            self.views.local.selected = 0;
            self.views.local.scroll = 0;
        }
        if frame.route == Route::Tasks {
            self.task_filter = frame.task_filter.clone();
            self.tasks.select_filtered_first(&self.task_filter);
        }
    }

    pub(super) fn move_history(&mut self, forward: bool) {
        self.capture_current_frame();
        let frame = if forward {
            self.view_history.forward()
        } else {
            self.view_history.backward()
        };
        if let Some(frame) = frame {
            self.restore_view_frame(&frame);
        } else {
            self.runtime_error = Some(if forward {
                "already at newest view".to_owned()
            } else {
                "already at oldest view".to_owned()
            });
        }
    }
}
