//! Planilha ".xlsx" mínima: o Hub a monta e o executor a reabre para provar o conteúdo no destino.
//!
//! O formato é fixo e pequeno (ZIP sem compressão, texto em "inlineStr", sem fórmulas, estilos,
//! macros ou vínculos externos), porque quem escreve e quem confere é a própria Mexerica. A leitura
//! aceita só o que a escrita produz; um arquivo de outra origem é recusado, nunca interpretado.
use anyhow::{Result, anyhow, bail, ensure};
use flate2::Crc;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

pub const MAX_SHEETS: usize = 5;
pub const MAX_ROWS: usize = 500;
pub const MAX_COLS: usize = 20;
pub const MAX_TEXT: usize = 500;
pub const MAX_NAME: usize = 31;
const MAX_ENTRY: usize = 1 << 20;
pub const MAX_FILE: usize = 128 * 1024;
const PARTS: &[&str] = &["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Empty(()),
    Bool(bool),
    Number(f64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
}

/// O que o dono vê antes de aprovar e o que o Hub confere depois de gravar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub name: String,
    pub rows: usize,
    pub cells: usize,
}

pub fn summary(sheets: &[Sheet]) -> Vec<Summary> {
    sheets
        .iter()
        .map(|s| Summary {
            name: s.name.clone(),
            rows: s.rows.len(),
            cells: s.rows.iter().flatten().filter(|c| !matches!(c, Cell::Empty(()))).count(),
        })
        .collect()
}

/// Impressão digital do conteúdo tipado, igual no Hub (entrada) e no executor (arquivo reaberto).
pub fn content_sha256(sheets: &[Sheet]) -> Result<String> {
    let canonical = serde_json::to_vec(&normalize(sheets))?;
    Ok(hex(&crate::remote::crypto::sha256(&[&canonical])))
}

