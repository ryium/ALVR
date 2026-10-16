use crate::dashboard::ServerRequest;
use alvr_common::{ConnectionState, frameconnect::QUALITY_EYE_SIZE};
use alvr_events::StatisticsSummary;
use alvr_gui_common::theme;
use alvr_packets::{ClientConnectionsAction, PathValuePair, parse_path};
use alvr_session::{CodecType, SessionConfig};
use eframe::egui::{self, Align, Color32, Frame, Layout, Margin, RichText, Stroke, Ui};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct FrameConnectTab {
    pub statistics: Option<(StatisticsSummary, Instant)>,
    export_message: Option<String>,
    settings_page: SettingsPage,
}

fn setting(path: &str, value: serde_json::Value) -> PathValuePair {
    PathValuePair {
        path: parse_path(path).unwrap(),
        value,
    }
}

#[derive(Clone, Copy, PartialEq)]
struct VideoSettings {
    codec: CodecType,
    eye: u32,
    auto: bool,
    fps: f32,
    mbps: u64,
    ten_bit: bool,
    gaze: bool,
}

impl VideoSettings {
    fn from_session(session: &SessionConfig) -> Self {
        let settings = session.to_settings();
        Self {
            codec: settings.video.preferred_codec,
            eye: match settings.video.transcoding_view_resolution {
                alvr_session::FrameSize::Absolute { width, .. } => width,
                _ => QUALITY_EYE_SIZE,
            },
            auto: settings.video.frameconnect_auto_refresh_rate,
            fps: settings.video.preferred_fps,
            mbps: session.session_settings.video.bitrate.mode.ConstantMbps,
            ten_bit: settings.video.encoder_config.use_10bit.unwrap_or(false),
            gaze: settings
                .headset
                .face_tracking
                .as_option()
                .is_some_and(|f| f.sink.eye_tracked_foveated_encoding),
        }
    }
}

fn quality_settings(
    before: VideoSettings,
    after: VideoSettings,
    resolution_selected: bool,
) -> Vec<PathValuePair> {
    let mut values = vec![];
    if before.codec != after.codec {
        values.push(setting(
            "session_settings.video.preferred_codec.variant",
            json!(after.codec),
        ));
    }
    if resolution_selected {
        values.extend([
            setting(
                "session_settings.video.transcoding_view_resolution.variant",
                json!("Absolute"),
            ),
            setting(
                "session_settings.video.transcoding_view_resolution.Absolute.width",
                json!(after.eye),
            ),
            setting(
                "session_settings.video.transcoding_view_resolution.Absolute.height.set",
                json!(true),
            ),
            setting(
                "session_settings.video.transcoding_view_resolution.Absolute.height.content",
                json!(after.eye),
            ),
        ]);
    }
    if before.auto != after.auto {
        values.push(setting(
            "session_settings.video.frameconnect_auto_refresh_rate",
            json!(after.auto),
        ));
    }
    if before.fps != after.fps {
        values.push(setting(
            "session_settings.video.preferred_fps",
            json!(after.fps),
        ));
    }
    if before.mbps != after.mbps {
        values.extend([
            setting(
                "session_settings.video.bitrate.mode.variant",
                json!("ConstantMbps"),
            ),
            setting(
                "session_settings.video.bitrate.mode.ConstantMbps",
                json!(after.mbps),
            ),
        ]);
    }
    if before.ten_bit != after.ten_bit {
        values.extend([
            setting(
                "session_settings.video.encoder_config.use_10bit.set",
                json!(true),
            ),
            setting(
                "session_settings.video.encoder_config.use_10bit.content",
                json!(after.ten_bit),
            ),
        ]);
    }
    if before.gaze != after.gaze {
        values.extend([
            setting(
                "session_settings.video.foveated_encoding.enabled",
                json!(after.gaze),
            ),
            setting(
                "session_settings.headset.face_tracking.enabled",
                json!(after.gaze),
            ),
            setting(
                "session_settings.headset.face_tracking.content.sink.eye_tracked_foveated_encoding",
                json!(after.gaze),
            ),
        ]);
    }
    if !values.is_empty() {
        values.push(setting(
            "frameconnect",
            json!(alvr_session::FrameConnectSession::default()),
        ));
    }
    values
}

