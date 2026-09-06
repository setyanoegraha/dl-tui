//! Pure state -> widget rendering for the dashboard. Nord theme, UI text in
//! Spanish.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, LineGauge, Paragraph, Row, Table, TableState,
    Tabs, Wrap,
};
use ratatui::Frame;

use crate::modules::machines::{dificultad_rank, Machine};
use crate::modules::ratings::MachineRating;

use super::{ActionReport, AppState, InputMode, Popup, PopupKind, ReportKind, Tab, ViewMode, WriteupsPopup, downloads::Phase};

// Nord theme palette (https://www.nordtheme.com/docs/colors-and-palettes).
const NORD1: Color = Color::Rgb(0x3B, 0x42, 0x52); // polar night (dim bg)
const NORD6: Color = Color::Rgb(0xEC, 0xEF, 0xF4); // snow storm (bright text)
const NORD7: Color = Color::Rgb(0x8F, 0xBC, 0xBB); // frost (teal)
const NORD8: Color = Color::Rgb(0x88, 0xC0, 0xD0); // frost (accent blue)
const NORD10: Color = Color::Rgb(0x5E, 0x81, 0xAC); // frost (links)
const NORD11: Color = Color::Rgb(0xBF, 0x61, 0x6A); // aurora red
const NORD13: Color = Color::Rgb(0xEB, 0xCB, 0x8B); // aurora yellow
const NORD14: Color = Color::Rgb(0xA3, 0xBE, 0x8C); // aurora green
const NORD15: Color = Color::Rgb(0xB4, 0x8E, 0xAD); // aurora purple

const ACCENT: Color = NORD8; // titles, active tab, gauges, borders
const WARN: Color = NORD13; // notices, non-final states
const OK: Color = NORD14; // success, completada
const BAD: Color = NORD11; // failures
const FROST: Color = NORD7; // videos, secondary accents
const PURPLE: Color = NORD15; // articles
const BRIGHT: Color = NORD6; // names, primary text
const LINK: Color = NORD10; // writeup URLs
const HL_BG: Color = NORD1; // selected-row background