/// Células vazias no fim da linha não existem no arquivo; a forma canônica as descarta.
pub fn normalize(sheets: &[Sheet]) -> Vec<Sheet> {
    sheets
        .iter()
        .map(|s| Sheet {
            name: s.name.clone(),
            rows: s
                .rows
                .iter()
                .map(|r| {
                    let end = r.iter().rposition(|c| !matches!(c, Cell::Empty(()))).map_or(0, |i| i + 1);
                    r[..end].to_vec()
                })
                .collect(),
        })
        .collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

pub fn validate(sheets: &[Sheet]) -> Result<()> {
    ensure!((1..=MAX_SHEETS).contains(&sheets.len()), "invalid_arguments: de 1 a {MAX_SHEETS} abas");
    let mut seen = Vec::new();
    for s in sheets {
        let n = s.name.chars().count();
        ensure!(
            (1..=MAX_NAME).contains(&n)
                && !s
                    .name
                    .chars()
                    .any(|c| c.is_control() || "[]:*?/\\".contains(c) || c == '\u{fffe}' || c == '\u{ffff}')
                && !s.name.starts_with('\'')
                && !s.name.ends_with('\''),
            "invalid_arguments: nome de aba inválido (até {MAX_NAME} caracteres, sem [ ] : * ? / \\)"
        );
        let lower = s.name.to_lowercase();
        ensure!(!seen.contains(&lower), "invalid_arguments: nome de aba repetido");
        seen.push(lower);
        ensure!(s.rows.len() <= MAX_ROWS, "too_large: até {MAX_ROWS} linhas por aba");
        for row in &s.rows {
            ensure!(row.len() <= MAX_COLS, "too_large: até {MAX_COLS} colunas");
            for cell in row {
                match cell {
                    Cell::Number(n) => ensure!(n.is_finite(), "invalid_arguments: número inválido"),
                    Cell::Text(t) => ensure!(
                        t.chars().count() <= MAX_TEXT
                            && !t.chars().any(|c| (c.is_control() && !matches!(c, '\t' | '\n'))
                                || c == '\u{fffe}'
                                || c == '\u{ffff}'),
                        "invalid_arguments: texto de célula inválido (até {MAX_TEXT} caracteres, sem controle)"
                    ),
                    Cell::Bool(_) | Cell::Empty(()) => {}
                }
            }
        }
    }
    Ok(())
}

/// Nomes que o Windows trata como dispositivo, com ou sem extensão (CON.xlsx, COM¹): valem para arquivo e pasta.
fn windows_reserved(name: &str) -> bool {
    let first = name.split('.').next().unwrap_or_default().trim_end_matches(' ').to_uppercase();
    let numbered = |prefix: &str| {
        first.strip_prefix(prefix).is_some_and(|n| {
            let mut chars = n.chars();
            matches!((chars.next(), chars.next()), (Some('1'..='9' | '¹' | '²' | '³'), None))
        })
    };
    matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

/// Nome simples de arquivo ".xlsx": sem pasta, sem "..", sem nome oculto e sem nome reservado do Windows.
pub fn filename_ok(name: &str) -> bool {
    name.strip_suffix(".xlsx").is_some_and(|s| {
        (1..=64).contains(&s.chars().count())
            && !windows_reserved(name)
            && !s.starts_with('.')
            && !s.contains("..")
            && s.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
            && !s.ends_with(' ')
            && !s.starts_with(' ')
    })
}

/// Nome de subpasta: um só componente, sem nome reservado do Windows, sem ponto ou espaço nas pontas.
pub fn folder_ok(name: &str) -> bool {
    (1..=64).contains(&name.chars().count())
        && !windows_reserved(name)
        && !name.starts_with(['.', ' '])
        && !name.ends_with(['.', ' '])
        && !name.contains("..")
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            // `_xHHHH_` é escape do SpreadsheetML: o sublinhado literal que o antecede vira `_x005F_`.
            '_' if is_hex_escape(&text[i + 1..]) => out.push_str("_x005F_"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}
fn is_hex_escape(rest: &str) -> bool {
    let b = rest.as_bytes();
    b.len() >= 6 && b[0] == b'x' && b[1..5].iter().all(u8::is_ascii_hexdigit) && b[5] == b'_'
}
fn unescape(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let end = rest[at..].find(';').ok_or_else(|| anyhow!("xlsx: entidade incompleta"))? + at;
        out.push_str(match &rest[at + 1..end] {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "#10" => "\n",
            "#9" => "\t",
            other => bail!("xlsx: entidade desconhecida {other}"),
        });
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out.replace("_x005F_", "_"))
}
fn column(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + u8::try_from(index % 26).unwrap_or(0));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

fn sheet_xml(sheet: &Sheet) -> Result<String> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    for (r, row) in sheet.rows.iter().enumerate() {
        let _ = write!(xml, "<row r=\"{}\">", r + 1);
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{}", column(c), r + 1);
            match cell {
                Cell::Empty(()) => {}
                Cell::Text(t) => {
                    let _ = write!(
                        xml,
                        "<c r=\"{at}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                        escape(t)
                    );
                }
                Cell::Number(n) => {
                    let n = serde_json::Number::from_f64(*n)
                        .ok_or_else(|| anyhow!("invalid_arguments: número inválido"))?;
                    let _ = write!(xml, "<c r=\"{at}\"><v>{n}</v></c>");
                }
                Cell::Bool(b) => {
                    let _ = write!(xml, "<c r=\"{at}\" t=\"b\"><v>{}</v></c>", u8::from(*b));
                }
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    Ok(xml)
}

/// Monta o arquivo. Determinístico: mesma entrada, mesmos bytes.
pub fn build(sheets: &[Sheet]) -> Result<Vec<u8>> {
    validate(sheets)?;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>",
    );
    let mut book = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>",
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (i, sheet) in sheets.iter().enumerate() {
        let n = i + 1;
        let _ = write!(
            types,
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        );
        let _ = write!(book, "<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>", escape(&sheet.name));
        let _ = write!(
            rels,
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        );
        parts.push((format!("xl/worksheets/sheet{n}.xml"), sheet_xml(sheet)?.into_bytes()));
    }
    types.push_str("</Types>");
    book.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    let root = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    let mut all = vec![
        ("[Content_Types].xml".to_owned(), types.into_bytes()),
        ("_rels/.rels".to_owned(), root.as_bytes().to_vec()),
        ("xl/workbook.xml".to_owned(), book.into_bytes()),
        ("xl/_rels/workbook.xml.rels".to_owned(), rels.into_bytes()),
    ];
    all.append(&mut parts);
    let file = zip_store(&all)?;
    ensure!(file.len() <= MAX_FILE, "too_large: a planilha passa de 128 KiB");
    Ok(file)
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn crc(data: &[u8]) -> u32 {
    let mut c = Crc::new();
    c.update(data);
    c.sum()
}
fn zip_store(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = u32::try_from(out.len())?;
        let size = u32::try_from(data.len())?;
        let name_len = u16::try_from(name.len())?;
        let sum = crc(data);
        // Data e hora fixas (1980-01-01): o arquivo não depende do relógio.
        for (buf, central_header) in [(&mut out, false), (&mut central, true)] {
            le32(buf, if central_header { 0x0201_4b50 } else { 0x0403_4b50 });
            if central_header {
                le16(buf, 20);
            }
            le16(buf, 20);
            le16(buf, 0);
            le16(buf, 0);
            le16(buf, 0);
            le16(buf, 0x21);
            le32(buf, sum);
            le32(buf, size);
            le32(buf, size);
            le16(buf, name_len);
            le16(buf, 0);
            if central_header {
                le16(buf, 0);
                le16(buf, 0);
                le16(buf, 0);
                le32(buf, 0);
                le32(buf, offset);
            }
            buf.extend_from_slice(name.as_bytes());
        }
        out.extend_from_slice(data);
    }
    let central_at = u32::try_from(out.len())?;
    let central_len = u32::try_from(central.len())?;
    out.append(&mut central);
    le32(&mut out, 0x0605_4b50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, u16::try_from(entries.len())?);
    le16(&mut out, u16::try_from(entries.len())?);
    le32(&mut out, central_len);
    le32(&mut out, central_at);
    le16(&mut out, 0);
    Ok(out)
}

fn u16_at(b: &[u8], at: usize) -> Result<usize> {
    b.get(at..at + 2)
        .map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])))
        .ok_or_else(|| anyhow!("xlsx: arquivo truncado"))
}
fn u32_at(b: &[u8], at: usize) -> Result<usize> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        .ok_or_else(|| anyhow!("xlsx: arquivo truncado"))
}
/// Lê só entradas sem compressão, sem criptografia e sem descritor de dados, como a escrita produz.
fn zip_entries(file: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    ensure!(file.len() <= MAX_FILE, "xlsx: arquivo maior que o limite");
    let eocd = file.len().checked_sub(22).filter(|at| u32_at(file, *at).is_ok_and(|s| s == 0x0605_4b50));
    let eocd = eocd.ok_or_else(|| anyhow!("xlsx: não é um arquivo ZIP desta Mexerica"))?;
    ensure!(u16_at(file, eocd + 20)? == 0, "xlsx: comentário não permitido");
    let count = u16_at(file, eocd + 10)?;
    ensure!(count <= 16, "xlsx: partes demais");
    let mut at = u32_at(file, eocd + 16)?;
    let mut out = Vec::new();
    for _ in 0..count {
        ensure!(u32_at(file, at)? == 0x0201_4b50, "xlsx: diretório central inválido");
        ensure!(u16_at(file, at + 8)? & 0x0009 == 0 && u16_at(file, at + 10)? == 0, "xlsx: só partes sem compressão");
        let (sum, size, name_len, extra, comment, local) = (
            u32_at(file, at + 16)?,
            u32_at(file, at + 24)?,
            u16_at(file, at + 28)?,
            u16_at(file, at + 30)?,
            u16_at(file, at + 32)?,
            u32_at(file, at + 42)?,
        );
        ensure!(size <= MAX_ENTRY && extra == 0 && comment == 0, "xlsx: parte inválida");
        let name = std::str::from_utf8(
            file.get(at + 46..at + 46 + name_len).ok_or_else(|| anyhow!("xlsx: arquivo truncado"))?,
        )?;
        ensure!(!name.starts_with('/') && !name.contains("..") && !name.contains('\\'), "xlsx: nome de parte inválido");
        ensure!(u32_at(file, local)? == 0x0403_4b50 && u32_at(file, local + 22)? == size, "xlsx: parte local inválida");
        let start = local + 30 + u16_at(file, local + 26)? + u16_at(file, local + 28)?;
        let data = file.get(start..start + size).ok_or_else(|| anyhow!("xlsx: arquivo truncado"))?;
        ensure!(crc(data) as usize == sum, "xlsx: soma de verificação divergente em {name}");
        out.push((name.to_owned(), data.to_vec()));
        at += 46 + name_len;
    }
    ensure!(at == u32_at(file, eocd + 12)? + u32_at(file, eocd + 16)?, "xlsx: diretório central com sobra");
    Ok(out)
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = if tag.starts_with(&key) { key.len() } else { tag.find(&format!(" {key}"))? + 1 + key.len() };
    Some(&tag[start..start + tag[start..].find('"')?])
}