#[derive(Default, PartialEq, Eq)]
enum SettingsPage {
    #[default]
    Display,
    Audio,
    General,
}

fn streaming(session: &SessionConfig, server_running: bool) -> bool {
    server_running
        && session
            .client_connections
            .values()
            .any(|c| c.connection_state == ConnectionState::Streaming)
}

fn sorted_devices(
    session: &SessionConfig,
) -> Vec<(&String, &alvr_session::ClientConnectionConfig)> {
    let mut devices: Vec<_> = session.client_connections.iter().collect();
    devices.sort_by_key(|(hostname, client)| {
        let priority = match client.connection_state {
            ConnectionState::Streaming => 0,
            ConnectionState::Connected => 1,
            ConnectionState::Connecting => 2,
            _ if client.current_ip.is_some() && !client.trusted => 3,
            _ if client.current_ip.is_some() => 4,
            _ => 5,
        };
        (priority, *hostname)
    });
    devices
}

fn section(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(theme::SECTION_BG)
        .corner_radius(12)
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
}

fn caption(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).color(theme::SUBTEXT).size(12.0));
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong());
}

fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [206.0, 40.0],
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(theme::ACCENT)
            .stroke(Stroke::NONE),
    )
}

impl FrameConnectTab {
    pub fn devices_ui(
        &mut self,
        ui: &mut Ui,
        session: Option<&SessionConfig>,
        server_running: bool,
    ) -> Vec<ServerRequest> {
        let mut requests = vec![];
        let Some(session) = session else {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                ui.spinner();
                caption(ui, "接続情報を読み込んでいます");
            });
            return requests;
        };
        let streaming = streaming(session, server_running);
        let devices = sorted_devices(session);
        let active_device = devices.first().map(|(_, device)| *device);
        let connecting = server_running
            && active_device.is_some_and(|d| {
                matches!(
                    d.connection_state,
                    ConnectionState::Connecting | ConnectionState::Connected
                )
            });
        let (title, description) = if streaming {
            (
                "Steam Frameに接続しました",
                "ヘッドセットでPCVRをお楽しみください",
            )
        } else if connecting {
            (
                "Steam Frameに接続しています",
                "ヘッドセットを装着して、しばらくお待ちください",
            )
        } else if server_running {
            (
                "接続を待っています",
                "Steam FrameでFrameConnectを開いてください",
            )
        } else {
            (
                "Steam Frameを接続",
                "SteamVRを起動して、PCVRをはじめましょう",
            )
        };

        section(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                let (status, color) = if streaming {
                    ("接続済み", theme::OK_GREEN)
                } else if connecting {
                    ("接続中", theme::ACCENT_LIGHT)
                } else if server_running {
                    ("接続待機中", theme::ACCENT_LIGHT)
                } else {
                    ("未接続", theme::SUBTEXT)
                };
                ui.label(
                    RichText::new(format!("●  {status}"))
                        .color(color)
                        .size(12.0),
                );
                super::headset_illustration(ui, streaming);
                ui.label(RichText::new(title).size(24.0).strong());
                caption(ui, description);
                ui.add_space(8.0);
                if server_running {
                    if ui
                        .add_sized([206.0, 38.0], egui::Button::new("SteamVRを終了"))
                        .clicked()
                    {
                        requests.push(ServerRequest::ShutdownSteamvr);
                    }
                } else if primary_button(ui, "SteamVRを起動").clicked() {
                    requests.push(ServerRequest::LaunchSteamvr);
                }
                ui.add_space(6.0);
            });
        });

        if let Some(reason) = &session.frameconnect.reason {
            ui.add_space(10.0);
            ui.label(RichText::new(reason).color(theme::WARN_YELLOW));
        }
        if let Some((stats, _)) = self
            .statistics
            .as_ref()
            .filter(|(_, t)| streaming && t.elapsed() < Duration::from_secs(3))
        {
            ui.add_space(14.0);
            section(ui, |ui| {
                ui.columns(3, |cols| {
                    cols[0].vertical_centered(|ui| {
                        caption(ui, "フレームレート");
                        ui.label(RichText::new(format!("{} fps", stats.client_fps)).size(22.0));
                    });
                    cols[1].vertical_centered(|ui| {
                        caption(ui, "合計遅延");
                        ui.label(
                            RichText::new(format!("{:.1} ms", stats.total_latency_ms)).size(22.0),
                        );
                    });
                    cols[2].vertical_centered(|ui| {
                        caption(ui, "ビットレート");
                        ui.label(
                            RichText::new(format!("{:.1} Mbps", stats.video_mbits_per_sec))
                                .size(22.0),
                        );
                    });
                });
            });
        }

        if !devices.is_empty() {
            ui.add_space(16.0);
            section_title(ui, "見つかったデバイス");
            for (hostname, device) in devices {
                ui.push_id(hostname, |ui| {
                    section(ui, |ui| {
                        ui.horizontal(|ui| {
                            super::desktop_icon(ui, super::DesktopIcon::Headset, theme::SUBTEXT);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(if device.display_name.is_empty() {
                                        hostname.as_str()
                                    } else {
                                        &device.display_name
                                    })
                                    .strong(),
                                );
                                caption(
                                    ui,
                                    device
                                        .current_ip
                                        .map(|ip| ip.to_string())
                                        .unwrap_or_else(|| "オフライン".into()),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if !device.trusted {
                                    if ui.button("この端末を登録").clicked() {
                                        requests.push(ServerRequest::UpdateClientList {
                                            hostname: hostname.clone(),
                                            action: ClientConnectionsAction::Trust,
                                        });
                                    }
                                } else {
                                    caption(
                                        ui,
                                        if server_running
                                            && device.connection_state == ConnectionState::Streaming
                                        {
                                            "接続済み"
                                        } else {
                                            "登録済み・自動接続"
                                        },
                                    );
                                }
                            });
                        });
                    });
                });
            }
        } else {
            ui.add_space(18.0);
            ui.columns(2, |cols| {
                section(&mut cols[0], |ui| {
                    ui.set_min_height(76.0);
                    ui.horizontal(|ui| {
                        super::desktop_icon(ui, super::DesktopIcon::Network, theme::ACCENT_LIGHT);
                        section_title(ui, "同じネットワークに接続");
                    });
                    caption(
                        ui,
                        "PCは有線LAN、Steam Frameは同じルーターのWi-Fiに接続します。",
                    );
                });
                section(&mut cols[1], |ui| {
                    ui.set_min_height(76.0);
                    ui.horizontal(|ui| {
                        super::desktop_icon(ui, super::DesktopIcon::Headset, theme::ACCENT_LIGHT);
                        section_title(ui, "ヘッドセットで起動");
                    });
                    caption(
                        ui,
                        "Steam FrameでFrameConnectを開き、表示された自分の端末を登録します。",
                    );
                });
            });
        }
        requests
    }

    pub fn settings_ui(
        &mut self,
        ui: &mut Ui,
        session: Option<&SessionConfig>,
        server_running: bool,
        show_experimental: bool,
    ) -> Vec<ServerRequest> {
        let mut requests = vec![];
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.settings_page, SettingsPage::Display, "画面");
            ui.selectable_value(&mut self.settings_page, SettingsPage::Audio, "音声");
            ui.selectable_value(&mut self.settings_page, SettingsPage::General, "一般");
        });
        ui.add_space(16.0);
        let Some(session) = session else {
            ui.spinner();
            return requests;
        };
        let settings = session.to_settings();
        match self.settings_page {
            SettingsPage::Display => {
                if server_running {
                    caption(
                        ui,
                        "画質を変更するには、デバイス画面でSteamVRを終了してください。",
                    );
                    ui.add_space(8.0);
                }
                let before = VideoSettings::from_session(session);
                let VideoSettings {
                    mut codec,
                    mut eye,
                    mut auto,
                    mut fps,
                    mut mbps,
                    mut ten_bit,
                    mut gaze,
                } = before;
                let mut resolution_selected = false;
                ui.add_enabled_ui(!server_running, |ui| {
                    section(ui, |ui| {
                        section_title(ui, "画質");
                        caption(ui, "片眼あたりの解像度を選択します。");
                        ui.add_space(10.0);
                        ui.columns(3, |cols| {
                            for (column, (size, title)) in cols.iter_mut().zip([(2176, "標準"), (2560, "高精細"), (2880, "最高精細")]) {
                                let selected = matches!(settings.video.transcoding_view_resolution, alvr_session::FrameSize::Absolute { width, height: Some(height) } if width == size && height == size);
                                let button = egui::Button::new(RichText::new(format!("{title}\n{size} × {size}")).size(13.0)).fill(if selected { theme::ACCENT_BG } else { theme::LIGHTER_BG }).stroke(Stroke::new(1.0, if selected { theme::ACCENT } else { theme::SEPARATOR_BG }));
                                if column.add_sized([column.available_width(), 76.0], button).clicked() { eye = size; resolution_selected = true; }
                            }
                        });
                        if !matches!(settings.video.transcoding_view_resolution, alvr_session::FrameSize::Absolute { width: width @ (2176 | 2560 | 2880), height: Some(height) } if width == height) {
                            caption(ui, "現在はカスタム解像度を使用しています。");
                        }
                    });
                    ui.add_space(14.0);
                    section(ui, |ui| {
                        section_title(ui, "ビットレート");
                        ui.scope(|ui| {
                            ui.visuals_mut().widgets.inactive.bg_fill = theme::SEPARATOR_BG;
                            ui.visuals_mut().selection.bg_fill = theme::ACCENT;
                            ui.add(egui::Slider::new(&mut mbps, 20..=200).clamping(egui::SliderClamping::Edits).trailing_fill(true).suffix(" Mbps"));
                        });
                        caption(ui, "高い値ほど映像が鮮明になります。通信が不安定なときは値を下げてください。");
                        ui.add_space(14.0);
                        section_title(ui, "リフレッシュレート");
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(auto, "自動").clicked() { auto = true; fps = 144.0; }
                            for hz in [90.0, 120.0, 144.0] {
                                if ui.selectable_label(!auto && (fps - hz).abs() < 0.5, format!("{hz:.0} Hz")).clicked() { auto = false; fps = hz; }
                            }
                        });
                        caption(ui, "自動では端末の対応状況と配信負荷に合わせて表示周期を調整します。");
                        ui.add_space(14.0);
                        section_title(ui, "映像形式");
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(&mut codec, CodecType::AV1, "AV1");
                            ui.selectable_value(&mut codec, CodecType::Hevc, "HEVC (H.265)");
                            if codec == CodecType::H264 { caption(ui, "現在の設定: H.264"); }
                        });
                        caption(ui, "PCとヘッドセットの両方が対応する形式を選んでください。");
                    });
                    if show_experimental {
                        ui.add_space(14.0);
                        section(ui, |ui| {
                            section_title(ui, "試験機能");
                            ui.checkbox(&mut ten_bit, "10bitカラーを使用する");
                            let gaze_available = session.frameconnect.negotiated.as_ref().is_some_and(|n| n.capabilities.eye_gaze);
                            ui.add_enabled_ui(gaze_available || gaze, |ui| { ui.checkbox(&mut gaze, "視線追従の中心窩エンコード"); });
                            caption(ui, "端末の対応が必要です。非対応の場合は配信を開始できません。");
                            if ui.button("表示周期を再評価").clicked() {
                                requests.push(ServerRequest::SetSessionValues(vec![setting("frameconnect.rate_ceiling", json!(null)), setting("frameconnect.reason", json!(null))]));
                            }
                        });
                    }
                });
                let changes = quality_settings(
                    before,
                    VideoSettings {
                        codec,
                        eye,
                        auto,
                        fps,
                        mbps,
                        ten_bit,
                        gaze,
                    },
                    resolution_selected,
                );
                if !changes.is_empty() {
                    requests.push(ServerRequest::SetSessionValues(changes));
                }
            }
            SettingsPage::Audio => {
                section(ui, |ui| {
                    section_title(ui, "ゲーム音声");
                    let mut enabled = settings.audio.game_audio.as_option().is_some();
                    if ui
                        .checkbox(&mut enabled, "PCの音声をヘッドセットに送る")
                        .changed()
                    {
                        requests.push(ServerRequest::SetSessionValues(vec![setting(
                            "session_settings.audio.game_audio.enabled",
                            json!(enabled),
                        )]));
                    }
                    caption(ui, "PCのゲーム音声をヘッドセットで再生します。");
                });
            }
            SettingsPage::General => {
                section(ui, |ui| {
                    section_title(ui, "SteamVR");
                    let mut automatic = settings
                        .extra
                        .steamvr_launcher
                        .open_close_steamvr_with_dashboard;
                    if ui
                        .checkbox(
                            &mut automatic,
                            "FrameConnectと一緒にSteamVRを起動・終了する",
                        )
                        .changed()
                    {
                        requests.push(ServerRequest::SetSessionValues(vec![setting("session_settings.extra.steamvr_launcher.open_close_steamvr_with_dashboard", json!(automatic))]));
                    }
                });
                ui.add_space(14.0);
                section(ui, |ui| {
                    section_title(ui, "FrameConnect");
                    caption(ui, format!("バージョン {}", session.server_version));
                    caption(
                        ui,
                        "ALVRを基盤とするSteam Frame向けPCVRストリーミングアプリです。",
                    );
                });
            }
        }
        requests
    }

    pub fn help_ui(
        &mut self,
        ui: &mut Ui,
        session: Option<&SessionConfig>,
        server_running: bool,
    ) -> Vec<ServerRequest> {
        let mut requests = vec![];
        section(ui, |ui| {
            section_title(ui, "接続のはじめかた");
            for (step, title, detail) in [
                (
                    "1",
                    "PCとヘッドセットを同じネットワークに接続",
                    "PCは有線LANでルーターへ、Steam FrameはWi-Fiで接続します。",
                ),
                (
                    "2",
                    "両方でFrameConnectを起動",
                    "PCのデバイス画面でSteamVRを起動し、Steam FrameでもFrameConnectを開きます。",
                ),
                (
                    "3",
                    "見つかった自分の端末を登録",
                    "デバイス画面の「この端末を登録」を押します。登録後は自動接続します。",
                ),
            ] {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(step).size(19.0).color(theme::ACCENT_LIGHT));
                    ui.vertical(|ui| {
                        ui.label(title);
                        caption(ui, detail);
                    });
                });
            }
        });
        ui.add_space(14.0);
        section(ui, |ui| {
            section_title(ui, "初回セットアップ");
            caption(
                ui,
                "初めて使うPCでは、ドライバーの登録とファイアウォールの設定を行ってください。",
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("SteamVRドライバーを登録").clicked() {
                    requests.push(ServerRequest::RegisterAlvrDriver);
                }
                if ui.button("Windowsファイアウォールを設定").clicked() {
                    requests.push(ServerRequest::AddFirewallRules);
                }
            });
            if session.is_some_and(|s| s.to_settings().extra.open_setup_wizard)
                && ui.button("セットアップを完了").clicked()
            {
                requests.push(ServerRequest::SetSessionValues(vec![setting(
                    "session_settings.extra.open_setup_wizard",
                    json!(false),
                )]));
            }
        });
        ui.add_space(14.0);
        section(ui, |ui| {
            section_title(ui, "接続できないとき");
            caption(
                ui,
                "同じFrameConnectビルドをPCとヘッドセットで使用し、ネットワークとファイアウォールを確認してください。",
            );
            if server_running && ui.button("SteamVRを再起動").clicked() {
                requests.push(ServerRequest::RestartSteamvr);
            }
            let Some(session) = session else {
                return;
            };
            let live = streaming(session, server_running);
            ui.collapsing("接続の詳細・診断", |ui| {
                if let Some(n) = &session.frameconnect.negotiated {
                    ui.label(format!("{}: {} · {}bit · 片眼 {} × {} · {:.0} Hz", if live { "接続設定" } else { "前回の接続設定" }, n.codec, if n.ten_bit { 10 } else { 8 }, n.eye_width, n.eye_height, n.refresh_rate));
                    caption(ui, format!("デコーダー: {}", n.decoder));
                } else { caption(ui, "接続情報はまだありません。"); }
                if let Some(reason) = &session.frameconnect.reason { ui.label(RichText::new(reason).color(theme::WARN_YELLOW)); }
                if ui.button("診断JSONを保存").clicked() {
                    let path = crate::get_filesystem_layout().log_dir.join("frameconnect-diagnostics.json");
                    let report = json!({"schema_version": 1, "session": session, "streaming": live, "statistics": self.statistics.as_ref().filter(|(_, t)| live && t.elapsed() < Duration::from_secs(3)).map(|(s, _)| s)});
                    self.export_message = Some(match std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()) {
                        Ok(()) => format!("保存しました: {}", path.display()),
                        Err(e) => format!("保存失敗: {e}"),
                    });
                }
                if let Some(message) = &self.export_message { ui.label(message); }
            });
        });
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_change_preserves_other_settings() {
        let session = SessionConfig::default();
        let before = VideoSettings::from_session(&session);
        let changes = quality_settings(
            before,
            VideoSettings {
                mbps: before.mbps + 10,
                ..before
            },
            false,
        );
        let paths: Vec<_> = changes
            .iter()
            .map(|p| {
                p.path
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                "session_settings.video.bitrate.mode.variant",
                "session_settings.video.bitrate.mode.ConstantMbps",
                "frameconnect",
            ]
        );
        assert!(quality_settings(before, before, false).is_empty());
    }

    #[test]
    fn active_device_precedes_saved_and_discovered_devices() {
        use alvr_session::ClientConnectionConfig;
        let mut session = SessionConfig::default();
        for (hostname, state) in [
            ("saved", ConnectionState::Disconnected),
            ("streaming", ConnectionState::Streaming),
            ("connecting", ConnectionState::Connecting),
        ] {
            session.client_connections.insert(
                hostname.into(),
                ClientConnectionConfig {
                    display_name: hostname.into(),
                    current_ip: None,
                    manual_ips: Default::default(),
                    trusted: true,
                    connection_state: state,
                },
            );
        }
        let devices = sorted_devices(&session);
        assert_eq!(devices[0].0, "streaming");
        assert_eq!(devices[1].0, "connecting");
        assert!(!streaming(&session, false));
        assert!(streaming(&session, true));
    }

    #[test]
    fn preset_paths_apply_to_real_session_schema() {
        let mut value = serde_json::to_value(SessionConfig::default()).unwrap();
        for setting in quality_settings(
            VideoSettings::from_session(&SessionConfig::default()),
            VideoSettings {
                codec: CodecType::AV1,
                eye: 2560,
                auto: true,
                fps: 144.0,
                mbps: 80,
                ten_bit: false,
                gaze: false,
            },
            true,
        ) {
            let mut current = &mut value;
            for segment in setting.path {
                match segment {
                    alvr_packets::PathSegment::Name(name) => {
                        assert!(current.get(&name).is_some(), "missing {name}");
                        current = &mut current[&name];
                    }
                    alvr_packets::PathSegment::Index(index) => current = &mut current[index],
                }
            }
            *current = setting.value;
        }
        let session: SessionConfig = serde_json::from_value(value).unwrap();
        let settings = session.to_settings();
        assert_eq!(settings.video.preferred_codec, CodecType::AV1);
        assert!(settings.video.frameconnect_auto_refresh_rate);
        assert!(matches!(
            settings.video.transcoding_view_resolution,
            alvr_session::FrameSize::Absolute {
                width: 2560,
                height: Some(2560)
            }
        ));
        assert!(settings.audio.microphone.as_option().is_none());
    }
}
