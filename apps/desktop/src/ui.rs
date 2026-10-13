use crate::tray::{Tray, TrayAction};
use eframe::egui::{self, Color32, RichText};
use std::{path::PathBuf, sync::mpsc, time::Duration};
use toolboxcraft_core::{
    catalog::{App, CATALOG, by_id},
    engine::{Command, EngineHandle, Event},
    model::*,
    releases::select_asset,
    self_update,
};
const BLUE: Color32 = Color32::from_rgb(107, 138, 255);
const MUTED: Color32 = Color32::from_rgb(165, 167, 172);
fn muted(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        MUTED
    } else {
        Color32::from_rgb(100, 103, 110)
    }
}
fn text_action(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let size = ui
        .painter()
        .layout_no_wrap(label.into(), font.clone(), BLUE)
        .size()
        + ui.spacing().button_padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = if response.is_pointer_button_down_on() {
        Color32::from_rgb(78, 109, 223)
    } else if response.hovered() || response.has_focus() {
        if ui.visuals().dark_mode {
            Color32::from_rgb(172, 190, 255)
        } else {
            Color32::from_rgb(48, 79, 192)
        }
    } else {
        BLUE
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn icon_action(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let color = if response.hovered() || response.has_focus() {
        ui.visuals().text_color()
    } else {
        muted(ui)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(20.0),
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn primary_action(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(14.0).color(Color32::WHITE))
            .frame(false)
            .min_size(egui::vec2(72.0, 32.0)),
    );
    let fill = if !enabled {
        BLUE.gamma_multiply(0.4)
    } else if response.is_pointer_button_down_on() {
        Color32::from_rgb(78, 109, 223)
    } else if response.hovered() || response.has_focus() {
        Color32::from_rgb(132, 158, 255)
    } else {
        BLUE
    };
    ui.painter()
        .set(background, egui::Shape::rect_filled(response.rect, 5, fill));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub struct Toolbox {
    handle: EngineHandle,
    events: mpsc::Receiver<Event>,
    tray: Option<Tray>,
    window: crate::window::WindowControl,
    tray_attempted: bool,
    search: String,
    settings_open: bool,
    details: Option<String>,
    remove: Option<String>,
    notice: String,
    update: Option<self_update::SelfUpdate>,
    background: bool,
    preview: bool,
    screenshot: Option<PathBuf>,
    frames: u32,
    quit: bool,
}
impl Toolbox {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        handle: EngineHandle,
        events: mpsc::Receiver<Event>,
        background: bool,
        preview: bool,
        screenshot: Option<PathBuf>,
        scale: Option<f32>,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Inter".into(),
            egui::FontData::from_static(include_bytes!("../../../assets/fonts/Inter.ttf")).into(),
        );
        fonts.font_data.insert(
            "JetBrains Mono".into(),
            egui::FontData::from_static(include_bytes!("../../../assets/fonts/JetBrainsMono.ttf"))
                .into(),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "Inter".into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .insert(0, "JetBrains Mono".into());
        cc.egui_ctx.set_fonts(fonts);
        if let Some(scale) = scale {
            cc.egui_ctx.set_pixels_per_point(scale);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(460.0, 720.0)));
        }
        Self {
            handle,
            events,
            tray: None,
            window: crate::window::WindowControl::new(cc),
            tray_attempted: false,
            search: String::new(),
            settings_open: preview && std::env::args().any(|arg| arg == "--preview-settings"),
            details: if preview && std::env::args().any(|arg| arg == "--preview-notes") {
                Some("photocraft".into())
            } else {
                None
            },
            remove: None,
            notice: String::new(),
            update: None,
            background,
            preview,
            screenshot,
            frames: 0,
            quit: false,
        }
    }
    fn send(&self, command: Command) {
        if !self.preview {
            let _ = self.handle.commands.send(command);
        }
    }
    fn row(&mut self, ui: &mut egui::Ui, app: App, state: &State, target: Target) {
        let installation = state.installations.get(app.id);
        let cached = state.releases.get(app.id);
        let operation = state.operations.get(app.id);
        let active = operation.is_some_and(Operation::active);
        let update = installation
            .filter(|i| i.managed)
            .zip(cached)
            .is_some_and(|(i, r)| newer(&r.release.tag_name, &i.version));
        let compatible = cached.is_some_and(|r| select_asset(app, &r.release, target).is_ok());
        let background = ui.painter().add(egui::Shape::Noop);
        let row = ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(app.id)
                .sense(egui::Sense::click()),
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(8, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Image::from_bytes(
                                    format!("bytes://{}.png", app.id),
                                    icon_bytes(app.id),
                                )
                                .fit_to_exact_size(egui::vec2(48.0, 48.0)),
                            );
                            ui.add_space(4.0);
                            let text_width = (ui.available_width() - 110.0).max(100.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(text_width, if update { 72.0 } else { 48.0 }),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.label(RichText::new(app.name).size(16.0).strong());
                                    let subtitle = if let Some(i) = installation {
                                        if i.managed {
                                            if update {
                                                format!(
                                                    "{} → {}",
                                                    i.version.trim_start_matches('v'),
                                                    cached
                                                        .unwrap()
                                                        .release
                                                        .tag_name
                                                        .trim_start_matches('v')
                                                )
                                            } else {
                                                i.version.trim_start_matches('v').to_string()
                                            }
                                        } else {
                                            format!(
                                                "{} · External",
                                                i.version.trim_start_matches('v')
                                            )
                                        }
                                    } else if let Some(cached) = cached {
                                        format!(
                                            "{} · {}",
                                            cached.release.tag_name.trim_start_matches('v'),
                                            app.description
                                        )
                                    } else {
                                        app.description.into()
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&subtitle).size(12.0).color(muted(ui)),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(subtitle);
                                    if update && text_action(ui, "What's new").clicked() {
                                        self.details = Some(app.id.into());
                                    }
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (more, _) =
                                        egui::containers::menu::MenuButton::from_button(
                                            egui::Button::new("")
                                                .frame(false)
                                                .min_size(egui::vec2(22.0, 30.0)),
                                        )
                                        .ui(ui, |ui| {
                                            if ui.button("Release notes").clicked() {
                                                self.details = Some(app.id.into());
                                                ui.close();
                                            }
                                            if ui.button("Website ↗").clicked() {
                                                ui.ctx().open_url(egui::OpenUrl::new_tab(
                                                    app.website(),
                                                ));
                                                ui.close();
                                            }
                                            if installation.is_some_and(|i| i.managed) {
                                                let mut enabled = !state
                                                    .settings
                                                    .disabled_apps
                                                    .iter()
                                                    .any(|id| id == app.id);
                                                if ui
                                                    .checkbox(&mut enabled, "Automatic updates")
                                                    .changed()
                                                {
                                                    let mut settings = state.settings.clone();
                                                    settings
                                                        .disabled_apps
                                                        .retain(|id| id != app.id);
                                                    if !enabled {
                                                        settings.disabled_apps.push(app.id.into());
                                                    }
                                                    self.send(Command::Settings(settings));
                                                }
                                                if ui
                                                    .add_enabled(
                                                        !active,
                                                        egui::Button::new("Uninstall…"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.remove = Some(app.id.into());
                                                    ui.close();
                                                }
                                            } else if installation.is_some()
                                                && ui
                                                    .add_enabled(
                                                        compatible && !active,
                                                        egui::Button::new("Install managed copy"),
                                                    )
                                                    .clicked()
                                            {
                                                self.send(Command::Install(app.id.into()));
                                                ui.close();
                                            }
                                        });
                                    let color = if more.hovered() || more.has_focus() {
                                        ui.visuals().text_color()
                                    } else {
                                        muted(ui)
                                    };
                                    for offset in [-5.0, 0.0, 5.0] {
                                        ui.painter().circle_filled(
                                            more.rect.center() + egui::vec2(0.0, offset),
                                            1.5,
                                            color,
                                        );
                                    }
                                    more.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            "More options",
                                        )
                                    });
                                    more.on_hover_text("More options")
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if !active
                                        && (installation.is_none() || update)
                                        && primary_action(
                                            ui,
                                            installation.is_some() || compatible,
                                            if update { "Update" } else { "Install" },
                                        )
                                        .clicked()
                                    {
                                        self.send(Command::Install(app.id.into()));
                                    }
                                },
                            );
                        });
                        if let Some(op) = operation {
                            match op {
                                Operation::Downloading { received, total } => {
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        let fraction = if *total == 0 {
                                            0.0
                                        } else {
                                            *received as f32 / *total as f32
                                        };
                                        ui.add(
                                            egui::ProgressBar::new(fraction)
                                                .desired_width(
                                                    (ui.available_width() - 45.0).max(100.0),
                                                )
                                                .text(format!(
                                                    "{:.1} / {:.1} MB",
                                                    *received as f64 / 1e6,
                                                    *total as f64 / 1e6
                                                )),
                                        );
                                        if icon_action(ui, "×", "Cancel download").clicked() {
                                            self.send(Command::Cancel(app.id.into()));
                                        }
                                    });
                                }
                                Operation::Queued
                                | Operation::Verifying
                                | Operation::WaitingForExit
                                | Operation::Installing => {
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(
                                            RichText::new(match op {
                                                Operation::Queued => "Queued",
                                                Operation::Verifying => "Verified download",
                                                Operation::WaitingForExit => {
                                                    "Waiting for app to close"
                                                }
                                                _ => "Installing…",
                                            })
                                            .size(11.0)
                                            .color(muted(ui)),
                                        );
                                        if !matches!(op, Operation::Installing)
                                            && text_action(ui, "Cancel").clicked()
                                        {
                                            self.send(Command::Cancel(app.id.into()));
                                        }
                                    });
                                }
                                Operation::Failed(message) => {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(message)
                                            .size(11.0)
                                            .color(Color32::from_rgb(245, 153, 137)),
                                    );
                                    if text_action(ui, "Retry").clicked() {
                                        self.send(Command::Install(app.id.into()));
                                    }
                                }
                                Operation::Cancelled => {
                                    ui.label(
                                        RichText::new("Cancelled").size(11.0).color(muted(ui)),
                                    );
                                }
                                Operation::Complete => {}
                            }
                        }
                    });
            },
        );
        let response = row.response;
        if response.contains_pointer() || response.has_focus() {
            ui.painter().set(
                background,
                egui::Shape::rect_filled(response.rect, 7, ui.visuals().widgets.hovered.bg_fill),
            );
        }
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, app.name));
        if response.clicked() {
            if installation.is_some() {
                self.send(Command::Launch(app.id.into()));
            } else {
                self.details = Some(app.id.into());
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.add_space(6.0);
    }
    fn release_notes(&mut self, ui: &mut egui::Ui, state: &State, id: &str) {
        let app = by_id(id).unwrap();
        egui::ScrollArea::vertical()
            .id_salt(("release-notes", id))
            .show(ui, |ui| {
                ui.label(RichText::new(app.description).color(muted(ui)));
                ui.add_space(12.0);
                ui.label(RichText::new("Release notes").size(16.0).strong());
                if let Some(cached) = state.releases.get(id) {
                    ui.label(RichText::new(&cached.release.tag_name).monospace());
                    if let Some(date) = &cached.release.published_at {
                        ui.label(RichText::new(date).size(11.0).color(muted(ui)));
                    }
                    if now().saturating_sub(cached.checked_at) > 6 * 60 * 60 {
                        ui.label("Cached release information. Check updates to refresh.");
                    }
                    ui.add_space(12.0);
                    ui.label(
                        cached
                            .release
                            .body
                            .as_deref()
                            .unwrap_or("No release notes published."),
                    );
                    ui.add_space(16.0);
                    if text_action(ui, "View release on GitHub ↗").clicked() {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(&cached.release.html_url));
                    }
                } else {
                    ui.label(
                        "Release information is unavailable. Check your connection and retry.",
                    );
                    if text_action(ui, "Retry").clicked() {
                        self.send(Command::Check);
                    }
                }
            });
    }
    fn settings(&mut self, ui: &mut egui::Ui, state: &State) {
        let mut settings = state.settings.clone();
        egui::ScrollArea::vertical().id_salt("settings-scroll").show(ui,|ui| {
            ui.label(RichText::new("Updates").strong());
            ui.checkbox(&mut settings.auto_update,"Automatically update closed apps");
            ui.label(RichText::new("Checks at startup and every six hours while ToolboxCraft runs.").size(11.0).color(muted(ui)));
            ui.checkbox(&mut settings.launch_at_login,"Launch ToolboxCraft at login");
            ui.add_space(12.0); ui.label(RichText::new("Appearance").strong());
            ui.horizontal(|ui| {ui.selectable_value(&mut settings.theme,Theme::System,"System");ui.selectable_value(&mut settings.theme,Theme::Dark,"Dark");ui.selectable_value(&mut settings.theme,Theme::Light,"Light");});
            ui.add_space(12.0); ui.label(RichText::new("ToolboxCraft").strong());ui.label(format!("Version {}",env!("CARGO_PKG_VERSION")));
            if let Some(update)=&self.update {ui.label(format!("Version {} is available",update.manifest.version));if ui.button("Update and restart").clicked(){self.notice="Downloading ToolboxCraft update…".into();self.send(Command::ApplySelfUpdate);}}
            else if self_update::configuration().is_none(){ui.label(RichText::new("Self-updates are unavailable in this build. A release repository and signing key have not been configured.").size(11.0).color(muted(ui)));}
            else if ui.button("Check ToolboxCraft updates").clicked(){self.send(Command::CheckSelfUpdate);}
            ui.add_space(12.0);egui::CollapsingHeader::new("Update history").show(ui,|ui|{egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui|{if state.history.is_empty(){ui.label("No updates installed yet.");}for entry in &state.history{let name=by_id(&entry.app).map(|a|a.name).unwrap_or(&entry.app);ui.label(format!("{} {} · {}",entry.action,name,entry.version));}});});
            ui.separator();ui.label(RichText::new("Independent launcher for Artcraft apps.").size(11.0).color(muted(ui)));
        });
        if settings != state.settings {
            self.send(Command::Settings(settings));
        }
    }
}
impl eframe::App for Toolbox {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if self.frames == 5
            && !self.preview
            && let Err(err) = toolboxcraft_platform::self_update::confirm_startup()
        {
            tracing::warn!("Cannot clean up self-update backup: {err}");
        }
        if !self.tray_attempted && !self.preview {
            self.tray_attempted = true;
            match Tray::new(self.handle.clone(), self.window.clone()) {
                Ok(tray) => self.tray = Some(tray),
                Err(err) => tracing::warn!("Tray unavailable: {err}"),
            };
            if self.background && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Changed => {}
                Event::Notice(message) => {
                    if !self.preview {
                        self.notice = message;
                    }
                }
                Event::SelfUpdate(update) => self.update = update,
                Event::RestartReady => {
                    self.quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(tray) = &mut self.tray {
            for action in tray.poll() {
                match action {
                    TrayAction::Open => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    TrayAction::Quit => {
                        self.quit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        let state = self.handle.state.lock().unwrap().clone();
        let target = Target::current().unwrap();
        let preference = match state.settings.theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Dark => egui::ThemePreference::Dark,
            Theme::Light => egui::ThemePreference::Light,
        };
        ctx.set_theme(preference);
        let mut visuals = if ctx.theme() == egui::Theme::Dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        if visuals.dark_mode {
            visuals.panel_fill = Color32::from_rgb(32, 33, 36);
            visuals.window_fill = visuals.panel_fill;
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(43, 45, 48);
            visuals.override_text_color = Some(Color32::from_rgb(242, 243, 245));
        }
        visuals.selection.bg_fill = BLUE;
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        ctx.set_visuals(visuals);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(9.0, 5.0);
            s.spacing.scroll = egui::style::ScrollStyle::solid();
            s.spacing.scroll.bar_width = 6.0;
        });
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.settings_open = false;
            self.details = None;
        }
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(18),
            )
            .show(ctx, |ui| {
                if self.settings_open || self.details.is_some() {
                    let heading = if self.settings_open {
                        "Settings".to_string()
                    } else {
                        by_id(self.details.as_deref().unwrap())
                            .unwrap()
                            .name
                            .to_string()
                    };
                    if text_action(ui, "← Back").clicked() {
                        self.settings_open = false;
                        self.details = None;
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new(heading).size(19.0).strong());
                    return;
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_bytes(
                            "bytes://toolboxcraft.png",
                            include_bytes!("../../../assets/icons/toolboxcraft.png"),
                        )
                        .fit_to_exact_size(egui::vec2(28.0, 28.0)),
                    );
                    ui.label(RichText::new("ToolboxCraft").size(19.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_action(ui, "⚙", "Settings").clicked() {
                            self.settings_open = true;
                            self.details = None;
                        }
                    });
                });
                ui.add_space(12.0);
                let search_fill = if ui.visuals().dark_mode {
                    Color32::from_rgb(43, 45, 48)
                } else {
                    Color32::from_rgb(237, 238, 241)
                };
                let search = egui::Frame::new()
                    .fill(search_fill)
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
                            let stroke = egui::Stroke::new(1.4, muted(ui));
                            let center = rect.min + egui::vec2(6.0, 7.0);
                            ui.painter().circle_stroke(center, 4.5, stroke);
                            ui.painter().line_segment(
                                [center + egui::vec2(3.5, 3.5), center + egui::vec2(7.0, 7.0)],
                                stroke,
                            );
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .hint_text("Search apps")
                                    .font(egui::FontId::proportional(13.0))
                                    .frame(false)
                                    .desired_width(ui.available_width() - 24.0),
                            );
                            if !self.search.is_empty()
                                && ui
                                    .add(egui::Button::new("×").frame(false))
                                    .on_hover_text("Clear search")
                                    .clicked()
                            {
                                self.search.clear();
                                response.request_focus();
                            }
                            response
                        })
                        .inner
                    })
                    .inner;
                if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                    search.request_focus();
                }
            });
        egui::TopBottomPanel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(target.label()).size(10.0).color(muted(ui)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if text_action(ui, "Check for updates").clicked() {
                            self.send(Command::Check);
                        }
                    });
                });
                if !self.notice.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&self.notice).size(10.0).color(muted(ui)));
                }
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(18, 8)),
            )
            .show(ctx, |ui| {
                if self.settings_open {
                    self.settings(ui, &state);
                    return;
                }
                if let Some(id) = self.details.clone() {
                    self.release_notes(ui, &state, &id);
                    return;
                }
                let query = self.search.to_lowercase();
                let apps: Vec<App> = CATALOG
                    .into_iter()
                    .filter(|a| {
                        a.name.to_lowercase().contains(&query)
                            || a.description.to_lowercase().contains(&query)
                    })
                    .collect();
                let updates: Vec<_> = state
                    .installations
                    .iter()
                    .filter(|(id, i)| {
                        i.managed
                            && state
                                .releases
                                .get(*id)
                                .is_some_and(|r| newer(&r.release.tag_name, &i.version))
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for installed in [true, false] {
                            let section: Vec<_> = apps
                                .iter()
                                .filter(|a| state.installations.contains_key(a.id) == installed)
                                .copied()
                                .collect();
                            if section.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if installed {
                                        "Installed"
                                    } else {
                                        "Available"
                                    })
                                    .size(14.0)
                                    .color(muted(ui)),
                                );
                                ui.label(
                                    RichText::new(section.len().to_string())
                                        .size(10.0)
                                        .color(muted(ui)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if installed && !updates.is_empty() {
                                            if text_action(ui, "Update all")
                                                .on_hover_text(format!(
                                                    "{} updates available",
                                                    updates.len()
                                                ))
                                                .clicked()
                                            {
                                                for id in &updates {
                                                    self.send(Command::Install(id.clone()));
                                                }
                                            }
                                        } else if !installed
                                            && text_action(ui, "Install all").clicked()
                                        {
                                            for app in &section {
                                                self.send(Command::Install(app.id.into()));
                                            }
                                        }
                                    },
                                );
                            });
                            ui.add_space(6.0);
                            for app in section {
                                self.row(ui, app, &state, target);
                            }
                            ui.add_space(12.0);
                        }
                        if apps.is_empty() {
                            ui.label("No matching apps. Try a different search.");
                        }
                    });
            });
        if let Some(id) = self.remove.clone() {
            let app = by_id(&id).unwrap();
            egui::Window::new(format!("Uninstall {}?",app.name)).collapsible(false).resizable(false).show(ctx,|ui|{ui.label("Application files and ToolboxCraft shortcuts will be removed. Your documents and app preferences will remain.");ui.horizontal(|ui|{if ui.button("Cancel").clicked(){self.remove=None;} else if ui.button("Uninstall").clicked(){self.send(Command::Remove(id.clone()));self.remove=None;}});});
        }
        if self.frames == 32 && self.screenshot.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let (Some(path), Some(image)) = (&self.screenshot, shot) {
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Err(err) = image::save_buffer(
                path,
                &pixels,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            ) {
                tracing::error!("Screenshot failed: {err}");
            }
            self.quit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
pub fn icon_bytes(id: &str) -> &'static [u8] {
    match id {
        "photocraft" => include_bytes!("../../../assets/icons/photocraft.png"),
        "vectorcraft" => include_bytes!("../../../assets/icons/vectorcraft.png"),
        "filmcraft" => include_bytes!("../../../assets/icons/filmcraft.png"),
        "lightcraft" => include_bytes!("../../../assets/icons/lightcraft.png"),
        "pdfcraft" => include_bytes!("../../../assets/icons/pdfcraft.png"),
        "effectcraft" => include_bytes!("../../../assets/icons/effectcraft.png"),
        "designcraft" => include_bytes!("../../../assets/icons/designcraft.png"),
        "wordcraft" => include_bytes!("../../../assets/icons/wordcraft.png"),
        "soundcraft" => include_bytes!("../../../assets/icons/soundcraft.png"),
        "deckcraft" => include_bytes!("../../../assets/icons/deckcraft.png"),
        "cadcraft" => include_bytes!("../../../assets/icons/cadcraft.png"),
        "gridcraft" => include_bytes!("../../../assets/icons/gridcraft.png"),
        _ => include_bytes!("../../../assets/icons/toolboxcraft.png"),
    }
}
pub fn preview_state(handle: &EngineHandle) {
    let mut state = handle.state.lock().unwrap();
    state.settings.theme = Theme::Dark;
    state.installations.clear();
    state.operations.clear();
    for (index, app) in CATALOG.iter().enumerate() {
        let version = format!("0.{}.0", index + 2);
        let target = Target::current().unwrap();
        let name = format!("{}-{version}-{}", app.id, target.asset_suffix());
        state.releases.insert(app.id.into(),CachedRelease {
            release: Release { tag_name:format!("v{version}"), body:Some("## What's new\n\n• Improved file compatibility\n• Faster startup\n• Fixed rendering issues".into()), html_url:format!("https://github.com/{}/releases",app.repo()), published_at:Some("2026-10-12".into()), prerelease:false, draft:false,
                assets: vec![Asset {name:name.clone(),size:120_000_000,browser_download_url:format!("https://github.com/{}/releases/download/v{version}/{name}",app.repo())}] },
            etag:None,checked_at:now()
        });
        if index < 3 {
            state.installations.insert(
                app.id.into(),
                Installation {
                    version: if index == 0 { "0.1.0".into() } else { version },
                    path: PathBuf::from("/preview"),
                    managed: true,
                },
            );
        }
    }
    state.operations.insert(
        "vectorcraft".into(),
        Operation::Downloading {
            received: 54_000_000,
            total: 120_000_000,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};
    fn harness_with_commands() -> (
        Harness<'static, Toolbox>,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
    ) {
        let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (_, events) = mpsc::channel();
        let handle = EngineHandle {
            commands,
            state: std::sync::Arc::new(std::sync::Mutex::new(State::default())),
        };
        preview_state(&handle);
        // No spinner: the keyboard test can reach a stable layout.
        handle.state.lock().unwrap().operations.clear();
        let harness = Harness::builder()
            .with_size([460.0, 720.0])
            .build_eframe(move |cc| Toolbox::new(cc, handle, events, false, true, None, Some(1.0)));
        (harness, receiver)
    }
    fn harness() -> Harness<'static, Toolbox> {
        harness_with_commands().0
    }
    #[test]
    fn rows_launch_and_nested_update_controls_do_not_launch() {
        let (mut h, mut commands) = harness_with_commands();
        h.state_mut().preview = false;
        h.state_mut().tray_attempted = true;
        h.get_by_label("FilmCraft").click();
        h.run_steps(3);
        assert!(matches!(commands.try_recv().unwrap(), Command::Launch(id) if id == "filmcraft"));
        assert!(h.query_by_label("Open").is_none());
        h.get_by_label("Update").click();
        h.run_steps(3);
        assert!(matches!(commands.try_recv().unwrap(), Command::Install(id) if id == "photocraft"));
        assert!(commands.try_recv().is_err());
        h.get_by_label("What's new").click();
        h.run_steps(3);
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn settings_replace_catalog_and_back_restores_it() {
        let mut h = harness();
        h.get_by_label("Settings").click();
        h.run_steps(3);
        assert!(h.state().settings_open);
        assert!(h.get_by_label("Settings").rect().min.y > h.get_by_label("← Back").rect().max.y);
        assert!(h.query_by_label("Search apps").is_none());
        assert!(h.query_by_label("PhotoCraft").is_none());
        assert!(
            h.query_by_label("Automatically update closed apps")
                .is_some()
        );
        h.get_by_label("← Back").click();
        h.run_steps(3);
        assert!(!h.state().settings_open);
        assert!(h.query_by_label("PhotoCraft").is_some());
    }
    #[test]
    fn clickable_controls_show_hand_cursor_and_hover_feedback() {
        let mut h = harness();
        for label in ["Settings", "Check for updates", "Update", "FilmCraft"] {
            h.event(egui::Event::PointerGone);
            h.run_steps(3);
            let before = h.output().shapes.clone();
            h.get_by_label(label).hover();
            h.run_steps(3);
            assert_eq!(
                h.output().platform_output.cursor_icon,
                egui::CursorIcon::PointingHand,
                "{label}"
            );
            assert_ne!(before, h.output().shapes, "{label} has no hover feedback");
        }
        let row = h.get_by_label("FilmCraft").rect();
        assert!(
            row.max.x <= 460.0 - 18.0 - 6.0,
            "row overlaps scrollbar: {row:?}"
        );
    }
    #[test]
    fn escape_returns_from_settings_and_release_notes_without_losing_search() {
        let mut h = harness();
        h.state_mut().search = "Photo".into();
        h.run_steps(3);
        h.get_by_label("Settings").click();
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(!h.state().settings_open);
        assert_eq!(h.state().search, "Photo");
        h.get_by_label("What's new").click();
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.state().details.is_none());
        assert_eq!(h.state().search, "Photo");
    }
    #[test]
    fn keyboard_search_filters_and_reports_empty_results() {
        let mut h = harness();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
        h.run_steps(3);
        h.event(egui::Event::Text("GridCraft".into()));
        h.run_steps(3);
        assert_eq!(h.state().search, "GridCraft");
        assert!(h.query_by_label("GridCraft").is_some());
        assert!(h.query_by_label("PhotoCraft").is_none());
        h.state_mut().search = "does not exist".into();
        h.run_steps(3);
        assert!(
            h.query_by_label("No matching apps. Try a different search.")
                .is_some()
        );
    }
    #[test]
    fn both_themes_render_and_release_notes_open_from_link() {
        let mut h = harness();
        assert_eq!(h.ctx.theme(), egui::Theme::Dark);
        h.get_by_label("What's new").click();
        h.run_steps(3);
        assert_eq!(h.state().details.as_deref(), Some("photocraft"));
        assert!(h.query_by_label("Search apps").is_none());
        assert!(h.query_by_label("FilmCraft").is_none());
        assert!(h.query_by_label("← Back").is_some());
        h.state().handle.state.lock().unwrap().settings.theme = Theme::Light;
        h.run_steps(3);
        assert_eq!(h.ctx.theme(), egui::Theme::Light);
        h.set_pixels_per_point(2.0);
        h.run_steps(3);
        assert!(h.query_by_label("View release on GitHub ↗").is_some());
        // AccessKit bounds are physical pixels; pointer input uses logical points.
        let back = h.get_by_label("← Back").rect().center() / h.ctx.pixels_per_point();
        h.hover_at(back);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: back,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(3);
        assert!(h.state().details.is_none());
        assert!(h.query_by_label("FilmCraft").is_some());
    }
}
