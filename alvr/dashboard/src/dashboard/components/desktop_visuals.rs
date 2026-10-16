use alvr_gui_common::theme;
use eframe::egui::{
    self, Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, vec2,
};

#[derive(Clone, Copy)]
pub enum DesktopIcon {
    Headset,
    Settings,
    Help,
    Network,
}

pub fn desktop_icon(ui: &mut Ui, icon: DesktopIcon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let origin = rect.min;
    let point = |x, y| origin + vec2(x, y);
    let stroke = Stroke::new(1.5, color);
    match icon {
        DesktopIcon::Headset => {
            painter.rect_stroke(
                Rect::from_min_max(point(2.0, 6.0), point(18.0, 16.0)),
                3,
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment([point(6.0, 6.0), point(7.0, 3.0)], stroke);
            painter.line_segment([point(14.0, 6.0), point(13.0, 3.0)], stroke);
            painter.line_segment([point(6.0, 11.0), point(8.0, 11.0)], stroke);
            painter.line_segment([point(12.0, 11.0), point(14.0, 11.0)], stroke);
        }
        DesktopIcon::Settings => {
            for (x, y) in [(4.0, 7.0), (10.0, 13.0), (16.0, 8.0)] {
                painter.line_segment([point(x, 3.0), point(x, 17.0)], stroke);
                painter.circle_filled(point(x, y), 2.5, theme::LIGHTER_BG);
                painter.circle_stroke(point(x, y), 2.5, stroke);
            }
        }
        DesktopIcon::Help => {
            painter.circle_stroke(rect.center(), 8.0, stroke);
            painter.text(
                point(10.0, 10.0),
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(14.0),
                color,
            );
        }
        DesktopIcon::Network => {
            for radius in [5.0, 9.0, 13.0] {
                let points = (0..=16)
                    .map(|i| {
                        let angle = std::f32::consts::PI * (1.22 + i as f32 / 16.0 * 0.56);
                        point(10.0 + radius * angle.cos(), 17.0 + radius * angle.sin())
                    })
                    .collect();
                painter.add(Shape::line(points, stroke));
            }
            painter.circle_filled(point(10.0, 17.0), 1.5, color);
        }
    }
}

pub fn desktop_brand(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 9.0;
        let (rect, _) = ui.allocate_exact_size(vec2(25.0, 28.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(
            Rect::from_min_size(rect.min + vec2(1.0, 3.0), vec2(8.0, 22.0)),
            3,
            theme::ACCENT,
        );
        painter.rect_filled(
            Rect::from_min_size(rect.min + vec2(12.0, 3.0), vec2(12.0, 8.0)),
            3,
            theme::ACCENT_LIGHT,
        );
        painter.rect_filled(
            Rect::from_min_size(rect.min + vec2(12.0, 14.0), vec2(8.0, 8.0)),
            3,
            theme::ACCENT,
        );
        ui.label(
            egui::RichText::new("FrameConnect")
                .size(17.0)
                .strong()
                .color(theme::FG),
        );
    });
}

/// A scalable, code-native illustration; it contains no inferred device telemetry.
pub fn headset_illustration(ui: &mut Ui, connected: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 182.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let scale = (rect.width() / 390.0).min(1.0);
    let center = rect.center() + vec2(0.0, 5.0);
    let point = |x, y| center + vec2(x, y) * scale;
    let bounds = |x, y, w, h| Rect::from_min_size(point(x, y), vec2(w, h) * scale);

    painter.add(Shape::ellipse_filled(
        point(0.0, 66.0),
        vec2(139.0, 12.0) * scale,
        Color32::from_black_alpha(35),
    ));
    painter.rect_stroke(
        bounds(-60.0, -74.0, 120.0, 95.0),
        30,
        Stroke::new(12.0 * scale, Color32::from_rgb(94, 99, 111)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        bounds(-27.0, -78.0, 54.0, 35.0),
        12,
        Color32::from_rgb(73, 78, 91),
    );
    painter.rect_filled(
        bounds(-112.0, -31.0, 224.0, 89.0),
        29,
        Color32::from_rgb(169, 176, 190),
    );
    painter.rect_filled(
        bounds(-109.0, -36.0, 218.0, 86.0),
        27,
        Color32::from_rgb(225, 230, 239),
    );
    painter.rect_filled(
        bounds(-100.0, -27.0, 200.0, 60.0),
        20,
        Color32::from_rgb(50, 55, 66),
    );
    painter.rect_filled(
        bounds(-92.0, -22.0, 184.0, 45.0),
        15,
        Color32::from_rgb(60, 66, 80),
    );
    painter.line_segment(
        [point(-78.0, -17.0), point(52.0, -17.0)],
        Stroke::new(1.0, Color32::from_rgb(99, 108, 126)),
    );
    for x in [-74.0, 74.0] {
        painter.circle_filled(point(x, 3.0), 7.0 * scale, Color32::from_rgb(29, 33, 42));
        painter.circle_filled(point(x, 3.0), 3.0 * scale, Color32::from_rgb(88, 105, 133));
    }
    painter.circle_filled(
        point(0.0, 42.0),
        2.0 * scale,
        if connected {
            theme::OK_GREEN
        } else {
            Color32::from_rgb(134, 145, 163)
        },
    );
    controller(&painter, point(-147.0, 18.0), scale, -0.18);
    controller(&painter, point(147.0, 18.0), scale, 0.18);
}

fn controller(painter: &Painter, center: Pos2, scale: f32, angle: f32) {
    let point = |x: f32, y: f32| {
        center
            + vec2(
                x * angle.cos() - y * angle.sin(),
                x * angle.sin() + y * angle.cos(),
            ) * scale
    };
    painter.add(Shape::convex_polygon(
        vec![
            point(-12.0, 0.0),
            point(12.0, 0.0),
            point(9.0, 44.0),
            point(-6.0, 47.0),
        ],
        Color32::from_rgb(183, 191, 205),
        Stroke::NONE,
    ));
    painter.circle_filled(center, 20.0 * scale, Color32::from_rgb(219, 225, 235));
    painter.circle_filled(center, 15.0 * scale, Color32::from_rgb(51, 57, 71));
    painter.circle_filled(
        point(-4.0, -3.0),
        5.0 * scale,
        Color32::from_rgb(105, 116, 136),
    );
    painter.circle_filled(
        point(7.0, 4.0),
        2.5 * scale,
        Color32::from_rgb(159, 172, 193),
    );
}
