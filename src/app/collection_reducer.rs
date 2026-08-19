use super::*;

impl App {
    pub(super) fn reduce_collection_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::ViewFilter => return self.open_collection_filter(),
            ActionId::DetailSearch => return self.open_detail_search(),
            ActionId::CollectionMoveUp => self.move_collection(-1),
            ActionId::CollectionMoveDown => self.move_collection(1),
            ActionId::CollectionFirst => self.select_collection_endpoint(false),
            ActionId::CollectionLast => self.select_collection_endpoint(true),
            ActionId::CollectionPageUp => self.move_collection(-5),
            ActionId::CollectionPageDown => self.move_collection(5),
            ActionId::CollectionBack => self.focus = Focus::Collection,
            ActionId::CollectionOpen => return self.open_collection_selection(),
            ActionId::CollectionSort => self.open_sort_menu(),
            ActionId::CollectionWideColumns => self.toggle_wide_columns(),
            ActionId::CollectionInspect => self.toggle_collection_inspector(),
            _ => return Vec::new(),
        }
        Vec::new()
    }

    pub(super) fn open_collection_filter(&mut self) -> Vec<Effect> {
        if self.filter_schema().is_empty()
            && !matches!(
                self.current_route(),
                Route::Tasks | Route::Profiles | Route::Config
            )
        {
            let subject = if self.current_route() == Route::Services {
                self.views.services.section.label()
            } else {
                self.current_route().label()
            };
            self.runtime_error = Some(format!("{subject} has nothing to filter on"));
            return Vec::new();
        }
        let input = self.collection_filter_input();
        let restoration = self.filter_restoration(&input, self.current_route() == Route::Services);
        let cursor = input.len();
        let sections = self.filter_suggestions(&input, cursor);
        let generation = self.advance_completion_generation();
        self.interaction = InteractionMode::FilterLine(FilterLineState {
            editor: LineEditorState::new(input),
            generation,
            sections,
            selected_completion: None,
            error: None,
            restoration,
            purpose: FilterLinePurpose::Collection,
        });
        Vec::new()
    }

    pub(super) fn collection_filter_input(&self) -> String {
        match self.current_route() {
            Route::Tasks => self.task_filter.clone(),
            Route::Users => self.views.users.filter.clone(),
            Route::Routes => self.views.routes.filter.clone(),
            Route::Credentials => self.views.credentials.filter.clone(),
            Route::Audit => self.views.audit.filter.clone(),
            Route::Profiles => self.views.profiles.filter.clone(),
            Route::Config => self.views.config.filter.clone(),
            Route::Services => self.views.services.filter_draft.clone(),
            _ => self.views.devices.filter_draft.clone(),
        }
    }

    pub(super) fn filter_restoration(
        &self,
        input: &str,
        service_expression: bool,
    ) -> FilterRestoration {
        FilterRestoration {
            input: input.to_owned(),
            expression: if service_expression {
                self.views.services.applied_filter.clone()
            } else {
                self.views.devices.applied_filter.clone()
            },
            selection: self.views.devices.selected_id.clone(),
            scroll: self.views.devices.scroll,
            task_filter: self.task_filter.clone(),
            task_selection: self.tasks.selected,
            profile_selection: self.views.profiles.selected,
            config_selection: self.views.config.selected,
            collection_selection: self.current_collection_selection(),
        }
    }

    pub(super) fn open_detail_search(&mut self) -> Vec<Effect> {
        let route = self.current_route();
        let input = if route == Route::Devices {
            self.views.devices.detail_search.clone()
        } else {
            self.detail_search.clone()
        };
        let restoration = self.filter_restoration(&self.views.devices.filter_draft, false);
        let generation = self.advance_completion_generation();
        self.interaction = InteractionMode::FilterLine(FilterLineState {
            editor: LineEditorState::new(input.clone()),
            generation,
            sections: Vec::new(),
            selected_completion: None,
            error: None,
            restoration,
            purpose: FilterLinePurpose::DetailSearch {
                route,
                scroll: if route == Route::Devices {
                    self.views.devices.detail_scroll
                } else {
                    self.detail_scroll
                },
                query: input,
                match_line: if route == Route::Devices {
                    self.views.devices.detail_search_match
                } else {
                    self.detail_search_match
                },
            },
        });
        Vec::new()
    }

    pub(super) fn move_collection(&mut self, offset: isize) {
        match self.current_route() {
            Route::Devices if self.focus == Focus::Inspector => {
                self.move_device_detail_scroll(offset);
            }
            Route::Access => self.move_access_scroll(offset),
            Route::Overview => self.move_overview_selection(offset),
            Route::Tasks => self.tasks.select_next_filtered(&self.task_filter, offset),
            Route::Audit => self.move_admin_activity_selection(offset),
            Route::Local => self.move_local_account_selection(offset),
            Route::Services => self.move_service_selection(offset),
            Route::Diagnostics => self.move_diagnostics_scroll(offset),
            Route::Users => self.move_admin_user_selection(offset),
            Route::Routes => self.move_admin_route_selection(offset),
            Route::Credentials => self.move_admin_credential_selection(offset),
            Route::Profiles => self.move_profile_selection(offset),
            Route::Config => self.move_config_selection(offset),
            _ => self.move_selection(offset),
        }
    }

    pub(super) fn select_collection_endpoint(&mut self, last: bool) {
        let endpoint = |length: usize| {
            if last { length.saturating_sub(1) } else { 0 }
        };
        match self.current_route() {
            Route::Devices if self.focus == Focus::Inspector => {
                self.views.devices.detail_scroll = if last {
                    self.device_detail_max_scroll()
                } else {
                    0
                };
            }
            Route::Access => {
                self.detail_scroll = if last { self.access_max_scroll() } else { 0 };
            }
            Route::Overview => {
                self.select_overview_position(if last { usize::MAX } else { 0 });
            }
            Route::Tasks => {
                if last {
                    self.tasks.select_filtered_last(&self.task_filter);
                } else {
                    self.tasks.select_filtered_first(&self.task_filter);
                }
            }
            Route::Audit => self.admin_activity_selected = endpoint(self.audit_event_count()),
            Route::Local => {
                self.views.local.selected = endpoint(self.local_accounts.len());
                self.views.local.scroll = self.views.local.selected;
            }
            Route::Services => {
                self.views.services.selected = endpoint(self.service_row_count());
                self.views.services.scroll = self.views.services.selected;
            }
            Route::Diagnostics if last => {
                self.views.diagnostics.scroll = self.metrics_max_scroll();
            }
            Route::Users => {
                self.admin_user_selected = endpoint(self.filtered_admin_users().len());
            }
            Route::Routes => {
                self.admin_route_selected = endpoint(self.filtered_admin_routes().len());
            }
            Route::Credentials => {
                self.admin_credential_selected = endpoint(self.filtered_admin_credentials().len());
            }
            Route::Profiles => {
                self.views.profiles.selected = endpoint(self.profile_rows().len());
            }
            Route::Config => {
                self.views.config.selected = endpoint(self.config_rows().len());
            }
            _ => self.move_selection_to(if last { usize::MAX } else { 0 }),
        }
    }

    pub(super) fn open_collection_selection(&mut self) -> Vec<Effect> {
        self.detail_search.clear();
        self.detail_search_match = None;
        match self.current_route() {
            Route::Overview => {
                if self.selected_overview_finding().is_some() {
                    self.focus = Focus::Inspector;
                }
            }
            Route::Tasks => {
                if let Some(task_id) = self.tasks.selected {
                    if self.admin_batch_results.contains_key(&task_id) {
                        self.overlays.push(Overlay::TaskInspector(task_id));
                    } else {
                        self.focus = Focus::Inspector;
                    }
                }
            }
            Route::Users => {
                if self.selected_admin_user().is_some() {
                    self.focus = Focus::Inspector;
                }
            }
            Route::Profiles => {
                if self.pending_navigation_route.is_some() {
                    return self.activate_selected_profile();
                }
                if self.selected_profile_row().is_some() {
                    self.focus = Focus::Inspector;
                }
            }
            Route::Routes if self.selected_admin_route().is_some() => {
                self.focus = Focus::Inspector;
            }
            Route::Credentials if self.selected_credential().is_some() => {
                self.focus = Focus::Inspector;
            }
            Route::Audit if self.selected_admin_activity().is_some() => {
                self.focus = Focus::Inspector;
            }
            Route::Services => {
                if self.service_inspector_available() {
                    self.focus = Focus::Inspector;
                }
            }
            _ if self.selected_device().is_some() => {
                let selected_id = self.selected_device().map(|device| device.id.0.clone());
                if self.current_route() == Route::Devices {
                    self.reset_device_detail_state();
                }
                self.focus = Focus::Inspector;
                if let Some(effect) = self.start_admin_device_enrichment(selected_id) {
                    return vec![effect];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub(super) fn open_sort_menu(&mut self) {
        self.interaction = InteractionMode::Transient(TransientMenuState {
            kind: TransientKind::Choice,
            title: "Sort",
            actions: Vec::new(),
            choices: self.sort_choices(),
            fields: Vec::new(),
            addresses: Vec::new(),
            prefix: None,
            message: None,
        });
    }

    pub(super) fn toggle_wide_columns(&mut self) {
        if self.current_route() != Route::Devices {
            return;
        }
        self.views.devices.wide_columns = !self.views.devices.wide_columns;
        self.status_notice = Some(format!(
            "device columns: {}",
            if self.views.devices.wide_columns {
                "extended"
            } else {
                "standard"
            }
        ));
    }

    pub(super) fn toggle_collection_inspector(&mut self) {
        let shown = match self.current_route() {
            Route::Devices => toggle(&mut self.views.devices.inspector),
            Route::Users => toggle(&mut self.views.users.inspector),
            Route::Tasks => toggle(&mut self.views.tasks.inspector),
            Route::Profiles => toggle(&mut self.views.profiles.inspector),
            Route::Routes => toggle(&mut self.views.routes.inspector),
            Route::Credentials => toggle(&mut self.views.credentials.inspector),
            Route::Audit => toggle(&mut self.views.audit.inspector),
            Route::Services => toggle(&mut self.views.services.inspector),
            _ => return,
        };
        if !shown {
            self.focus = Focus::Collection;
        }
    }
}

fn toggle(value: &mut bool) -> bool {
    *value = !*value;
    *value
}

impl App {
    pub(super) fn reconcile_selection(&mut self, replacement: Option<&Vec<Device>>) {
        let old_visible = self.visible_indices_for(&self.devices_resource.snapshot);
        let old_position = self.views.devices.selected_id.as_ref().and_then(|id| {
            old_visible.iter().position(|index| {
                self.devices_resource
                    .snapshot
                    .get(*index)
                    .is_some_and(|device| &device.id == id)
            })
        });
        let selected_id = self.views.devices.selected_id.clone();
        if let Some(devices) = replacement {
            if selected_id
                .as_ref()
                .is_some_and(|id| devices.iter().any(|device| &device.id == id))
            {
                return;
            }
            let target = old_position.unwrap_or(0);
            self.views.devices.selected_id = devices
                .get(target.min(devices.len().saturating_sub(1)))
                .map(|device| device.id.clone());
        } else {
            let visible = self.visible_indices();
            if let Some(id) = selected_id
                && visible
                    .iter()
                    .any(|index| self.devices_resource.snapshot[*index].id == id)
            {
                return;
            }
            let target = old_position.unwrap_or(0);
            self.views.devices.selected_id = visible
                .get(target.min(visible.len().saturating_sub(1)))
                .and_then(|index| self.devices_resource.snapshot.get(*index))
                .map(|device| device.id.clone());
        }
        self.views.devices.scroll = 0;
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.visible_indices_arc().as_ref().clone()
    }

    pub fn visible_indices_arc(&self) -> Arc<Vec<usize>> {
        let key = DeviceVisibleCacheKey {
            devices_generation: self.devices_resource.generation,
            local_generation: self.local_resource.generation,
            admin_generation: self.admin.devices.generation,
            // Only a filter that reads the clock makes the result depend on it.
            // Carrying it unconditionally re-filtered and re-sorted the whole
            // snapshot every tick for a result that had not changed.
            now: self
                .views
                .devices
                .applied_filter
                .requires_now()
                .then_some(self.now),
            source_mode: self.source_mode,
            filter: self.views.devices.applied_filter.clone(),
            sort: self.views.devices.sort,
            sort_terms: self.views.devices.sort_terms.clone(),
        };
        if let Some(cache) = self.device_visible_cache.borrow().as_ref()
            && cache.key == key
        {
            return Arc::clone(&cache.indices);
        }
        let indices = Arc::new(self.visible_indices_for(&self.devices_resource.snapshot));
        *self.device_visible_cache.borrow_mut() = Some(DeviceVisibleCache {
            key,
            indices: Arc::clone(&indices),
        });
        indices
    }

    pub(super) fn visible_indices_for(&self, devices: &[Device]) -> Vec<usize> {
        let requires_admin_data = self.views.devices.applied_filter.requires_admin_data();
        let sort_terms = self.device_sort_terms();
        let mut indices: Vec<usize> = devices
            .iter()
            .enumerate()
            .filter(|(_, device)| {
                let dns_name = if self.source_mode == SourceMode::Local {
                    self.local_dns_name(&device.id)
                } else {
                    None
                };
                let common_matches = self
                    .views
                    .devices
                    .applied_filter
                    .matches_with_dns(device, dns_name, self.now);
                let admin_matches = if requires_admin_data {
                    self.admin
                        .devices
                        .snapshot
                        .as_ref()
                        .and_then(|admin_devices| {
                            admin_devices
                                .iter()
                                .find(|admin| admin.stable_id == device.id.0)
                        })
                        .is_some_and(|admin| {
                            self.views
                                .devices
                                .applied_filter
                                .matches_admin(admin, self.now)
                        })
                } else {
                    true
                };
                common_matches && admin_matches
            })
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|left, right| {
            let left_device = devices.get(*left);
            let right_device = devices.get(*right);
            match (left_device, right_device) {
                (Some(left), Some(right)) => compare_devices_by_specs(left, right, &sort_terms),
                _ => left.cmp(right),
            }
        });
        indices
    }

    pub(super) fn local_dns_name(&self, id: &DeviceId) -> Option<&str> {
        self.local_resource.snapshot.as_ref().and_then(|snapshot| {
            if &snapshot.self_node.id == id {
                snapshot.self_node.dns_name.as_deref()
            } else {
                snapshot
                    .peers
                    .iter()
                    .find(|device| &device.id == id)
                    .and_then(|device| device.dns_name.as_deref())
            }
        })
    }

    pub(super) fn device_sort_terms(&self) -> Vec<SortSpec> {
        if self.views.devices.sort_terms.is_empty()
            || self.views.devices.sort_terms.first() != Some(&self.views.devices.sort)
        {
            vec![self.views.devices.sort]
        } else {
            self.views.devices.sort_terms.clone()
        }
    }

    pub(super) fn move_selection(&mut self, offset: isize) {
        let visible = self.visible_indices_arc();
        if visible.is_empty() {
            self.views.devices.selected_id = None;
            return;
        }
        let current = self
            .views
            .devices
            .selected_id
            .as_ref()
            .and_then(|id| {
                visible
                    .iter()
                    .position(|index| self.devices_resource.snapshot[*index].id == *id)
            })
            .unwrap_or(0);
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.views.devices.selected_id = visible
            .get(next)
            .and_then(|index| self.devices_resource.snapshot.get(*index))
            .map(|device| device.id.clone());
        self.ensure_device_selection_visible(next);
    }

    pub(super) fn move_selection_to(&mut self, position: usize) {
        let visible = self.visible_indices_arc();
        if visible.is_empty() {
            self.views.devices.selected_id = None;
            self.views.devices.scroll = 0;
            return;
        }
        let index = if position == usize::MAX {
            visible.len().saturating_sub(1)
        } else {
            position.min(visible.len().saturating_sub(1))
        };
        self.views.devices.selected_id = visible
            .get(index)
            .and_then(|value| self.devices_resource.snapshot.get(*value))
            .map(|device| device.id.clone());
        self.ensure_device_selection_visible(index);
    }

    pub(super) fn ensure_device_selection_visible(&mut self, position: usize) {
        let viewport = self.device_viewport_rows();
        if position < self.views.devices.scroll {
            self.views.devices.scroll = position;
        } else if position >= self.views.devices.scroll.saturating_add(viewport) {
            self.views.devices.scroll = position.saturating_add(1).saturating_sub(viewport);
        }
    }

    pub(super) fn device_viewport_rows(&self) -> usize {
        usize::from(self.terminal_height.saturating_sub(8)).max(1)
    }

    pub(super) fn move_admin_user_selection(&mut self, offset: isize) {
        let length = self.filtered_admin_users().len();
        self.admin_user_selected = move_bounded_index(self.admin_user_selected, length, offset);
    }

    pub(super) fn current_collection_selection(&self) -> usize {
        match self.current_route() {
            Route::Users => self.admin_user_selected,
            Route::Routes => self.admin_route_selected,
            Route::Credentials => self.admin_credential_selected,
            Route::Audit => self.admin_activity_selected,
            _ => 0,
        }
    }

    pub(super) fn set_simple_collection_filter(&mut self, filter: String, selection: usize) {
        match self.current_route() {
            Route::Users => {
                self.views.users.filter = filter;
                self.admin_user_selected = selection;
            }
            Route::Routes => {
                self.views.routes.filter = filter;
                self.admin_route_selected = selection;
            }
            Route::Credentials => {
                self.views.credentials.filter = filter;
                self.admin_credential_selected = selection;
            }
            Route::Audit => {
                self.views.audit.filter = filter;
                self.admin_activity_selected = selection;
            }
            _ => {}
        }
    }

    pub(super) fn move_device_detail_scroll(&mut self, offset: isize) {
        let length = self.device_detail_max_scroll().saturating_add(1);
        let current = self
            .views
            .devices
            .detail_scroll
            .min(length.saturating_sub(1));
        self.views.devices.detail_scroll = move_bounded_index(current, length, offset);
    }

    pub(super) fn move_access_scroll(&mut self, offset: isize) {
        let length = self.access_max_scroll().saturating_add(1);
        let current = self.detail_scroll.min(length.saturating_sub(1));
        self.detail_scroll = move_bounded_index(current, length, offset);
    }

    pub(super) fn access_max_scroll(&self) -> usize {
        let frame = self.frame_layout();
        crate::ui::views::access::max_scroll(self, frame.content.width, frame.content.height)
    }

    pub(super) fn device_detail_max_scroll(&self) -> usize {
        let frame = self.frame_layout();
        crate::ui::components::inspector::device_detail_max_scroll(
            self,
            frame.content.width,
            frame.content.height,
        )
    }

    pub(super) fn clamp_device_detail_scroll(&mut self) {
        if self.current_route() != Route::Devices || self.focus != Focus::Inspector {
            return;
        }
        self.views.devices.detail_scroll = self
            .views
            .devices
            .detail_scroll
            .min(self.device_detail_max_scroll());
    }

    pub(super) fn reset_device_detail_state(&mut self) {
        self.views.devices.detail_scroll = 0;
        self.views.devices.detail_search.clear();
        self.views.devices.detail_search_match = None;
    }

    pub(super) fn update_detail_search_preview(&mut self) {
        let (route, input, initial_scroll) = match &self.interaction {
            InteractionMode::FilterLine(FilterLineState {
                editor,
                purpose: FilterLinePurpose::DetailSearch { route, scroll, .. },
                ..
            }) => (*route, editor.input.trim().to_owned(), *scroll),
            _ => return,
        };
        if route != Route::Devices {
            self.detail_search = input;
            if route == Route::Access {
                let matches = crate::ui::views::access::search_matches(self, &self.detail_search);
                if self.detail_search.is_empty() {
                    self.detail_search_match = None;
                    self.detail_scroll = initial_scroll.min(self.access_max_scroll());
                } else {
                    let matched = matches
                        .iter()
                        .copied()
                        .find(|line| *line >= initial_scroll)
                        .or_else(|| matches.first().copied());
                    self.detail_search_match = matched;
                    if let Some(line) = matched {
                        self.detail_scroll = line.min(self.access_max_scroll());
                    }
                    if let InteractionMode::FilterLine(state) = &mut self.interaction {
                        state.error = matched.is_none().then(|| FilterErrorReport {
                            message: "No matches in this policy".to_owned(),
                            expected: "plain text".to_owned(),
                        });
                    }
                    return;
                }
            }
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
            }
            return;
        }
        self.views.devices.detail_search = input;
        let matches = crate::ui::components::inspector::device_detail_search_matches(
            self,
            &self.views.devices.detail_search,
        );
        if self.views.devices.detail_search.is_empty() {
            self.views.devices.detail_search_match = None;
            self.views.devices.detail_scroll = initial_scroll.min(self.device_detail_max_scroll());
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
            }
            return;
        }
        let matched = matches
            .iter()
            .copied()
            .find(|line| *line >= initial_scroll)
            .or_else(|| matches.first().copied());
        self.views.devices.detail_search_match = matched;
        if let Some(line) = matched {
            self.views.devices.detail_scroll = line.min(self.device_detail_max_scroll());
        }
        if let InteractionMode::FilterLine(state) = &mut self.interaction {
            state.error = matched.is_none().then(|| FilterErrorReport {
                message: "No matches in this device record".to_owned(),
                expected: "plain text".to_owned(),
            });
        }
    }

    pub(super) fn move_detail_search_match(&mut self, backwards: bool) {
        if self.current_route() == Route::Access {
            let matches = crate::ui::views::access::search_matches(self, &self.detail_search);
            let Some(next) = next_search_match(&matches, self.detail_search_match, backwards)
            else {
                self.runtime_error = Some("search the policy with / first".to_owned());
                return;
            };
            self.detail_search_match = Some(next);
            self.detail_scroll = next.min(self.access_max_scroll());
            return;
        }
        let matches = crate::ui::components::inspector::device_detail_search_matches(
            self,
            &self.views.devices.detail_search,
        );
        let Some(next) =
            next_search_match(&matches, self.views.devices.detail_search_match, backwards)
        else {
            self.runtime_error = Some("search device details with / first".to_owned());
            return;
        };
        self.views.devices.detail_search_match = Some(next);
        self.views.devices.detail_scroll = next.min(self.device_detail_max_scroll());
    }

    pub(super) fn move_admin_route_selection(&mut self, offset: isize) {
        let length = self.filtered_admin_routes().len();
        self.admin_route_selected = move_bounded_index(self.admin_route_selected, length, offset);
    }

    pub(super) fn move_admin_credential_selection(&mut self, offset: isize) {
        let length = self.filtered_admin_credentials().len();
        self.admin_credential_selected =
            move_bounded_index(self.admin_credential_selected, length, offset);
    }

    pub fn selected_admin_user(&self) -> Option<&crate::domain::user::AdminUser> {
        self.filtered_admin_users()
            .get(self.admin_user_selected)
            .copied()
    }

    pub fn filtered_admin_users(&self) -> Vec<&crate::domain::user::AdminUser> {
        let query = self.views.users.filter.trim();
        self.admin
            .users
            .snapshot
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|user| {
                query.is_empty()
                    || [
                        Some(user.id.as_str()),
                        user.display_name.as_deref(),
                        user.login_name.as_deref(),
                        user.role.as_deref(),
                        user.status.as_deref(),
                        user.relation_type.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| filter::contains_matches(value, query))
            })
            .collect()
    }

    pub fn filtered_admin_routes(&self) -> Vec<crate::admin::routes::AdminRouteObservation> {
        let query = self.views.routes.filter.trim();
        self.admin
            .route_observations()
            .into_iter()
            .filter(|route| {
                query.is_empty()
                    || [route.device_id.as_str(), route_role_label(route)]
                        .into_iter()
                        .chain(route.advertised.iter().map(String::as_str))
                        .chain(route.enabled.iter().map(String::as_str))
                        .any(|value| filter::contains_matches(value, query))
            })
            .collect()
    }

    pub fn filtered_admin_credentials(
        &self,
    ) -> Vec<&crate::domain::credential::CredentialMetadata> {
        let query = self.views.credentials.filter.trim();
        self.admin
            .credentials
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.records.as_slice())
            .iter()
            .filter(|credential| {
                query.is_empty()
                    || [
                        Some(credential.id.as_str()),
                        Some(credential.key_type.as_str()),
                        credential.description.as_deref(),
                        credential.user_id.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .chain(credential.scopes.iter().map(String::as_str))
                    .chain(credential.tags.iter().map(String::as_str))
                    .any(|value| filter::contains_matches(value, query))
            })
            .collect()
    }

    pub fn selected_admin_credential_for_view(
        &self,
    ) -> Option<&crate::domain::credential::CredentialMetadata> {
        self.filtered_admin_credentials()
            .get(self.admin_credential_selected)
            .copied()
    }

    pub(super) fn selected_admin_device(&self) -> Option<&crate::domain::device::AdminDevice> {
        let selected = self.views.devices.selected_id.as_ref()?.0.as_str();
        self.admin
            .devices
            .snapshot
            .as_ref()?
            .iter()
            .find(|device| device.stable_id == selected || device.exact_node_id() == Some(selected))
    }

    pub(super) fn selected_admin_route(
        &self,
    ) -> Option<crate::admin::routes::AdminRouteObservation> {
        self.filtered_admin_routes()
            .into_iter()
            .nth(self.admin_route_selected)
    }

    pub fn selected_admin_route_for_view(
        &self,
    ) -> Option<crate::admin::routes::AdminRouteObservation> {
        self.selected_admin_route()
    }

    pub(super) fn move_admin_activity_selection(&mut self, offset: isize) {
        let length = self.audit_event_count();
        self.admin_activity_selected =
            move_bounded_index(self.admin_activity_selected, length, offset);
    }

    pub fn audit_event_count(&self) -> usize {
        self.filtered_audit_events().len()
    }

    pub(super) fn selected_admin_activity(&self) -> Option<&crate::domain::activity::AuditEvent> {
        self.filtered_audit_events()
            .into_iter()
            .nth(self.admin_activity_selected)
    }

    pub fn filtered_audit_events(&self) -> Vec<&crate::domain::activity::AuditEvent> {
        let query = self.views.audit.filter.trim();
        self.admin
            .activity
            .snapshot
            .as_ref()
            .map_or_else(Vec::new, |snapshot| {
                snapshot.filtered_events(&self.audit_filters)
            })
            .into_iter()
            .filter(|event| {
                query.is_empty()
                    || [
                        event.event_type.as_deref(),
                        event.action.as_deref(),
                        event.origin.as_deref(),
                        event.action_details.as_deref(),
                        event.error.as_deref(),
                        event
                            .actor
                            .as_ref()
                            .and_then(|actor| actor.display.as_deref()),
                        event.actor.as_ref().and_then(|actor| actor.id.as_deref()),
                        event
                            .target
                            .as_ref()
                            .and_then(|target| target.display.as_deref()),
                        event
                            .target
                            .as_ref()
                            .and_then(|target| target.id.as_deref()),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| filter::contains_matches(value, query))
            })
            .collect()
    }

    pub(crate) fn selected_audit_event_for_view(
        &self,
    ) -> Option<&crate::domain::activity::AuditEvent> {
        self.selected_admin_activity()
    }

    pub(super) fn open_audit_reference(&mut self, target: bool) -> Vec<Effect> {
        let selected = self.selected_admin_activity().cloned();
        let (kind, id) = selected.as_ref().map_or((None, None), |event| {
            if target {
                (
                    event
                        .target
                        .as_ref()
                        .and_then(|value| value.kind.as_deref().map(str::to_ascii_lowercase)),
                    event.target.as_ref().and_then(|value| value.id.clone()),
                )
            } else {
                (
                    event
                        .actor
                        .as_ref()
                        .and_then(|value| value.kind.as_deref().map(str::to_ascii_lowercase)),
                    event.actor.as_ref().and_then(|value| value.id.clone()),
                )
            }
        });
        let Some(id) = id else {
            self.runtime_error =
                Some("the selected audit record has no exact reference ID".to_owned());
            return Vec::new();
        };
        if target {
            match kind.as_deref() {
                Some("dns") | Some("nameserver") | Some("searchpath") => {
                    self.navigate(Route::Dns);
                    return Vec::new();
                }
                Some("route") | Some("device_route") => {
                    if self
                        .admin
                        .route_observations()
                        .iter()
                        .any(|route| route.device_id == id)
                    {
                        self.navigate(Route::Routes);
                        return Vec::new();
                    }
                    self.runtime_error = Some(
                        "the exact audit route reference is not in the current snapshot".to_owned(),
                    );
                    return Vec::new();
                }
                Some("credential") | Some("key") | Some("auth_key") => {
                    self.views.credentials.filter.clear();
                    if let Some(index) =
                        self.admin
                            .credentials
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| {
                                snapshot.records.iter().position(|record| record.id == id)
                            })
                    {
                        self.admin_credential_selected = index;
                        self.navigate(Route::Credentials);
                        return Vec::new();
                    }
                    self.runtime_error = Some(
                        "the exact audit credential reference is not in the current snapshot"
                            .to_owned(),
                    );
                    return Vec::new();
                }
                Some("policy") | Some("acl") | Some("access") => {
                    self.navigate(Route::Access);
                    return Vec::new();
                }
                _ => {}
            }
        }
        match kind.as_deref() {
            Some("user") => {
                self.views.users.filter.clear();
                if let Some(index) = self
                    .admin
                    .users
                    .snapshot
                    .as_ref()
                    .and_then(|users| users.iter().position(|user| user.id == id))
                {
                    self.admin_user_selected = index;
                    self.navigate(Route::Users);
                    return Vec::new();
                }
            }
            Some("device") | Some("node") => {
                if let Some(index) =
                    self.admin.devices.snapshot.as_ref().and_then(|devices| {
                        devices.iter().position(|device| device.stable_id == id)
                    })
                {
                    let device_id = self
                        .admin
                        .devices
                        .snapshot
                        .as_ref()
                        .and_then(|devices| devices.get(index))
                        .map(|device| device.stable_id.clone());
                    self.views.devices.selected_id = device_id.map(DeviceId::new);
                    self.navigate(Route::Devices);
                    self.reset_device_detail_state();
                    self.focus = Focus::Inspector;
                    return self
                        .views
                        .devices
                        .selected_id
                        .as_ref()
                        .map(|device_id| device_id.0.clone())
                        .and_then(|device_id| self.start_admin_device_enrichment(Some(device_id)))
                        .into_iter()
                        .collect();
                }
            }
            Some("credential") | Some("key") | Some("auth_key") => {
                self.views.credentials.filter.clear();
                if let Some(index) = self
                    .admin
                    .credentials
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.records.iter().position(|record| record.id == id))
                {
                    self.admin_credential_selected = index;
                    self.navigate(Route::Credentials);
                    return Vec::new();
                }
            }
            _ => {}
        }
        self.runtime_error =
            Some("the exact audit reference is not in the current snapshot".to_owned());
        Vec::new()
    }

    pub(super) fn open_user_devices(&mut self) -> Vec<Effect> {
        let Some(user_id) = self.selected_admin_user().map(|user| user.id.clone()) else {
            return Vec::new();
        };
        self.views.devices.filter_draft = format!("owner:{user_id}");
        self.views.devices.applied_filter = FilterExpression {
            terms: vec![FilterTerm::Field {
                field: FilterField::Owner,
                negated: false,
                values: vec![user_id],
                comparison: None,
            }],
        };
        self.views.devices.selected_id = None;
        self.navigate(Route::Devices);
        self.reconcile_selection(None);
        Vec::new()
    }

    pub(super) fn open_route_device(&mut self) -> Vec<Effect> {
        let Some(route) = self.selected_admin_route() else {
            return Vec::new();
        };
        let device_id = DeviceId::new(route.device_id);
        if !self
            .devices_resource
            .snapshot
            .iter()
            .any(|device| device.id == device_id)
        {
            self.runtime_error =
                Some("route advertiser is not in the current device snapshot".to_owned());
            return Vec::new();
        }
        self.views.devices.selected_id = Some(device_id);
        self.views.devices.filter_draft.clear();
        self.views.devices.applied_filter = FilterExpression::empty();
        self.navigate(Route::Devices);
        self.reset_device_detail_state();
        self.focus = Focus::Inspector;
        let selected = self
            .views
            .devices
            .selected_id
            .as_ref()
            .map(|id| id.0.clone());
        self.start_admin_device_enrichment(selected)
            .into_iter()
            .collect()
    }

    /// Sort as two independent one-key decisions rather than a list of every
    /// field-and-direction pair.
    /// Field and direction in one mnemonic: the field key names the column, the
    /// second key names the order.
    /// The mapping table's own columns. Ascending and descending live behind
    /// the field key, the same two-key shape the device sort uses.
    pub(super) fn service_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.services.sort;
        ServiceSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ServiceSort(ServiceSortSpec { field, direction }),
                })
            })
            .collect()
    }

    pub(super) fn profile_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.profiles.sort;
        ProfileSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ProfileSort(ProfileSortSpec { field, direction }),
                })
            })
            .collect()
    }

    pub(super) fn config_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.config.sort;
        SettingSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ConfigSort(SettingSortSpec { field, direction }),
                })
            })
            .collect()
    }

    pub(super) fn sort_choices(&self) -> Vec<MenuChoice> {
        if self.current_route() == Route::Services {
            return self.service_sort_choices();
        }
        if self.current_route() == Route::Profiles {
            return self.profile_sort_choices();
        }
        if self.current_route() == Route::Config {
            return self.config_sort_choices();
        }
        const FIELDS: [(char, SortField, &str, &str); 10] = [
            ('n', SortField::Name, "Identity", "name"),
            ('i', SortField::DeviceId, "Identity", "id"),
            ('w', SortField::Owner, "Identity", "owner"),
            ('s', SortField::Liveness, "Connection", "state"),
            ('p', SortField::Path, "Connection", "path"),
            ('t', SortField::LastSeen, "Connection", "last seen"),
            ('o', SortField::Os, "Platform", "os"),
            ('v', SortField::Version, "Platform", "version"),
            ('c', SortField::Rx, "Traffic", "received"),
            ('m', SortField::Tx, "Traffic", "transmitted"),
        ];
        let current = self.views.devices.sort;
        FIELDS
            .into_iter()
            .flat_map(|(key, field, group, subject)| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{key}{order}"),
                    group: group.to_owned(),
                    subject: subject.to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::Sort(SortSpec { field, direction }),
                })
            })
            .collect()
    }

    pub(super) fn apply_choice(&mut self, outcome: ChoiceOutcome) -> Vec<Effect> {
        match outcome {
            ChoiceOutcome::Sort(sort) => {
                self.set_sort(sort);
                Vec::new()
            }
            ChoiceOutcome::ServiceSort(sort) => {
                self.views.services.sort = sort;
                self.views.services.selected = 0;
                self.views.services.scroll = 0;
                Vec::new()
            }
            ChoiceOutcome::ProfileSort(sort) => {
                self.views.profiles.sort = sort;
                self.views.profiles.selected = 0;
                Vec::new()
            }
            ChoiceOutcome::ConfigSort(sort) => {
                self.views.config.sort = sort;
                self.views.config.selected = 0;
                Vec::new()
            }
        }
    }

    pub(super) fn set_sort(&mut self, sort: SortSpec) {
        self.views.devices.sort = sort;
        self.views.devices.sort_terms = vec![sort];
        self.reconcile_selection(None);
    }

    pub(super) fn copy_field(&mut self, field: CopyField) -> Vec<Effect> {
        if field == CopyField::DiagnosticSummary {
            let value = self.diagnostic_summary();
            return self.copy_text(value);
        }
        if field == CopyField::Metrics {
            let value = self
                .services_snapshot
                .metrics
                .value
                .as_ref()
                .map_or_else(String::new, |metrics| metrics.text.clone());
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ServiceUrl | CopyField::ServiceListener | CopyField::ServiceBackend
        ) {
            let Some(mapping) = self.selected_service_mapping() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::ServiceListener => {
                    format!("{}:{}", mapping.listener.label(), mapping.listener.port())
                }
                CopyField::ServiceBackend => mapping.backend.argument(),
                _ => self.service_url(&mapping),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ConfigSetting | CopyField::ConfigValue | CopyField::ConfigSource
        ) {
            let Some(row) = self.selected_config_row() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::ConfigSetting => row.name.to_owned(),
                CopyField::ConfigValue => row.value,
                CopyField::ConfigSource => row.source.label().to_owned(),
                _ => return Vec::new(),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::UserId | CopyField::UserName | CopyField::UserLogin
        ) {
            let Some(user) = self.selected_admin_user() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::UserName => user.display_name.clone(),
                CopyField::UserLogin => user.login_name.clone(),
                _ => Some(user.id.clone()),
            };
            // The menu only offers a field the API reported, so a missing one
            // here means the selection moved: copy nothing rather than a lie.
            let Some(value) = value else {
                return Vec::new();
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::TaskId
                | CopyField::TaskResult
                | CopyField::TaskCommand
                | CopyField::TaskOutput
        ) {
            let Some(task) = self.focused_task() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::TaskResult => task.summary.clone(),
                CopyField::TaskCommand => task.redacted_argv.join(" "),
                CopyField::TaskOutput => task.detail.clone(),
                _ => task.id.to_string(),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ProfileName
                | CopyField::ProfileTailnet
                | CopyField::ProfileAccount
                | CopyField::ProfileCredential
                | CopyField::ProfileBackend
        ) {
            let Some(row) = self.selected_profile_row() else {
                return Vec::new();
            };
            let value = match (field, row) {
                (CopyField::ProfileName, row) => Some(row.label().to_owned()),
                (CopyField::ProfileTailnet, row) => row.tailnet().map(str::to_owned),
                (CopyField::ProfileAccount, ProfileRow::Local { account, .. }) => {
                    account.map(str::to_owned)
                }
                (CopyField::ProfileCredential, ProfileRow::Admin { config, .. }) => {
                    Some(config.credential.clone())
                }
                (CopyField::ProfileBackend, ProfileRow::Admin { config, .. }) => {
                    Some(config.credential_backend.location().display().to_string())
                }
                _ => None,
            };
            // The menu only offers a field the row has, so a missing one here
            // means the selection moved: copy nothing rather than a lie.
            let Some(value) = value else {
                return Vec::new();
            };
            return self.copy_text(value);
        }
        if field == CopyField::DnsName {
            let Some(value) = self.selected_dns_name() else {
                return Vec::new();
            };
            return self.copy_text(value);
        }
        if matches!(field, CopyField::PublicKey | CopyField::Endpoint) {
            let value = self.selected_local_device().and_then(|device| match field {
                CopyField::PublicKey => device.public_key.clone(),
                CopyField::Endpoint => device.current_endpoint.clone(),
                _ => None,
            });
            let value = match value {
                Some(value) => value,
                None => "not returned".to_owned(),
            };
            return self.copy_text(value);
        }
        let Some(device) = self.selected_device() else {
            return Vec::new();
        };
        let value = match field {
            CopyField::DeviceId => device.id.to_string(),
            CopyField::DisplayName => device.display_name.clone(),
            CopyField::Hostname => device.hostname.clone(),
            CopyField::Owner => match device.owner.clone().or_else(|| device.owner_label.clone()) {
                Some(owner) => owner,
                None => "not returned".to_owned(),
            },
            CopyField::Addresses => device.addresses.join(", "),
            CopyField::Tags => device.tags.join(", "),
            CopyField::DnsName
            | CopyField::PublicKey
            | CopyField::Endpoint
            | CopyField::DiagnosticSummary
            | CopyField::Metrics => "not returned".to_owned(),
            CopyField::ServiceUrl
            | CopyField::ServiceListener
            | CopyField::ServiceBackend
            | CopyField::UserId
            | CopyField::UserName
            | CopyField::UserLogin
            | CopyField::TaskId
            | CopyField::TaskResult
            | CopyField::TaskCommand
            | CopyField::TaskOutput
            | CopyField::ProfileName
            | CopyField::ProfileTailnet
            | CopyField::ProfileAccount
            | CopyField::ProfileCredential
            | CopyField::ProfileBackend
            | CopyField::ConfigSetting
            | CopyField::ConfigValue
            | CopyField::ConfigSource => "not returned".to_owned(),
        };
        self.copy_text(value)
    }

    /// What a mapping is reachable at: this machine's DNS name, the listener's
    /// scheme and port, and the mount path. This is the thing worth pasting
    /// somewhere, which is why it is the first entry in the copy menu.
    pub(super) fn service_url(&self, mapping: &ServiceMapping) -> String {
        // The first eligible certificate domain is exactly the name a Serve
        // mapping answers on, which is why the client offers it at all.
        let host = self
            .services_snapshot
            .certificate_domains
            .value
            .as_ref()
            .and_then(|domains| domains.first())
            .map(String::as_str)
            .unwrap_or("this-machine");
        let host = host.trim_end_matches('.');
        let scheme = match mapping.listener {
            Listener::Https(_) | Listener::TlsTerminatedTcp(_) => "https",
            Listener::Http(_) => "http",
            Listener::Tcp(_) => "tcp",
        };
        let port = mapping.listener.port();
        let path = mapping.mount.as_path();
        let path = if path == "/" { "" } else { path };
        format!("{scheme}://{host}:{port}{path}")
    }

    /// Says which command is missing, where Tale looked, and what to do. The
    /// discovery failure already knows all three; only the wording is new here.
    pub(super) fn missing_executable_reason(&self) -> String {
        use crate::domain::source::LocalCliState;
        match &self.local_cli_state {
            LocalCliState::Discovering => "still looking for the tailscale command".to_owned(),
            LocalCliState::Disabled => {
                "local access is off for this run; restart without --no-local".to_owned()
            }
            LocalCliState::Mock => "simulated data has no local command".to_owned(),
            LocalCliState::Available => {
                "the tailscale command is available but this action could not use it".to_owned()
            }
            LocalCliState::Unsupported { detail }
            | LocalCliState::Unavailable { detail }
            | LocalCliState::Missing { detail }
            | LocalCliState::PermissionDenied { detail } => detail.clone(),
        }
    }

    /// The status bar reports the text, so simulated copies report it too:
    /// `--mock` must show the same sentence the real clipboard produces.
    pub(super) fn copy_text(&mut self, text: String) -> Vec<Effect> {
        if self.source_mode == SourceMode::Mock {
            self.copied_value = Some(text);
            Vec::new()
        } else {
            vec![Effect::CopyText { text }]
        }
    }

    /// Whether the side inspector shares the content pane. The two table
    /// routes hold it behind `i` — the table is what they are for; everywhere
    /// else the inspector is the route.
    pub fn inspector_pane_visible(&self) -> bool {
        match self.current_route() {
            Route::Devices => self.views.devices.inspector,
            Route::Users => self.views.users.inspector,
            Route::Tasks => self.views.tasks.inspector,
            Route::Profiles => self.views.profiles.inspector,
            Route::Routes => self.views.routes.inspector,
            Route::Credentials => self.views.credentials.inspector,
            Route::Audit => self.views.audit.inspector,
            Route::Services => self.views.services.inspector,
            _ => true,
        }
    }

    pub fn footer_actions(&self, width: u16) -> Vec<action::FooterHint> {
        action::footer_actions_filtered(self.action_context(), self.current_route(), width, |id| {
            self.footer_action_is_relevant(id)
        })
    }

    pub fn active_detail_search(&self) -> &str {
        if self.current_route() == Route::Devices {
            &self.views.devices.detail_search
        } else {
            &self.detail_search
        }
    }

    pub fn footer_action_is_relevant(&self, id: ActionId) -> bool {
        if !action::applies_to_route(id, self.current_route())
            || self.action_unavailable_reason(id).is_some()
        {
            return false;
        }
        match id {
            ActionId::CollectionMoveUp
            | ActionId::CollectionMoveDown
            | ActionId::CollectionFirst
            | ActionId::CollectionLast
            | ActionId::CollectionPageUp
            | ActionId::CollectionPageDown
            | ActionId::CollectionOpen
            | ActionId::CollectionSort
            | ActionId::CollectionInspect => self.collection_subject_available(),
            ActionId::CollectionBack => self.focus == Focus::Inspector,
            ActionId::TaskCancel => self.tasks.selected_can_cancel(),
            _ => true,
        }
    }

    pub(super) fn collection_subject_available(&self) -> bool {
        match self.current_route() {
            Route::Overview => self.selected_overview_finding().is_some(),
            Route::Local => self.selected_local_account().is_some(),
            Route::Devices => self.selected_device().is_some(),
            Route::Users => self.selected_admin_user().is_some(),
            Route::Routes => self.selected_admin_route().is_some(),
            Route::Credentials => self.selected_credential().is_some(),
            Route::Profiles => self.selected_profile_row().is_some(),
            Route::Config => self.selected_config_row().is_some(),
            Route::Tasks => self.tasks.selected.is_some(),
            Route::Audit => self.selected_admin_activity().is_some(),
            Route::Services => self.service_inspector_available(),
            _ => true,
        }
    }

    pub fn selected_device(&self) -> Option<&Device> {
        let id = self.views.devices.selected_id.as_ref()?;
        self.devices_resource
            .snapshot
            .iter()
            .find(|device| &device.id == id)
    }

    pub fn selected_overview_finding(&self) -> Option<&Finding> {
        let selected = self.views.overview.selected_id.as_deref();
        selected
            .and_then(|id| self.health_findings.iter().find(|finding| finding.id == id))
            .or_else(|| self.health_findings.first())
    }

    pub(super) fn move_overview_selection(&mut self, offset: isize) {
        if self.health_findings.is_empty() {
            self.views.overview.selected_id = None;
            return;
        }
        let current = self
            .views
            .overview
            .selected_id
            .as_deref()
            .and_then(|id| {
                self.health_findings
                    .iter()
                    .position(|finding| finding.id == id)
            })
            .unwrap_or(0);
        let next = move_bounded_index(current, self.health_findings.len(), offset);
        self.views.overview.selected_id = self
            .health_findings
            .get(next)
            .map(|finding| finding.id.clone());
    }

    pub(super) fn select_overview_position(&mut self, position: usize) {
        let index = if position == usize::MAX {
            self.health_findings.len().saturating_sub(1)
        } else {
            position.min(self.health_findings.len().saturating_sub(1))
        };
        self.views.overview.selected_id = self
            .health_findings
            .get(index)
            .map(|finding| finding.id.clone());
    }

    pub(super) fn reconcile_overview_selection(&mut self) {
        if self
            .views
            .overview
            .selected_id
            .as_deref()
            .is_some_and(|id| self.health_findings.iter().any(|finding| finding.id == id))
        {
            return;
        }
        self.views.overview.selected_id = self
            .health_findings
            .first()
            .map(|finding| finding.id.clone());
    }

    pub fn selected_local_device(&self) -> Option<&LocalDevice> {
        let id = self.views.devices.selected_id.as_ref()?;
        let snapshot = self.local_resource.snapshot.as_ref()?;
        if &snapshot.self_node.id == id {
            return Some(&snapshot.self_node);
        }
        snapshot.peers.iter().find(|device| &device.id == id)
    }
}
