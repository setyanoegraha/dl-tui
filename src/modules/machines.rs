//! Machine catalog scraper for the dockerlabs.es homepage. The whole
//! catalog is server-rendered into `GET /` as `maquina-item` cards that
//! embed every field in inline `onclick` handlers:
//!   presentacion("Name","Dificultad","#color","autor","url","fecha","logo","")
//!   descripcion("Name","full description text")
//!   window.open('/maquinas/<id>/descargar','_blank')

use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::modules::session::DlSession;

#[derive(Debug, Clone, Serialize)]
pub struct Machine {
    pub id: u32,
    pub name: String,
    pub dificultad: String,
    pub clase: String,
    pub color: String,
    pub categoria: String,
    pub autor: String,
    pub autor_url: String,
    pub fecha: String,
    pub descripcion: String,
}

pub struct MachineScraper {
    session: DlSession,
}

impl MachineScraper {
    pub fn new(session: DlSession) -> Self {
        Self { session }
    }

    /// Fetches the homepage and parses the full catalog.
    pub async fn get_catalog(&self) -> Result<Vec<Machine>> {
        let html = self.session.get("/").await?;
        Ok(parse_catalog(&html))
    }
}

/// Extracts the quoted arguments of an inline JS call, e.g.
/// `presentacion("A","B")` -> ["A", "B"]. Handles escaped quotes and JS
/// `\uXXXX` unicode escapes (the site emits `F\u00e1cil` for "Fácil").
fn parse_quoted_args(onclick: &str) -> Vec<String> {
    let Some(start) = onclick.find('(') else {
        return Vec::new();
    };
    let mut args = Vec::new();
    let mut chars = onclick[start + 1..].chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ')' => break,
            ',' | ' ' => {
                chars.next();
            }
            quote @ ('"' | '\'') => {
                chars.next();
                let mut value = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == quote {
                        chars.next();
                        break;
                    }
                    chars.next();
                    if c2 == '\\' {
                        match chars.next() {
                            Some('u') => {
                                let mut hex = String::new();
                                while hex.len() < 4 {
                                    match chars.peek() {
                                        Some(&h) if h.is_ascii_hexdigit() => {
                                            hex.push(h);
                                            chars.next();
                                        }
                                        _ => break,
                                    }
                                }
                                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                    if let Some(decoded) = char::from_u32(code) {
                                        value.push(decoded);
                                    }
                                }
                            }
                            Some(escaped) => value.push(escaped),
                            None => break,
                        }
                        continue;
                    }
                    value.push(c2);
                }
                args.push(value);
            }
            _ => break, // malformed call: stop at the first unquoted char
        }
    }
    args
}

