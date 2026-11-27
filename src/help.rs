//! The user guide: the pages in docs/, built into the program and shown in a window.

use eframe::egui::{self, Color32, RichText, text::LayoutJob};

pub const PAGES: &[(&str, &str)] = &[
    ("Getting started", include_str!("../docs/01-getting-started.md")),
    ("Sites", include_str!("../docs/02-sites.md")),
    ("Antennas", include_str!("../docs/03-antennas.md")),
    ("Coverage plots", include_str!("../docs/04-coverage.md")),
    ("Links", include_str!("../docs/05-links.md")),
    ("Radio horizon", include_str!("../docs/06-horizon.md")),
    ("Maps, terrain and data", include_str!("../docs/07-maps-and-data.md")),
    ("Tips and shortcuts", include_str!("../docs/08-tips.md")),
];

/// The page for a tab.
pub fn page_for(tab: &str) -> usize {
    match tab {
        "Sites" => 1,
        "Antennas" => 2,
        "Coverage" => 3,
        "Links" => 4,
        "Horizon" => 5,
        _ => 0,
    }
}

/// The guide window; `open` is the page shown, None when closed.
pub fn window(ctx: &egui::Context, open: &mut Option<usize>) {
    let Some(mut page) = *open else { return };
    let mut keep = true;
    egui::Window::new("Coverant user guide")
        .open(&mut keep)
        .default_size([780.0, 560.0])
        .max_width(ctx.content_rect().width() - 40.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    for (i, (title, _)) in PAGES.iter().enumerate() {
                        if ui.selectable_label(page == i, *title).clicked() {
                            page = i;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt(("help", page)).auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.vertical(|ui| {
                        render(ui, PAGES[page].1);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if page > 0 && ui.button(format!("← {}", PAGES[page - 1].0)).clicked() {
                                page -= 1;
                            }
                            if page + 1 < PAGES.len() && ui.button(format!("{} →", PAGES[page + 1].0)).clicked() {
                                page += 1;
                            }
                        });
                    });
                });
            });
        });
    *open = if keep { Some(page) } else { None };
}

/// A small Markdown subset: # and ## headings, paragraphs, - and 1. lists, **bold** and `code`.
pub fn render(ui: &mut egui::Ui, md: &str) {
    let mut para = String::new();
    let flush = |ui: &mut egui::Ui, para: &mut String| {
        if !para.trim().is_empty() {
            let job = inline(ui, para.trim(), 14.0);
            ui.add(egui::Label::new(job).wrap());
            ui.add_space(6.0);
        }
        para.clear();
    };
    for line in md.lines() {
        let t = line.trim_end();
        if let Some(h) = t.strip_prefix("# ") {
            flush(ui, &mut para);
            ui.label(RichText::new(h).size(22.0).strong());
            ui.add_space(6.0);
        } else if let Some(h) = t.strip_prefix("## ") {
            flush(ui, &mut para);
            ui.add_space(4.0);
            ui.label(RichText::new(h).size(17.0).strong());
            ui.add_space(2.0);
        } else if let Some((marker, rest, indent)) = list_item(t) {
            flush(ui, &mut para);
            ui.horizontal_top(|ui| {
                ui.add_space(8.0 + indent);
                ui.add_sized([18.0, 18.0], egui::Label::new(RichText::new(marker).color(crate::theme::ACCENT)));
                let job = inline(ui, rest, 14.0);
                ui.add(egui::Label::new(job).wrap());
            });
            ui.add_space(2.0);
        } else if t.trim().is_empty() {
            flush(ui, &mut para);
        } else {
            para.push(' ');
            para.push_str(t.trim());
        }
    }
    flush(ui, &mut para);
}

fn list_item(t: &str) -> Option<(String, &str, f32)> {
    let indent = (t.len() - t.trim_start().len()) as f32 * 8.0;
    let s = t.trim_start();
    if let Some(r) = s.strip_prefix("- ") {
        return Some(("•".into(), r, indent));
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        if let Some(r) = s[digits.len()..].strip_prefix(". ") {
            return Some((format!("{digits}."), r, indent));
        }
    }
    None
}

fn inline(ui: &egui::Ui, text: &str, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let body = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let code_bg = ui.visuals().code_bg_color;
    let (mut bold, mut code) = (false, false);
    let mut buf = String::new();
    let push = |job: &mut LayoutJob, buf: &mut String, bold: bool, code: bool| {
        if buf.is_empty() {
            return;
        }
        let fmt = egui::TextFormat {
            font_id: if code { egui::FontId::monospace(size - 1.0) } else { egui::FontId::proportional(size) },
            color: if bold { strong } else { body },
            background: if code { code_bg } else { Color32::TRANSPARENT },
            ..Default::default()
        };
        job.append(buf, 0.0, fmt);
        buf.clear();
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            push(&mut job, &mut buf, bold, code);
            code = !code;
        } else if c == '*' && chars.peek() == Some(&'*') && !code {
            chars.next();
            push(&mut job, &mut buf, bold, code);
            bold = !bold;
        } else {
            buf.push(c);
        }
    }
    push(&mut job, &mut buf, bold, code);
    job
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_page_has_a_title_and_balanced_markup() {
        for (name, md) in super::PAGES {
            assert!(md.starts_with("# "), "{name}");
            for line in md.lines() {
                assert_eq!(line.matches("**").count() % 2, 0, "{name}: {line}");
                assert_eq!(line.matches('`').count() % 2, 0, "{name}: {line}");
            }
        }
    }
}
