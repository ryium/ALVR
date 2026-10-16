mod components;

use self::components::{
    DevicesTab, LogsTab, NotificationBar, SettingsTab, SetupWizard, SetupWizardRequest,
};
use crate::{
    DataSources,
    dashboard::components::{CloseAction, NewVersionPopup, StatisticsTab},
};
use alvr_common::{
    LogEntry,
    parking_lot::{Condvar, Mutex},
};
use alvr_events::EventType;
use alvr_gui_common::theme;
use alvr_packets::{ClientConnectionsAction, PathValuePair};
use alvr_session::SessionConfig;
use eframe::egui::{
    self, Align, CentralPanel, Color32, Frame, Layout, Margin, Panel, RichText, Ui,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerRequest {
    LaunchSteamvr,
    Log(LogEntry),
    GetSession,
    UpdateSession(Box<SessionConfig>),
    SetSessionValues(Vec<PathValuePair>),
    UpdateClientList {
        hostname: String,
        action: ClientConnectionsAction,
    },
    CaptureFrame,
    InsertIdr,
    StartRecording,
    StopRecording,
    AddFirewallRules,
    RemoveFirewallRules,
    GetDriverList,
    RegisterAlvrDriver,
    UnregisterDriver(PathBuf),
    RestartSteamvr,
    ShutdownSteamvr,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Tab {
    Home,
    Help,
    AdvancedSettings,
    Devices,
    Statistics,
    Settings,
    #[cfg(not(target_arch = "wasm32"))]
    Installation,
    Logs,
    Debug,
    About,
}

pub struct Dashboard {
    frameconnect_tab: components::FrameConnectTab,
    show_advanced: bool,
    data_sources: DataSources,
    just_opened: bool,
    server_restarting: Arc<Mutex<bool>>,
    server_restarting_condvar: Arc<Condvar>,
    selected_tab: Tab,
    tab_labels: BTreeMap<Tab, &'static str>,
    connections_tab: DevicesTab,
    statistics_tab: StatisticsTab,
    settings_tab: SettingsTab,
    #[cfg(not(target_arch = "wasm32"))]
    installation_tab: components::InstallationTab,
    logs_tab: LogsTab,
    notification_bar: NotificationBar,
    setup_wizard: SetupWizard,
    new_version_popup: Option<components::NewVersionPopup>,
    setup_wizard_open: bool,
    session: Option<SessionConfig>,
}

impl Dashboard {
    pub fn new(creation_context: &eframe::CreationContext<'_>, data_sources: DataSources) -> Self {
        alvr_gui_common::theme::set_theme(&creation_context.egui_ctx);
        // Use the system Japanese font; do not redistribute Windows font files.
        #[cfg(windows)]
        if let Ok(font) = std::fs::read("C:/Windows/Fonts/meiryo.ttc") {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("japanese".into(), egui::FontData::from_owned(font).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("japanese".into());
            creation_context.egui_ctx.set_fonts(fonts);
        }

        data_sources.request(ServerRequest::GetSession);

        Self {
            frameconnect_tab: components::FrameConnectTab::default(),
            show_advanced: false,
            data_sources,
            just_opened: true,
            server_restarting: Arc::new(Mutex::new(false)),
            server_restarting_condvar: Arc::new(Condvar::new()),
            selected_tab: Tab::Home,
            tab_labels: [
                (Tab::Home, "デバイス"),
                (Tab::Settings, "設定"),
                (Tab::Help, "ヘルプセンター"),
                (Tab::AdvancedSettings, "ALVRの詳細設定"),
                (Tab::Devices, "デバイスの管理"),
                (Tab::Statistics, "パフォーマンス"),
                #[cfg(not(target_arch = "wasm32"))]
                (Tab::Installation, "ドライバー管理"),
                (Tab::Logs, "システムログ"),
                (Tab::Debug, "デバッグ"),
                (Tab::About, "ライセンス・情報"),
            ]
            .into_iter()
            .collect(),
            connections_tab: DevicesTab::new(),
            statistics_tab: StatisticsTab::new(),
            settings_tab: SettingsTab::new(),
            #[cfg(not(target_arch = "wasm32"))]
            installation_tab: components::InstallationTab::new(),
            logs_tab: LogsTab::new(),
            notification_bar: NotificationBar::new(),
            setup_wizard: SetupWizard::new(),
            setup_wizard_open: false,
            session: None,
            new_version_popup: None,
        }
    }

    // This call may block
    fn restart_steamvr(&self, requests: &mut Vec<ServerRequest>) {
        requests.push(ServerRequest::RestartSteamvr);

        let mut server_restarting_lock = self.server_restarting.lock();

        if *server_restarting_lock {
            self.server_restarting_condvar
                .wait(&mut server_restarting_lock);
        }

        *server_restarting_lock = true;

        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn({
            let server_restarting = Arc::clone(&self.server_restarting);
            let condvar = Arc::clone(&self.server_restarting_condvar);
            move || {
                crate::steamvr_launcher::LAUNCHER.lock().restart_steamvr();

                *server_restarting.lock() = false;
                condvar.notify_one();
            }
        });
    }
}

impl eframe::App for Dashboard {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let mut requests = vec![];

        let connected_to_server = self.data_sources.server_connected();

        while let Some(event) = self.data_sources.poll_event() {
            self.logs_tab.push_event(event.inner.clone());

            match event.inner.event_type {
                EventType::Log(log_event) => {
                    self.notification_bar
                        .push_notification(log_event, event.from_dashboard);
                }
                EventType::GraphStatistics(graph_statistics) => self
                    .statistics_tab
                    .update_graph_statistics(graph_statistics),
                EventType::StatisticsSummary(statistics) => {
                    self.frameconnect_tab.statistics =
                        Some((statistics.clone(), std::time::Instant::now()));
                    self.statistics_tab.update_statistics(statistics)
                }
                EventType::Session(session) => {
                    let settings = session.to_settings();

                    self.connections_tab.update_client_list(&session);
                    self.settings_tab.update_session(&session.session_settings);
                    self.logs_tab.update_settings(&settings);
                    self.notification_bar.update_settings(&settings);
                    if self.just_opened {
                        if settings.extra.open_setup_wizard {
                            self.selected_tab = Tab::Help;
                        }

                        self.just_opened = false;
                    }

                    self.session = Some(*session);
                }
                EventType::ServerRequestsSelfRestart => self.restart_steamvr(&mut requests),
                #[cfg(not(target_arch = "wasm32"))]
                EventType::DriversList(list) => self.installation_tab.update_drivers(list),
                EventType::Adb(adb_event) => self
                    .connections_tab
                    .update_adb_download_progress(adb_event.download_progress),
                EventType::NewVersionFound { version, message } => {
                    if self.show_advanced {
                        self.new_version_popup = Some(NewVersionPopup::new(version, message));
                    }
                }
                EventType::DebugGroup { .. }
                | EventType::Tracking(_)
                | EventType::Buttons(_)
                | EventType::Haptics(_) => (),
            }
        }

        if *self.server_restarting.lock() {
            CentralPanel::default().show(ui, |ui| {
                // todo: find a way to center both vertically and horizontally
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading(RichText::new("SteamVRを再起動しています").size(30.0));
                });
            });

            return;
        }

        self.notification_bar.ui(ui, self.show_advanced);

        if self.setup_wizard_open {
            CentralPanel::default().show(ui, |ui| {
                if let Some(request) = self.setup_wizard.ui(ui) {
                    match request {
                        SetupWizardRequest::ServerRequest(request) => {
                            requests.push(request);
                        }
                        SetupWizardRequest::Close { finished } => {
                            if finished {
                                requests.push(ServerRequest::SetSessionValues(vec![
                                    PathValuePair {
                                        path: alvr_packets::parse_path(
                                            "session_settings.extra.open_setup_wizard",
                                        )
                                        .unwrap(),
                                        value: serde_json::Value::Bool(false),
                                    },
                                ]))
                            }

                            self.setup_wizard_open = false;
                        }
                    }
                }
            });
        } else {
            Panel::left("side_panel")
                .resizable(false)
                .exact_size(204.0)
                .frame(
                    Frame::new()
                        .fill(theme::LIGHTER_BG)
                        .inner_margin(Margin::symmetric(16, 24)),
                )
                .show(ui, |ui| {
                    components::desktop_brand(ui);
                    ui.add_space(34.0);
                    for tab in [Tab::Home, Tab::Settings, Tab::Help] {
                        let selected = match tab {
                            Tab::Home => self.selected_tab == Tab::Home,
                            Tab::Help => self.selected_tab == Tab::Help,
                            _ => !matches!(self.selected_tab, Tab::Home | Tab::Help),
                        };
                        let icon = match tab {
                            Tab::Home => components::DesktopIcon::Headset,
                            Tab::Help => components::DesktopIcon::Help,
                            _ => components::DesktopIcon::Settings,
                        };
                        let color = if selected {
                            theme::ACCENT_LIGHT
                        } else {
                            theme::SUBTEXT
                        };
                        let response = Frame::new()
                            .fill(if selected {
                                theme::ACCENT_BG
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(8)
                            .inner_margin(Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    components::desktop_icon(ui, icon, color);
                                    ui.label(
                                        RichText::new(self.tab_labels[&tab])
                                            .color(color)
                                            .size(14.0),
                                    );
                                });
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            self.selected_tab = tab;
                        }
                        ui.add_space(4.0);
                    }
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        ui.label(
                            RichText::new(format!("Version {}", *alvr_common::ALVR_VERSION))
                                .size(10.0)
                                .color(theme::SUBTEXT),
                        );
                        ui.label(
                            RichText::new("PCストリーミング")
                                .size(11.0)
                                .color(theme::SUBTEXT),
                        );
                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("●")
                                    .color(if connected_to_server {
                                        theme::OK_GREEN
                                    } else {
                                        theme::SUBTEXT
                                    })
                                    .size(10.0),
                            );
                            ui.label(
                                RichText::new(if connected_to_server {
                                    "SteamVR 起動中"
                                } else {
                                    "SteamVR 停止中"
                                })
                                .size(12.0),
                            );
                        });
                    });
                });

            CentralPanel::default()
                .frame(Frame::new().inner_margin(Margin::same(28)).fill(theme::BG))
                .show(ui, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                        if !matches!(self.selected_tab, Tab::Home | Tab::Settings | Tab::Help) {
                            if ui.small_button("‹ 設定に戻る").clicked() {
                                self.selected_tab = Tab::Settings;
                            }
                            ui.add_space(8.0);
                        }
                        ui.label(
                            RichText::new(self.tab_labels[&self.selected_tab])
                                .size(24.0)
                                .strong(),
                        );
                        let subtitle = match self.selected_tab {
                            Tab::Home => "PCとSteam Frameをつなぐ",
                            Tab::Settings => "お好みに合わせてストリーミングを調整",
                            Tab::Help => "接続ガイドとトラブルシューティング",
                            _ => "開発者向けツール",
                        };
                        ui.label(RichText::new(subtitle).color(theme::SUBTEXT).size(12.0));
                        ui.add_space(20.0);
                        match self.selected_tab {
                            Tab::Home => {
                                egui::ScrollArea::vertical().id_salt("device_page").show(
                                    ui,
                                    |ui| {
                                        requests.extend(self.frameconnect_tab.devices_ui(
                                            ui,
                                            self.session.as_ref(),
                                            connected_to_server,
                                        ));
                                    },
                                );
                            }
                            Tab::Settings => {
                                egui::ScrollArea::vertical()
                                    .id_salt("simple_settings_page")
                                    .show(ui, |ui| {
                                        requests.extend(self.frameconnect_tab.settings_ui(
                                            ui,
                                            self.session.as_ref(),
                                            connected_to_server,
                                            self.show_advanced,
                                        ));
                                        ui.add_space(24.0);
                                        ui.collapsing(
                                            RichText::new("開発者向け")
                                                .size(12.0)
                                                .color(theme::SUBTEXT),
                                            |ui| {
                                                ui.checkbox(
                                                    &mut self.show_advanced,
                                                    "ALVRの詳細ツールを表示",
                                                );
                                                if self.show_advanced {
                                                    ui.add_space(8.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        for tab in [
                                                            Tab::AdvancedSettings,
                                                            Tab::Devices,
                                                            Tab::Statistics,
                                                            #[cfg(not(target_arch = "wasm32"))]
                                                            Tab::Installation,
                                                            Tab::Logs,
                                                            Tab::Debug,
                                                            Tab::About,
                                                        ] {
                                                            if ui
                                                                .button(self.tab_labels[&tab])
                                                                .clicked()
                                                            {
                                                                self.selected_tab = tab;
                                                            }
                                                        }
                                                    });
                                                }
                                            },
                                        );
                                    });
                            }
                            Tab::Help => {
                                egui::ScrollArea::vertical()
                                    .id_salt("help_page")
                                    .show(ui, |ui| {
                                        requests.extend(self.frameconnect_tab.help_ui(
                                            ui,
                                            self.session.as_ref(),
                                            connected_to_server,
                                        ));
                                    });
                            }
                            Tab::Devices => {
                                requests.extend(self.connections_tab.ui(ui, connected_to_server));
                            }
                            Tab::Statistics => {
                                if let Some(request) = self.statistics_tab.ui(ui) {
                                    requests.push(request);
                                }
                            }
                            Tab::AdvancedSettings => {
                                requests.extend(self.settings_tab.ui(ui));
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            Tab::Installation => {
                                for request in self.installation_tab.ui(ui) {
                                    match request {
                                        components::InstallationTabRequest::OpenSetupWizard => {
                                            self.setup_wizard_open = true
                                        }
                                        components::InstallationTabRequest::ServerRequest(
                                            request,
                                        ) => requests.push(request),
                                    }
                                }
                            }
                            Tab::Logs => self.logs_tab.ui(ui),
                            Tab::Debug => {
                                if let Some(request) = components::debug_tab_ui(ui) {
                                    requests.push(request);
                                }
                            }
                            Tab::About => components::about_tab_ui(ui),
                        }
                    });
                });
        }

        let shutdown_alvr = || {
            self.data_sources.request(ServerRequest::ShutdownSteamvr);

            crate::steamvr_launcher::LAUNCHER
                .lock()
                .ensure_steamvr_shutdown();
        };

        if let Some(popup) = &self.new_version_popup
            && let Some(action) = popup.ui(ui, shutdown_alvr)
        {
            if let CloseAction::CloseWithRequest(request) = action {
                requests.push(request);
            }

            self.new_version_popup = None;
        }

        for request in requests {
            self.data_sources.request(request);
        }

        if ui.input(|state| state.viewport().close_requested())
            && self.session.as_ref().is_some_and(|s| {
                s.to_settings()
                    .extra
                    .steamvr_launcher
                    .open_close_steamvr_with_dashboard
            })
        {
            shutdown_alvr();
        }
    }
}