pub fn draw(frame: &mut Frame, app: &mut AppState) {
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    draw_tabs(frame, tabs, app);

    match app.tab {
        Tab::Maquinas => draw_maquinas(frame, body, app),
        Tab::Progreso => draw_progreso(frame, body, app),
        Tab::Writeups => draw_writeups_tab(frame, body, app),
    }

    draw_footer(frame, footer, app);

    if app.view == ViewMode::Downloads && app.popup.is_none() && app.report.is_none() {
        draw_downloads(frame, frame.area(), app);
    }
    if let Some(popup) = &app.popup {
        draw_popup(frame, frame.area(), popup);
    }
    if let Some(report) = &app.report {
        draw_report(frame, frame.area(), report);
    }
    if app.writeups_popup.is_some() {
        if let Some(popup) = app.writeups_popup.clone() {
            draw_writeups_popup(frame, frame.area(), &popup);
        }
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let profile = &app.data.profile;
    let pct = profile.progreso.porcentaje.round() as u64;
    let line = Line::from(vec![
        Span::styled(" DockerLabs", Style::new().fg(ACCENT).bold()),
        Span::styled(" dashboard", Style::new().dim()),
        Span::raw("  ·  "),
        Span::styled(
            format!("{} ", profile.username),
            Style::new().fg(BRIGHT).bold(),
        ),
        Span::raw("  ·  "),
        Span::styled(
            format!(
                "{}/{} máquinas",
                profile.progreso.maquinas_hechas, profile.progreso.maquinas_totales
            ),
            Style::new().fg(WARN),
        ),
        Span::styled(format!(" ({pct}%)"), Style::new().fg(ACCENT)),
        Span::raw("  ·  "),
        Span::styled(
            format!("{} writeups", profile.writeups.len()),
            Style::new().fg(FROST),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &AppState) {
    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
    let index = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(index)
        // Dim the inactive tabs so the highlighted one stands out.
        .style(Style::new().dim())
        .highlight_style(Style::new().fg(ACCENT).bold().underlined());
    frame.render_widget(tabs, area);
}

fn hex_color(hex: &str) -> Option<Color> {
    let h = hex.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Difficulty color: the site's own hex when available, else the official
/// DockerLabs palette per difficulty rank.
fn difficulty_color(machine: &Machine) -> Color {
    hex_color(&machine.color).unwrap_or_else(|| match dificultad_rank(&machine.dificultad) {
        0 => Color::Rgb(0x43, 0x95, 0x9b), // Muy Fácil (teal)
        1 => Color::Rgb(0x8b, 0xc3, 0x4a), // Fácil (green)
        2 => Color::Rgb(0xe0, 0xa5, 0x53), // Medio (amber)
        3 => Color::Rgb(0xd8, 0x3c, 0x31), // Difícil (red)
        _ => BRIGHT,
    })
}

fn draw_maquinas(frame: &mut Frame, area: Rect, app: &mut AppState) {
    let visible = app.visible_machines();
    let completed = app.data.completed_names();
    let header = Row::new(["Máquina", "Dificultad", "Categoría", "Autor", "Fecha", "✓"])
        .style(Style::new().fg(ACCENT).bold());

    let rows: Vec<Row> = visible
        .iter()
        .map(|m| {
            let done = completed.contains(&m.name.trim().to_lowercase());
            Row::new([
                Span::styled(m.name.clone(), Style::new().fg(BRIGHT)),
                Span::styled(m.dificultad.clone(), Style::new().fg(difficulty_color(m)).bold()),
                Span::raw(m.categoria.clone()),
                Span::styled(m.autor.clone(), Style::new().fg(FROST)),
                Span::styled(m.fecha.clone(), Style::new().dim()),
                Span::styled(if done { "✔" } else { "" }, Style::new().fg(OK).bold()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(11),
            Constraint::Length(20),
            Constraint::Length(22),
            Constraint::Length(11),
            Constraint::Length(3),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(HL_BG).add_modifier(Modifier::BOLD))
    .block(filter_block(app));

    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);

    app.set_visible_rows(visible_rows_in(area.height));
}

fn filter_block(app: &AppState) -> Block<'_> {
    let count_line = match app.tab {
        Tab::Maquinas => format!(
            " Máquinas {}/{}{} ",
            app.visible_machines().len(),
            app.data.catalog.len(),
            app.machine_sort.indicator()
        ),
        Tab::Progreso => format!(
            " Completadas {}/{} ",
            app.visible_hechas().len(),
            app.data.profile.maquinas_hechas.len()
        ),
        Tab::Writeups => format!(
            " Writeups {}/{} ",
            app.visible_own_writeups().len(),
            app.data.profile.writeups.len()
        ),
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().dim());

    if app.input_mode == InputMode::Filter {
        block = block
            .title(Span::styled(
                format!(" filtro: {}▏", app.filter),
                Style::new().fg(WARN).bold(),
            ))
            .title_position(ratatui::widgets::block::Position::Top)
            .border_style(Style::new().fg(WARN));
    } else if !app.filter.is_empty() {
        block = block.title(Span::styled(
            format!(" filtro: {} ", app.filter),
            Style::new().fg(WARN),
        ));
    }

    if !count_line.is_empty() {
        block = block.title_bottom(Span::styled(count_line, Style::new().dim()));
    }
    block
}

/// Difficulty progress in a fixed, canonical order; falls back through the
/// spelling variants the database is known to mix ('Fácil'/'Facil').
fn dificultad_progreso(
    profile: &crate::modules::profile::Profile,
    names: &[&str],
) -> (u64, u64) {
    for name in names {
        if let Some(d) = profile.progreso.por_dificultad.get(*name) {
            return (d.hechas, d.totales);
        }
    }
    (0, 0)
}

fn draw_progreso(frame: &mut Frame, area: Rect, app: &mut AppState) {
    let profile = &app.data.profile;
    let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)])
        .areas(area);

    // ---- left: stacked gauges + perfil + statistics ---------------------
    let title = Paragraph::new(Span::styled(
        "[ Progreso ]",
        Style::new().fg(ACCENT).bold(),
    ));
    frame.render_widget(title, left);

    let mut rows: Vec<(String, u64, u64)> = vec![(
        "Total".to_string(),
        profile.progreso.maquinas_hechas,
        profile.progreso.maquinas_totales,
    )];
    for (label, variants) in [
        ("Muy Fácil", &["Muy Fácil", "Muy Facil", "muy-facil"][..]),
        ("Fácil", &["Fácil", "Facil", "facil"][..]),
        ("Medio", &["Medio", "medio"][..]),
        ("Difícil", &["Difícil", "Dificil", "dificil"][..]),
    ] {
        let (hechas, totales) = dificultad_progreso(profile, variants);
        rows.push((label.to_string(), hechas, totales));
    }

    // Gauges stacked one per row, snug under the title.
    let mut y = left.y + 1;
    for (label, hechas, totales) in &rows {
        if y + 1 > left.bottom() {
            break;
        }
        let slot = Rect {
            x: left.x + 2,
            y,
            width: left.width.saturating_sub(4).max(16),
            height: 1,
        };
        let ratio = if *totales == 0 {
            0.0
        } else {
            (*hechas as f64) / (*totales as f64)
        };
        let percent = (ratio * 100.0).round() as u64;
        let gauge = LineGauge::default()
            .label(Span::styled(
                format!("{label} {hechas}/{totales} ({percent}%)"),
                Style::new().fg(BRIGHT),
            ))
            .ratio(ratio.clamp(0.0, 1.0))
            .filled_style(Style::new().fg(ACCENT));
        frame.render_widget(gauge, slot);
        y += 1;
    }

    // Perfil + Estadísticas as one paragraph filling the rest — nothing
    // gets clipped and Perfil sits snug under the gauges.
    let rest_y = (y + 1).min(left.bottom());
    let rest_height = left.bottom().saturating_sub(rest_y);
    let member_since = profile
        .perfil
        .miembro_desde
        .split('T')
        .next()
        .unwrap_or("-")
        .to_string();
    let diploma_name = profile
        .perfil
        .nombre_diplomas
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&profile.username);
    let mut lines = vec![
        Line::from(Span::styled("[ Perfil ]", Style::new().fg(ACCENT).bold())),
        Line::from(format!("  Nombre (diplomas): {diploma_name}")),
        Line::from(format!("  Miembro desde: {member_since}")),
        Line::from(format!(
            "  Biografía    : {}",
            if profile.perfil.biografia.is_empty() {
                "-"
            } else {
                &profile.perfil.biografia
            }
        )),
        Line::from(""),
        Line::from(Span::styled(
            "[ Estadísticas ]",
            Style::new().fg(ACCENT).bold(),
        )),
        Line::from(format!(
            "  Puntos writeups : {}",
            profile.estadisticas.puntos_writeups
        )),
        Line::from(format!(
            "  Ranking writeups: #{}",
            profile.estadisticas.ranking_writeups
        )),
        Line::from(format!(
            "  Ranking creadores: {}",
            app.data
                .ranking_creador
                .map(|n| format!("#{n}"))
                .unwrap_or_else(|| format!("#{}", profile.estadisticas.ranking_creadores))
        )),
    ];
    let rest_area = Rect {
        x: left.x + 2,
        y: rest_y,
        width: left.width.saturating_sub(4),
        height: rest_height,
    };
    if rest_height > 0 {
        lines.truncate(rest_height as usize);
        frame.render_widget(Paragraph::new(lines), rest_area);
    }

    // ---- right: completed machines, aligned like the Máquinas table ----
    let visible = app.visible_hechas();
    let header = Row::new(["Máquina", "Completada", "Certificado"])
        .style(Style::new().fg(ACCENT).bold());
    let rows: Vec<Row> = visible
        .iter()
        .map(|m| {
            let date = m
                .completada_el
                .as_deref()
                .and_then(|d| d.split('T').next())
                .unwrap_or("-")
                .to_string();
            let cert = match &m.certificado {
                Some(cert) => Span::styled(cert.cert_id.clone(), Style::new().fg(OK)),
                None => Span::styled("-", Style::new().dim()),
            };
            Row::new([
                Span::styled(m.nombre.clone(), Style::new().fg(BRIGHT).bold()),
                Span::styled(date, Style::new().dim()),
                cert,
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(HL_BG).add_modifier(Modifier::BOLD))
    .block(filter_block(app));

    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, right, &mut state);

    if visible.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "Nada todavía — marca máquinas con m en Máquinas.",
            Style::new().dim(),
        ));
        frame.render_widget(empty, right);
    }

    app.set_visible_rows(visible_rows_in(right.height));
}

fn draw_writeups_tab(frame: &mut Frame, area: Rect, app: &mut AppState) {
    let visible = app.visible_own_writeups();
    let header = Row::new(["Máquina", "Tipo", "Publicada", "URL"])
        .style(Style::new().fg(ACCENT).bold());
    let rows: Vec<Row> = visible
        .iter()
        .map(|w| {
            let (tipo, tipo_style) = if w.tipo.contains("video") {
                ("🎥", Style::new().fg(FROST))
            } else {
                ("📝", Style::new().fg(PURPLE))
            };
            let date = w
                .publicado_el
                .as_deref()
                .and_then(|d| d.split('T').next())
                .unwrap_or("-")
                .to_string();
            Row::new([
                Span::styled(w.maquina.clone(), Style::new().fg(BRIGHT).bold()),
                Span::styled(tipo, tipo_style),
                Span::styled(date, Style::new().dim()),
                Span::styled(w.url.clone(), Style::new().fg(LINK)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .row_highlight_style(Style::new().bg(HL_BG).add_modifier(Modifier::BOLD))
    .block(filter_block(app));

    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);

    app.set_visible_rows(visible_rows_in(area.height));
}

fn draw_popup(frame: &mut Frame, area: Rect, popup: &Popup) {
    match popup.kind {
        PopupKind::Account => {
            let box_area = popup_area(area, 56, 7);
            frame.render_widget(Clear, box_area);
            let username = if popup.machine.is_empty() { "-" } else { &popup.machine };
            let lines = vec![
                Line::from(Span::styled(
                    format!("Sesión iniciada como {username}"),
                    Style::new().fg(OK).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Enter cambiar cuenta · l cerrar sesión · Esc cerrar",
                    Style::new().dim(),
                )),
            ];
            let block = Block::bordered()
                .title(Span::styled(
                    format!(" Cuenta — {username} "),
                    Style::new().fg(OK).bold(),
                ))
                .border_style(Style::new().fg(OK));
            frame.render_widget(Paragraph::new(lines).block(block), box_area);
            return;
        }
        PopupKind::Descripcion => {
            let width = area.width.saturating_sub(8).max(40);
            let box_area = popup_area(area, width, 14);
            frame.render_widget(Clear, box_area);
            let mut lines = Vec::new();
            if let Some(text) = &popup.text {
                for line in text.split('\n') {
                    lines.push(Line::from(Span::raw(line.to_string())));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Esc cerrar", Style::new().dim())));
            let block = Block::bordered()
                .title(Span::styled(
                    format!(" {} ", popup.machine),
                    Style::new().fg(ACCENT).bold(),
                ))
                .border_style(Style::new().fg(ACCENT));
            frame.render_widget(
                Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
                box_area,
            );
            return;
        }
        PopupKind::Valoracion => {
            let box_area = popup_area(area, 56, 10);
            frame.render_widget(Clear, box_area);
            let mut lines = Vec::new();
            if let Some(text) = &popup.text {
                for line in text.split('\n') {
                    lines.push(Line::from(Span::raw(line.to_string())));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Enter valorar · Esc cerrar",
                Style::new().dim(),
            )));
            let block = Block::bordered()
                .title(Span::styled(
                    format!(" Valoración — {} ", popup.machine),
                    Style::new().fg(WARN).bold(),
                ))
                .border_style(Style::new().fg(WARN));
            frame.render_widget(Paragraph::new(lines).block(block), box_area);
            return;
        }
        _ => {}
    }

    let height = match popup.kind {
        PopupKind::Config => 10,
        PopupKind::Descarga => 7,
        PopupKind::WriteupSubmit => 9,
        PopupKind::RatingSubmit => 12,
        _ => 8,
    };
    // Room for the path-completion listing (max 6 candidates + header +
    // overflow notice).
    let completion_lines = if matches!(popup.kind, PopupKind::Descarga | PopupKind::CertDest)
        && !popup.completions.is_empty()
    {
        1 + popup.completions.len().min(6) + usize::from(popup.completions.len() > 6)
    } else {
        0
    };
    let height = height + completion_lines as u16 + u16::from(popup.notice.is_some());
    let width = match popup.kind {
        PopupKind::RatingSubmit => 64,
        _ => 74,
    };
    let box_area = popup_area(area, width, height);
    frame.render_widget(Clear, box_area);

    let (title, prompts, hint): (String, Vec<&str>, &str) = match popup.kind {
        PopupKind::Config => (
            " Configurar DockerLabs ".to_string(),
            vec!["Usuario:", "Contraseña:"],
            "Enter guardar y conectar · ↑↓/Tab cambiar campo · Esc salir",
        ),
        PopupKind::Descarga => (
            format!(" Descargar — {} ", popup.machine),
            vec!["Destino:"],
            "Tab completar ruta · Enter descargar · Esc cancelar",
        ),
        PopupKind::CertDest => (
            if popup.machine == "todos" {
                " Destino de los certificados ".to_string()
            } else {
                format!(" Destino del certificado — {} ", popup.machine)
            },
            vec!["Destino:"],
            "Tab completar ruta · Enter descargar · Esc cancelar",
        ),
        PopupKind::WriteupSubmit => (
            format!(" Enviar writeup — {} ", popup.machine),
            vec!["URL:", "Tipo (📝/🎥):"],
            "Enter enviar · ↑↓/Tab cambiar campo · Esc cancelar",
        ),
        PopupKind::RatingSubmit => (
            format!(" Valorar — {} ", popup.machine),
            vec![
                "Dificultad (1-5):",
                "Aprendizaje (1-5):",
                "Recomendaría (1-5):",
                "Diversión (1-5):",
            ],
            "Enter enviar · ↑↓/Tab cambiar campo · Esc cancelar",
        ),
        _ => unreachable!("info popups handled above"),
    };

    let mut lines = Vec::new();
    if let Some(notice) = &popup.notice {
        lines.push(Line::from(Span::styled(
            format!("⚠ {notice}"),
            Style::new().fg(WARN).bold(),
        )));
        lines.push(Line::from(""));
    }
    for (index, prompt) in prompts.iter().enumerate() {
        let active = index == popup.field;
        let marker = if active { "▏" } else { "" };
        let raw = popup.buffers.get(index).map(String::as_str).unwrap_or("");
        let buffer = if popup.kind == PopupKind::Config && index == 1 {
            // Mask the password field.
            "•".repeat(raw.chars().count())
        } else {
            raw.to_string()
        };
        let style = if active {
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().dim()
        };
        lines.push(Line::from(Span::styled(
            format!("{prompt} {buffer}{marker}"),
            style,
        )));
        if index + 1 < prompts.len() {
            lines.push(Line::from(""));
        }
    }
    lines.push(Line::from(""));
    // Path-completion listing (Tab in the Descarga popup), zsh style.
    if matches!(popup.kind, PopupKind::Descarga | PopupKind::CertDest)
        && !popup.completions.is_empty()
    {
        lines.push(Line::from(Span::styled(
            "  directorios:",
            Style::new().dim(),
        )));
        for name in popup.completions.iter().take(6) {
            lines.push(Line::from(Span::styled(
                format!("  {name}"),
                Style::new().fg(FROST),
            )));
        }
        let rest = popup.completions.len().saturating_sub(6);
        if rest > 0 {
            lines.push(Line::from(Span::styled(
                format!("  … y {rest} más"),
                Style::new().dim(),
            )));
        }
    }
    lines.push(Line::from(Span::styled(hint, Style::new().dim())));

    let border_color = match popup.kind {
        PopupKind::Config => WARN,
        _ => WARN,
    };
    let block = Block::bordered()
        .title(Span::styled(title, Style::new().fg(border_color).bold()))
        .border_style(Style::new().fg(border_color));

    frame.render_widget(Paragraph::new(lines).block(block), box_area);
}

fn draw_report(frame: &mut Frame, area: Rect, report: &ActionReport) {
    let height = (report.entries.len() as u16 * 2 + 4).clamp(5, 14);
    let width = 64;
    let box_area = popup_area(area, width, height);
    frame.render_widget(Clear, box_area);

    let mut lines = vec![Line::from("")];
    for (kind, text) in &report.entries {
        let span = match kind {
            ReportKind::Success => Span::styled(text.clone(), Style::new().fg(OK).bold()),
            ReportKind::Failure => Span::styled(text.clone(), Style::new().fg(BAD).bold()),
            ReportKind::Info => Span::styled(text.clone(), Style::new().fg(WARN)),
        };
        lines.push(Line::from(format!("  {span}")));
        lines.push(Line::from(""));
    }
    let footer_hint = if report.changed {
        "Los datos se refrescarán al cerrar · Enter / Esc cerrar"
    } else {
        "Enter / Esc cerrar"
    };
    lines.push(Line::from(Span::styled(footer_hint, Style::new().dim())));

    let block = Block::bordered()
        .title(Span::styled(
            report.title.clone(),
            Style::new().fg(ACCENT).bold(),
        ))
        .border_style(Style::new().fg(ACCENT));

    frame.render_widget(Paragraph::new(lines).block(block), box_area);
}

fn draw_writeups_popup(frame: &mut Frame, area: Rect, popup: &WriteupsPopup) {
    let rows: Vec<Row> = popup
        .entries
        .iter()
        .map(|w| {
            let tipo_style = if w.tipo.contains("🎥") {
                Style::new().fg(FROST)
            } else {
                Style::new().fg(PURPLE)
            };
            Row::new(vec![
                Span::styled(w.name.clone(), Style::new().fg(BRIGHT).bold()),
                Span::styled(w.tipo.clone(), tipo_style),
                Span::styled(w.url.clone(), Style::new().fg(LINK)),
            ])
        })
        .collect();

    let height = (popup.entries.len() as u16 + 3).clamp(6, 18);
    // Nearly full terminal width so writeup links stay readable.
    let width = area.width.saturating_sub(4).max(60);
    let box_area = popup_area(area, width, height);
    frame.render_widget(Clear, box_area);

    let header = Row::new(["Nombre", "Tipo", "URL"]).style(Style::new().fg(ACCENT).bold());
    let hint = Row::new([" ", " ", "Enter abrir · u enviar el tuyo · jk seleccionar · Esc cerrar"])
        .style(Style::new().dim());

    let table = Table::new(
        rows,
        [
            Constraint::Length(28),
            Constraint::Length(6),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .footer(hint)
    .row_highlight_style(Style::new().bg(HL_BG).add_modifier(Modifier::BOLD))
    .block(
        Block::bordered()
            .title(Span::styled(
                format!(" Writeups — {} ", popup.machine),
                Style::new().fg(ACCENT).bold(),
            ))
            .border_style(Style::new().fg(ACCENT)),
    );

    let mut state = TableState::default().with_selected(Some(popup.selected));
    frame.render_stateful_widget(table, box_area, &mut state);
}

fn draw_downloads(frame: &mut Frame, area: Rect, app: &AppState) {
    let jobs = app.download_jobs.len();
    let height = (jobs as u16 + 5).clamp(6, 16);
    let box_area = popup_area(area, 96, height);
    frame.render_widget(Clear, box_area);

    let mut lines: Vec<Line> = Vec::new();
    if jobs == 0 {
        lines.push(Line::from(Span::styled(
            "Sin descargas — pulsa d sobre una máquina.",
            Style::new().dim(),
        )));
    }

    let selected_index = if app.view == ViewMode::Downloads {
        app.download_selected()
    } else {
        usize::MAX
    };

    for (index, job) in app.download_jobs.iter().enumerate() {
        // Lock once and read everything through the guard: `is_active()`
        // and `download_selected()` would re-lock the same non-reentrant
        // std::Mutex while the guard is alive — self-deadlock.
        let state = job.state.lock().unwrap();
        let active = matches!(state.phase, Phase::Resolving | Phase::Downloading);
        let selected = index == selected_index;
        let marker = if selected && active {
            Span::styled("c ", Style::new().fg(WARN).bold())
        } else {
            Span::raw("  ")
        };

        let line = match state.phase {
            Phase::Resolving => Line::from(vec![
                marker,
                Span::styled(
                    format!("… {}  resolviendo enlace…", job.machine),
                    Style::new().dim(),
                ),
                Span::styled(
                    format!(
                        "  → {}",
                        super::downloads::shorten_path(&job.dest_dir)
                    ),
                    Style::new().dim(),
                ),
            ]),
            Phase::Downloading => {
                let ratio = if state.total > 0 {
                    state.downloaded as f64 / state.total as f64
                } else {
                    0.0
                };
                let filled = (ratio * 24.0).round() as usize;
                let bar = format!(
                    "[{}{}]",
                    "█".repeat(filled),
                    "░".repeat(24usize.saturating_sub(filled))
                );
                Line::from(vec![
                    marker,
                    Span::styled(
                        format!("↓ {:<14}", job.machine),
                        Style::new().fg(ACCENT).bold(),
                    ),
                    Span::styled(format!("{bar} "), Style::new().fg(ACCENT)),
                    Span::styled(
                        format!(
                            "{}/{} · {}/s",
                            super::downloads::fmt_bytes(state.downloaded),
                            super::downloads::fmt_bytes(state.total),
                            super::downloads::fmt_bytes(state.speed_bps),
                        ),
                        Style::new().fg(BRIGHT),
                    ),
                    Span::styled(
                        format!(
                            "  → {}",
                            super::downloads::shorten_path(&job.dest_dir)
                        ),
                        Style::new().dim(),
                    ),
                ])
            }
            Phase::Done => Line::from(vec![
                Span::raw("  "),
                Span::styled("✓ ", Style::new().fg(OK).bold()),
                Span::styled(
                    format!("{} → {}", job.machine, state.message),
                    Style::new().fg(OK),
                ),
            ]),
            Phase::Failed => Line::from(vec![
                Span::raw("  "),
                Span::styled("✗ ", Style::new().fg(BAD).bold()),
                Span::styled(
                    format!("{}: {}", job.machine, state.message),
                    Style::new().fg(BAD),
                ),
            ]),
            Phase::Cancelled => Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("• {} cancelada", job.machine), Style::new().dim()),
            ]),
        };
        lines.push(line);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "o cerrar (las descargas siguen) · c cancelar la última · q avisa al salir",
        Style::new().dim(),
    )));

    let block = Block::bordered()
        .title(Span::styled(" Descargas ", Style::new().fg(WARN).bold()))
        .border_style(Style::new().fg(WARN));
    frame.render_widget(Paragraph::new(lines).block(block), box_area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &AppState) {
    // Two rows so the key hints never get truncated: row 1 = context
    // actions, row 2 = global keys + status.
    let [actions, bottom] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let [global_area, status_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Min(24)]).areas(bottom);

    let actions_line: String = if app.popup.is_some() {
        match app.popup.as_ref().map(|p| p.kind) {
            Some(PopupKind::Config) => "Enter guardar y conectar · ↑↓/Tab cambiar campo · Esc salir".to_string(),
            Some(PopupKind::Account) => "Enter cambiar cuenta · l cerrar sesión · Esc cerrar".to_string(),
            Some(PopupKind::Valoracion) => "Enter valorar · Esc cerrar".to_string(),
            Some(PopupKind::Descripcion) => "Esc cerrar".to_string(),
            Some(PopupKind::Descarga) | Some(PopupKind::CertDest) => {
                "Tab completar ruta · Enter descargar · Esc cancelar".to_string()
            }
            _ => "Enter enviar · ↑↓/Tab cambiar campo · Esc cancelar".to_string(),
        }
    } else {
        match app.input_mode {
            InputMode::Filter => "Enter confirmar · Esc limpia y sale".to_string(),
            InputMode::Normal => match app.tab {
                Tab::Maquinas => "jk mover · / filtrar · s orden · d descargar · w writeups · v valorar · m completada · i info".to_string(),
                Tab::Progreso => "jk mover · / filtrar · Enter writeup · c cert · C todos".to_string(),
                Tab::Writeups => "jk mover · / filtrar · Enter abrir".to_string(),
            },
        }
    };
    frame.render_widget(
        Paragraph::new(Span::styled(actions_line, Style::new().dim())),
        actions,
    );

    let global = "Tab pestañas · a cuenta · o descargas · r refrescar · q salir";
    frame.render_widget(Paragraph::new(Span::styled(global, Style::new().dim())), global_area);

    let status = if let Some(label) = &app.fetching {
        Span::styled(format!("⟳ {label}"), Style::new().fg(WARN).bold())
    } else {
        match (&app.status, app.status_expiry) {
            (Some(message), Some(expiry)) if std::time::Instant::now() < expiry => {
                Span::styled(message.clone(), Style::new().fg(WARN))
            }
            _ => Span::raw(""),
        }
    };
    frame.render_widget(
        Paragraph::new(status).alignment(ratatui::layout::Alignment::Right),
        status_area,
    );
}

/// How many table rows fit in `height` (border 2 + header 1).
fn visible_rows_in(height: u16) -> usize {
    height.saturating_sub(3) as usize
}

fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Formats the rating averages shown in the Valoración popup.
pub fn format_rating(rating: &MachineRating) -> String {
    let mut lines = vec![
        format!("Dificultad    {:.1}", rating.details.dificultad),
        format!("Aprendizaje   {:.1}", rating.details.aprendizaje),
        format!("Recomendaría  {:.1}", rating.details.recomendaria),
        format!("Diversión     {:.1}", rating.details.diversion),
        format!(
            "Media: {:.1} ({} valoraciones)",
            rating.average, rating.count
        ),
    ];
    if let Some(user) = &rating.user_rating {
        lines.push(format!("Tu valoración: {}", user.summary()));
    }
    lines.join("\n")
}