/// Reabre o arquivo e devolve o conteúdo tipado. Recusa qualquer coisa que a escrita não produziria.
pub fn inspect(file: &[u8]) -> Result<Vec<Sheet>> {
    let parts = zip_entries(file)?;
    let get = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, d)| std::str::from_utf8(d)).transpose();
    for needed in PARTS {
        ensure!(parts.iter().any(|(n, _)| n == needed), "xlsx: falta a parte {needed}");
    }
    let book = get("xl/workbook.xml")?.ok_or_else(|| anyhow!("xlsx: sem workbook"))?;
    let mut sheets = Vec::new();
    for (i, tag) in book.split("<sheet ").skip(1).enumerate() {
        let tag = tag.split("/>").next().unwrap_or_default();
        let name = unescape(attr(tag, "name").ok_or_else(|| anyhow!("xlsx: aba sem nome"))?)?;
        let xml =
            get(&format!("xl/worksheets/sheet{}.xml", i + 1))?.ok_or_else(|| anyhow!("xlsx: falta a aba {}", i + 1))?;
        let data = xml
            .split("<sheetData>")
            .nth(1)
            .and_then(|s| s.split("</sheetData>").next())
            .ok_or_else(|| anyhow!("xlsx: aba sem dados"))?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for row in data.split("<row ").skip(1) {
            let row = row.split("</row>").next().unwrap_or_default();
            let mut cells: Vec<Cell> = Vec::new();
            for cell in row.split("<c ").skip(1) {
                let head = cell.split('>').next().unwrap_or_default();
                let reference = attr(head, "r").ok_or_else(|| anyhow!("xlsx: célula sem posição"))?;
                let letters: String = reference.chars().take_while(char::is_ascii_uppercase).collect();
                ensure!(!letters.is_empty() && letters.len() <= 2, "xlsx: posição de célula inválida");
                let index = letters.bytes().fold(0usize, |acc, b| acc * 26 + usize::from(b - b'A' + 1)) - 1;
                ensure!(index >= cells.len() && index < MAX_COLS, "xlsx: posição de célula inválida");
                cells.resize(index, Cell::Empty(()));
                let value = |open: &str, close: &str| cell.split(open).nth(1).and_then(|s| s.split(close).next());
                cells.push(match attr(head, "t") {
                    Some("inlineStr") => Cell::Text(unescape(
                        value("xml:space=\"preserve\">", "</t>").ok_or_else(|| anyhow!("xlsx: texto ausente"))?,
                    )?),
                    Some("b") => Cell::Bool(value("<v>", "</v>") == Some("1")),
                    None => Cell::Number(value("<v>", "</v>").ok_or_else(|| anyhow!("xlsx: número ausente"))?.parse()?),
                    Some(other) => bail!("xlsx: tipo de célula não permitido: {other}"),
                });
            }
            rows.push(cells);
        }
        sheets.push(Sheet { name, rows });
    }
    ensure!(!sheets.is_empty(), "xlsx: sem abas");
    Ok(sheets)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn sample() -> Vec<Sheet> {
        vec![
            Sheet {
                name: "Vendas".into(),
                rows: vec![
                    vec![
                        Cell::Text("Produto".into()),
                        Cell::Text("Qtd".into()),
                        Cell::Text("Preço".into()),
                        Cell::Text("Ativo".into()),
                    ],
                    vec![
                        Cell::Text("Caneta <azul> & cia".into()),
                        Cell::Number(12.0),
                        Cell::Number(3.5),
                        Cell::Bool(true),
                    ],
                    vec![Cell::Empty(()), Cell::Number(-0.25), Cell::Empty(()), Cell::Bool(false)],
                ],
            },
            Sheet { name: "Notas".into(), rows: vec![vec![Cell::Text("=SOMA(A1)\nlinha\t2".into())]] },
        ]
    }

    #[test]
    fn writes_deterministically_and_reopens_to_the_same_typed_content() {
        let a = build(&sample()).unwrap();
        assert_eq!(a, build(&sample()).unwrap());
        assert_eq!(&a[..4], b"PK\x03\x04");
        let back = inspect(&a).unwrap();
        assert_eq!(content_sha256(&back).unwrap(), content_sha256(&sample()).unwrap());
        assert_eq!(back[0].rows[1][0], Cell::Text("Caneta <azul> & cia".into()));
        assert_eq!(back[1].rows[0][0], Cell::Text("=SOMA(A1)\nlinha\t2".into()));
        assert_eq!(summary(&back)[0], Summary { name: "Vendas".into(), rows: 3, cells: 10 });
    }

    #[test]
    fn formulas_stay_text_and_never_become_formula_cells() {
        let file = build(&sample()).unwrap();
        let parts = zip_entries(&file).unwrap();
        let sheet =
            std::str::from_utf8(&parts.iter().find(|(n, _)| n == "xl/worksheets/sheet2.xml").unwrap().1).unwrap();
        assert!(!sheet.contains("<f>") && sheet.contains("t=\"inlineStr\""));
    }

    #[test]
    fn rejects_bad_names_limits_and_foreign_or_tampered_files() {
        let one = |name: &str| Sheet { name: name.into(), rows: vec![] };
        for bad in ["", "a/b", "x[y]", "'abc", &"n".repeat(32), "tab\tx"] {
            assert!(build(&[one(bad)]).is_err(), "{bad:?}");
        }
        assert!(build(&[one("A"), one("a")]).is_err());
        assert!(build(&[Sheet { name: "A".into(), rows: vec![vec![Cell::Number(f64::NAN)]] }]).is_err());
        assert!(build(&[Sheet { name: "A".into(), rows: vec![vec![Cell::Empty(()); MAX_COLS + 1]] }]).is_err());
        assert!(build(&[Sheet { name: "A".into(), rows: vec![vec![Cell::Text("\u{0}".into())]] }]).is_err());
        let file = build(&sample()).unwrap();
        let mut tampered = file.clone();
        let at = tampered.windows(6).position(|w| w == b"Vendas").unwrap();
        tampered[at] = b'X';
        assert!(inspect(&tampered).is_err());
        assert!(
            inspect(b"PK\x05\x06").is_err()
                && inspect(b"nao e zip").is_err()
                && inspect(&file[..file.len() - 5]).is_err()
        );
    }

    #[test]
    fn literal_spreadsheetml_escape_sequences_stay_text_and_round_trip() {
        let texts = ["_x0041_", "_x005F_", "a_x0041_b_x005F_x0041_", "_x00ZZ_", "x_", "__x0041_"];
        let sheets =
            vec![Sheet { name: "Vendas".into(), rows: vec![texts.iter().map(|t| Cell::Text((*t).into())).collect()] }];
        let bytes = build(&sheets).unwrap();
        let xml = String::from_utf8_lossy(
            &zip_entries(&bytes).unwrap().into_iter().find(|(n, _)| n == "xl/worksheets/sheet1.xml").unwrap().1,
        )
        .into_owned();
        assert!(xml.contains(">_x005F_x0041_<"), "o Excel leria _x0041_ como A: {xml}");
        assert_eq!(inspect(&bytes).unwrap(), sheets);
    }

    #[test]
    fn filenames_are_plain_xlsx_without_paths_or_tricks() {
        for ok in ["Vendas.xlsx", "Relatório de teste 1.xlsx", "a-b_c.d.xlsx"] {
            assert!(filename_ok(ok), "{ok}");
        }
        for bad in [
            ".xlsx",
            "a/b.xlsx",
            "..\\x.xlsx",
            "x.xls",
            "x.xlsx.exe",
            ".oculto.xlsx",
            "a..b.xlsx",
            "c:x.xlsx",
            &format!("{}.xlsx", "n".repeat(65)),
        ] {
            assert!(!filename_ok(bad), "{bad}");
        }
        for bad in ["CON.xlsx", "nul.xlsx", "COM1.xlsx", "LPT¹.xlsx", "AUX .xlsx"] {
            assert!(!filename_ok(bad), "{bad}");
            assert!(!folder_ok(bad.trim_end_matches(".xlsx")), "{bad}");
        }
        assert!(filename_ok("Console.xlsx") && filename_ok("COM10.xlsx") && folder_ok("Comunicados"));
        let mut sheets = sample();
        sheets[0].name = "Vendas\u{fffe}".into();
        assert!(validate(&sheets).is_err(), "caractere inválido em XML não pode entrar no nome da aba");
    }
}