/// Extracts the numeric machine id from
/// `window.open('/maquinas/123/descargar','_blank')`.
fn parse_machine_id(onclick: &str) -> Option<u32> {
    let start = onclick.find("/maquinas/")? + "/maquinas/".len();
    let rest = &onclick[start..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Pure parsing function so it can be unit-tested against fixture HTML.
pub fn parse_catalog(html: &str) -> Vec<Machine> {
    let doc = scraper::Html::parse_document(html);
    let card_sel = scraper::Selector::parse(".maquina-item").unwrap();
    let onclick_sel = scraper::Selector::parse("[onclick]").unwrap();

    let mut machines = Vec::new();
    for card in doc.select(&card_sel) {
        let clase = card
            .value()
            .attr("class")
            .and_then(|c| c.split_whitespace().find(|t| *t != "maquina-item"))
            .unwrap_or("")
            .to_string();
        let categoria = card
            .value()
            .attr("data-category")
            .unwrap_or("")
            .to_string();

        let mut presentacion: Option<Vec<String>> = None;
        let mut descripcion: Option<String> = None;
        let mut id: Option<u32> = None;

        // The presentacion(...) call sits on the card element itself; the
        // description and download buttons are its children. Scan both.
        let mut onclicks: Vec<&str> = Vec::new();
        if let Some(own) = card.value().attr("onclick") {
            onclicks.push(own);
        }
        for element in card.select(&onclick_sel) {
            if let Some(onclick) = element.value().attr("onclick") {
                onclicks.push(onclick);
            }
        }

        for onclick in onclicks {
            if onclick.starts_with("presentacion(") {
                let args = parse_quoted_args(onclick);
                if args.len() >= 7 {
                    presentacion = Some(args);
                }
            } else if onclick.starts_with("descripcion(") {
                let args = parse_quoted_args(onclick);
                descripcion = args.into_iter().nth(1);
            } else if let Some(found) = parse_machine_id(onclick) {
                id = Some(found);
            }
        }

        // Build the machine only when every part was collected; element
        // order inside the card must not matter.
        let Some(args) = presentacion else {
            continue;
        };
        let machine = Machine {
            id: id.unwrap_or(0),
            name: args[0].clone(),
            dificultad: args[1].clone(),
            color: args[2].clone(),
            autor: args[3].clone(),
            autor_url: args[4].clone(),
            fecha: args[5].clone(),
            clase: clase.clone(),
            categoria: categoria.clone(),
            descripcion: descripcion.unwrap_or_default(),
        };
        if !machine.name.is_empty() {
            machines.push(machine);
        }
    }
    machines
}

/// "15/03/2025" -> (2025, 3, 15) for chronological sorting; unparseable
/// dates sort as the oldest possible.
pub fn fecha_sort_key(fecha: &str) -> (u32, u32, u32) {
    let parts: Vec<u32> = fecha
        .split('/')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    match parts.as_slice() {
        [d, m, y] => (*y, *m, *d),
        _ => (0, 0, 0),
    }
}

/// Difficulty rank for sorting (Muy Fácil first). Accent-insensitive.
pub fn dificultad_rank(dificultad: &str) -> u8 {
    HashMap::from([
        ("muy fácil", 0u8),
        ("muy facil", 0),
        ("fácil", 1),
        ("facil", 1),
        ("medio", 2),
        ("difícil", 3),
        ("dificil", 3),
    ])
    .get(dificultad.trim().to_lowercase().as_str())
    .copied()
    .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
    <html><body>
    <div class="maquina-item facil" data-category="Hacking Web">
      <div onclick="presentacion(&quot;Intranet&quot;,&quot;Fácil&quot;,&quot;#8bc34a&quot;,&quot;d1se0&quot;,&quot;https://github.com/d1se0&quot;,&quot;15/03/2025&quot;,&quot;/img/maquina/123&quot;,&quot;&quot;)">card</div>
      <div onclick="descripcion(&quot;Intranet&quot;,&quot;Una máquina fácil, con web y mucho más&quot;)">desc</div>
      <button onclick="window.open('/maquinas/123/descargar','_blank')">download</button>
    </div>
    <div class="maquina-item medio" data-category="Pivoting">
      <div onclick="presentacion(&quot;Dance Samba&quot;,&quot;Medio&quot;,&quot;#e0a553&quot;,&quot;El Pingüino&quot;,&quot;&quot;,&quot;01/12/2024&quot;,&quot;/img/maquina/99&quot;,&quot;&quot;)">card</div>
      <button onclick="window.open('/maquinas/99/descargar','_blank')">download</button>
    </div>
    </body></html>"#;

    #[test]
    fn parses_catalog_fixture() {
        let machines = parse_catalog(FIXTURE);
        assert_eq!(machines.len(), 2);

        assert_eq!(machines[0].id, 123);
        assert_eq!(machines[0].name, "Intranet");
        assert_eq!(machines[0].dificultad, "Fácil");
        assert_eq!(machines[0].clase, "facil");
        assert_eq!(machines[0].categoria, "Hacking Web");
        assert_eq!(machines[0].autor, "d1se0");
        assert_eq!(machines[0].fecha, "15/03/2025");
        // Commas inside the description must survive argument parsing.
        assert_eq!(machines[0].descripcion, "Una máquina fácil, con web y mucho más");

        assert_eq!(machines[1].id, 99);
        assert_eq!(machines[1].name, "Dance Samba");
        assert_eq!(machines[1].clase, "medio");
        assert!(machines[1].descripcion.is_empty());
    }

    #[test]
    fn quoted_args_handle_escapes_and_single_quotes() {
        assert_eq!(
            parse_quoted_args("f(\"a\\\"b\",\"c\")"),
            vec!["a\"b".to_string(), "c".to_string()]
        );
        // JS \uXXXX escapes decode to real characters (site emits F\u00e1cil).
        assert_eq!(
            parse_quoted_args("f(\"F\\u00e1cil\")"),
            vec!["Fácil".to_string()]
        );
        assert_eq!(
            parse_quoted_args("window.open('/maquinas/42/descargar', '_blank')"),
            vec!["/maquinas/42/descargar".to_string(), "_blank".to_string()]
        );
        assert!(parse_quoted_args("noargs()").is_empty());
    }

    /// Matches the live card shape: presentacion() on the card element with
    /// &#34; entities, \u escapes, trailing stopPropagation on buttons.
    #[test]
    fn parses_live_style_card() {
        let html = r#"
        <div onclick="presentacion(&#34;PipePwned&#34;, &#34;Medio&#34;, &#34;#e0a553&#34;, &#34;M4RC0Sx22&#34;, &#34;https://yt.example&#34;, &#34;11/08/2026&#34;, &#34;/img/maquina/281&#34;, &#34;&#34;)"
             class="maquina-item medio mas-reciente " data-category="">
          <span><strong>PipePwned</strong></span>
          <button onclick="descripcion(&#34;PipePwned&#34;, &#34;Basada en CI/CD, deber\u00e1s encadenar varias debilidades&#34;); event.stopPropagation();">desc</button>
          <button onclick="window.open('/maquinas/281/descargar', '_blank'); event.stopPropagation();">dl</button>
        </div>"#;
        let machines = parse_catalog(html);
        assert_eq!(machines.len(), 1);
        let m = &machines[0];
        assert_eq!(m.id, 281);
        assert_eq!(m.name, "PipePwned");
        assert_eq!(m.dificultad, "Medio");
        assert_eq!(m.clase, "medio");
        assert_eq!(m.autor, "M4RC0Sx22");
        assert_eq!(m.fecha, "11/08/2026");
        assert_eq!(m.descripcion, "Basada en CI/CD, deberás encadenar varias debilidades");
    }

    #[test]
    fn sorts_by_date_and_difficulty() {
        assert!(fecha_sort_key("15/03/2025") > fecha_sort_key("01/12/2024"));
        assert_eq!(fecha_sort_key("basura"), (0, 0, 0));
        assert!(dificultad_rank("Muy Fácil") < dificultad_rank("Fácil"));
        assert!(dificultad_rank("Fácil") < dificultad_rank("Medio"));
        assert!(dificultad_rank("Medio") < dificultad_rank("Difícil"));
        assert!(dificultad_rank("Muy Facil") == dificultad_rank("Muy Fácil"));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore] // live test: cargo test live_catalog_parses -- --ignored --nocapture
    async fn live_catalog_parses() {
        let client = reqwest::Client::builder()
            .user_agent(concat!("dl-tui/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap();
        let html = client
            .get("https://dockerlabs.es/")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let machines = parse_catalog(&html);
        println!("parsed {} máquinas", machines.len());
        assert!(
            machines.len() > 100,
            "el catálogo debería tener cientos de máquinas"
        );
        let first = &machines[0];
        println!(
            "primera: {} | {} | {} | {} | id={}",
            first.name, first.dificultad, first.autor, first.fecha, first.id
        );
        assert!(!first.name.is_empty());
        assert!(first.id > 0, "cada tarjeta lleva su id de descarga");
        let described = machines.iter().filter(|m| !m.descripcion.is_empty()).count();
        println!("con descripción: {described}");
        assert!(described > 50, "las descripciones deben parsearse");
    }
}
