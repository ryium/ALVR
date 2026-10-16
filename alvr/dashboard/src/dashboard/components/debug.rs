use crate::dashboard::ServerRequest;
use crate::dashboard::i18n::Language;
use eframe::egui::Ui;

pub fn debug_tab_ui(ui: &mut Ui) -> Option<ServerRequest> {
    let language = Language::from_context(ui.ctx());
    let mut request = None;

    ui.label(language.text(
        "Recording from ALVR using the buttons below is not suitable for capturing gameplay.
For that, use other means of recording, for example through headset or desktop VR output.",
    ));

    ui.columns(4, |ui| {
        if ui[0].button(language.text("Capture frame")).clicked() {
            request = Some(ServerRequest::CaptureFrame);
        }

        if ui[1].button(language.text("Insert IDR")).clicked() {
            request = Some(ServerRequest::InsertIdr);
        }

        if ui[2].button(language.text("Start recording")).clicked() {
            request = Some(ServerRequest::StartRecording);
        }

        if ui[3].button(language.text("Stop recording")).clicked() {
            request = Some(ServerRequest::StopRecording);
        }
    });

    request
}
